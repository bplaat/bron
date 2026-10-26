/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use bwindow::EventLoopProxy;
use preferences::{Preferences, Value};
use reversi_engine::Player;

use crate::{AppEvent, Game};

const BOARD: &str = "board";
const TURN: &str = "turn";
// Coalesce the human and computer moves of a turn into one write
const SAVE_DELAY: Duration = Duration::from_secs(1);

/// Persists the game in the shared preferences, delayed so not every change writes.
pub(crate) struct GameSaver {
    prefs: Preferences,
    proxy: Arc<EventLoopProxy<AppEvent>>,
    dirty: bool,
    scheduled: bool,
}

impl GameSaver {
    pub(crate) fn new(proxy: EventLoopProxy<AppEvent>) -> Self {
        Self {
            prefs: Preferences::new("nl.bplaat.Reversi").expect("Failed to open preferences"),
            proxy: Arc::new(proxy),
            dirty: false,
            scheduled: false,
        }
    }

    /// Load the saved game, or start a new one when there is none.
    pub(crate) fn load(&self) -> Game {
        let board = match self.prefs.get(BOARD) {
            Ok(Some(Value::String(board))) => board.parse().ok(),
            _ => None,
        };
        let turn = match self.prefs.get(TURN) {
            Ok(Some(Value::String(turn))) => match turn.as_str() {
                "black" => Some(Player::Black),
                "white" => Some(Player::White),
                _ => None,
            },
            _ => None,
        };
        let (Some(board), Some(turn)) = (board, turn) else {
            return Game::default();
        };
        let mut game = Game {
            board,
            turn,
            ..Game::default()
        };
        // Restores the game over state, saved games already passed when needed
        game.advance_passes();
        game
    }

    /// Mark the game changed and send `AppEvent::Save` after the save delay.
    pub(crate) fn schedule(&mut self) {
        self.dirty = true;
        if !self.scheduled {
            self.scheduled = true;
            let proxy = Arc::clone(&self.proxy);
            thread::spawn(move || {
                thread::sleep(SAVE_DELAY);
                let _ = proxy.send_user_event(AppEvent::Save);
            });
        }
    }

    /// Handle `AppEvent::Save`.
    pub(crate) fn scheduled_save(&mut self, game: &Game) {
        self.scheduled = false;
        self.save(game);
    }

    /// Write the game when it changed since the last save.
    pub(crate) fn save(&mut self, game: &Game) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let turn = match game.turn {
            Player::Black => "black",
            Player::White => "white",
        };
        if let Err(error) = self
            .prefs
            .set(BOARD, game.board.to_string())
            .and_then(|()| self.prefs.set(TURN, turn))
        {
            eprintln!("Failed to save game: {error}");
        }
    }
}
