/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::Cell;
use std::rc::Rc;
use std::thread;
use std::time::Duration;

#[cfg(target_os = "macos")]
use bwindow::Theme;
use bwindow::{CursorIcon, EventLoop, Window};
use reversi_engine::{Move, Player};

use crate::worker::AiWorker;
use crate::{AppEvent, Game};

const TIMEOUT: Duration = Duration::from_secs(15);
const OPENING: Move = Move { row: 2, col: 3 };

/// Self test run with `--smoke`, it plays against the AI and exits.
pub(crate) struct SmokeTest {
    started: bool,
    ai_done: bool,
    showed_progress_cursor: bool,
    #[cfg(target_os = "macos")]
    themes: u8,
    completed: Rc<Cell<bool>>,
}

impl SmokeTest {
    pub(crate) fn new(event_loop: &EventLoop<AppEvent>) -> Self {
        let watchdog = event_loop.create_proxy();
        thread::Builder::new()
            .name("smoke-watchdog".into())
            .spawn(move || {
                thread::sleep(TIMEOUT);
                let _ = watchdog.exit();
            })
            .expect("Can't spawn smoke watchdog thread");
        Self {
            started: false,
            ai_done: false,
            showed_progress_cursor: false,
            #[cfg(target_os = "macos")]
            themes: 0,
            completed: Rc::new(Cell::new(false)),
        }
    }

    pub(crate) fn completed(&self) -> Rc<Cell<bool>> {
        self.completed.clone()
    }

    pub(crate) fn after_draw(&mut self, game: &mut Game, worker: &AiWorker, window: &mut Window) {
        if !self.started {
            #[cfg(target_os = "macos")]
            window.set_theme(Theme::Dark);
            // Exercise cancellation/reset while one search is queued.
            assert!(game.play(OPENING));
            worker.submit(game.take_ai_job().expect("AI job"));
            game.reset();
            worker.cancel(game.revision);
            assert!(game.play(OPENING));
            self.started = true;
        } else if self.ai_done {
            assert!(self.showed_progress_cursor, "loading cursor was never set");
            #[cfg(target_os = "macos")]
            if self.themes != 3 {
                window.request_redraw();
                return;
            }
            assert!(game.board.score(Player::Black) + game.board.score(Player::White) >= 6);
            self.completed.set(true);
            window.close();
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn theme_changed(&mut self, theme: Theme, window: &mut Window) {
        if !self.started {
            return;
        }
        match theme {
            Theme::Dark => {
                self.themes |= 1;
                window.set_theme(Theme::Light);
            }
            Theme::Light => self.themes |= 2,
        }
    }

    pub(crate) const fn ai_applied(&mut self) {
        self.ai_done = true;
    }

    pub(crate) fn cursor_set(&mut self, cursor: CursorIcon) {
        self.showed_progress_cursor |= cursor == CursorIcon::Progress;
    }
}
