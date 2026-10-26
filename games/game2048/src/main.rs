/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use bwebview::{WebviewBuilder, WebviewEvent};
use bwindow::{Event, EventLoopBuilder, LogicalSize, Theme, WindowBuilder, WindowEvent};
use preferences::{Preferences, Value};
use rust_embed::Embed;
use serde::{Deserialize, Serialize};

#[derive(Embed)]
#[folder = "web"]
struct WebAssets;

const BEST_SCORE: &str = "best_score";
const GAME_STATE: &str = "game_state";
// Coalesce the state changes of every move into one write
const SAVE_DELAY: Duration = Duration::from_secs(1);

const fn background_color(theme: Theme) -> u32 {
    match theme {
        Theme::Light => 0xfaf8ef,
        Theme::Dark => 0x1b1814,
    }
}

// MARK: IPC messages
#[derive(Deserialize, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum IpcMessage {
    Ready,
    State(State),
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct State {
    best_score: i64,
    game_state: Option<serde_json::Value>,
}

impl State {
    fn load(prefs: &Preferences) -> Self {
        Self {
            best_score: match prefs.get(BEST_SCORE) {
                Ok(Some(Value::Integer(score))) => score,
                _ => 0,
            },
            game_state: match prefs.get(GAME_STATE) {
                Ok(Some(Value::String(state))) => serde_json::from_str(&state).ok(),
                _ => None,
            },
        }
    }

    fn save(&self, prefs: &Preferences) {
        let result = prefs
            .set(BEST_SCORE, self.best_score)
            .and_then(|()| match &self.game_state {
                Some(state) => prefs.set(GAME_STATE, state.to_string()),
                None => prefs.remove(GAME_STATE),
            });
        if let Err(error) = result {
            eprintln!("Failed to save game state: {error}");
        }
    }
}

fn save_pending(pending_state: &mut Option<State>, prefs: &Preferences) {
    if let Some(state) = pending_state.take() {
        state.save(prefs);
    }
}

// MARK: Main
enum AppEvent {
    Webview(bwindow::WindowId, WebviewEvent),
    Save,
}

fn main() {
    let event_loop = EventLoopBuilder::new()
        .with_user_event::<AppEvent>()
        .app_id("nl", "bplaat", "game2048")
        .build();
    let event_loop_proxy = Arc::new(event_loop.create_proxy());

    #[allow(unused_mut)]
    let mut window_builder = WindowBuilder::new()
        .title("2048")
        .size(LogicalSize::new(600.0, 850.0))
        .resizable(false)
        .background_color(background_color(event_loop.theme()))
        .center()
        .remember_window_state();
    #[cfg(target_os = "macos")]
    {
        window_builder = window_builder.macos_titlebar_style(bwindow::MacosTitlebarStyle::Hidden);
    }
    let mut window = window_builder.build();

    let prefs = Preferences::new("nl.bplaat.game2048").expect("Failed to open preferences");
    let mut webview = WebviewBuilder::new(&window)
        .on_event(event_loop.create_proxy(), AppEvent::Webview)
        .load_rust_embed::<WebAssets>()
        .build();

    #[cfg(target_os = "macos")]
    webview.add_user_script(
        format!(
            "document.documentElement.style.setProperty('--macos-titlebar-height', '{}px');",
            window.macos_titlebar_size().height
        ),
        bwebview::InjectionTime::DocumentStart,
    );

    let mut pending_state: Option<State> = None;
    let mut save_scheduled = false;
    event_loop.run(move |event| match event {
        Event::Window(_, WindowEvent::ThemeChanged(theme)) => {
            window.set_background_color(background_color(theme));
        }
        Event::UserEvent(AppEvent::Webview(_window_id, WebviewEvent::MessageReceive(message))) => {
            match serde_json::from_str(&message) {
                Ok(IpcMessage::Ready) => {
                    save_pending(&mut pending_state, &prefs);
                    let state = State::load(&prefs);
                    webview.send_ipc_message(
                        serde_json::to_string(&IpcMessage::State(state))
                            .expect("Failed to serialize state"),
                    );
                }
                Ok(IpcMessage::State(state)) => {
                    pending_state = Some(state);
                    if !save_scheduled {
                        save_scheduled = true;
                        let event_loop_proxy = Arc::clone(&event_loop_proxy);
                        thread::spawn(move || {
                            thread::sleep(SAVE_DELAY);
                            let _ = event_loop_proxy.send_user_event(AppEvent::Save);
                        });
                    }
                }
                Err(error) => eprintln!("Ignoring invalid IPC message: {error}"),
            }
        }
        Event::UserEvent(AppEvent::Save) => {
            save_scheduled = false;
            save_pending(&mut pending_state, &prefs);
        }
        // Save right away when the app may be suspended or killed
        Event::Window(_, WindowEvent::Blur) | Event::LoopExiting => {
            save_pending(&mut pending_state, &prefs);
        }
        _ => {}
    });
}
