/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;

use bwebview::{WebviewBuilder, WebviewEvent};
use bwindow::{Event, EventLoopBuilder, LogicalSize, Theme, WindowBuilder};
use rust_embed::Embed;

use crate::ipc::{IpcPush, IpcRequest, PROGRESS_EVENT_PREFIX, update_progress};
use crate::worker::{OPERATION_CLEAN, OPERATION_IDLE, OPERATION_SCAN, WorkerCommand};

mod catalog;
mod elevation;
mod engine;
mod ipc;
mod pattern;
mod win32;
mod worker;

#[derive(Embed)]
#[folder = "web"]
struct WebAssets;

pub(crate) enum AppEvent {
    Webview(bwindow::WindowId, WebviewEvent),
    UserEvent(String),
}

const fn background_color(theme: Theme) -> u32 {
    match theme {
        Theme::Light => 0xffffff,
        Theme::Dark => 0x222222,
    }
}

fn main() {
    elevation::wait_for_parent_if_requested();

    let is_administrator = elevation::is_process_elevated();
    let event_loop = EventLoopBuilder::new()
        .with_user_event::<AppEvent>()
        .app_id("nl", "bplaat", "Binman")
        .build();
    let proxy = Arc::new(event_loop.create_proxy());
    let cancelled = Arc::new(AtomicBool::new(false));
    let operation_state = Arc::new(AtomicU8::new(OPERATION_IDLE));
    let (sender, receiver) = mpsc::channel();
    let worker_proxy = Arc::clone(&proxy);
    let worker_cancelled = Arc::clone(&cancelled);
    let worker_operation_state = Arc::clone(&operation_state);
    thread::Builder::new()
        .name("worker".to_string())
        .spawn(move || {
            worker::run(
                receiver,
                worker_proxy,
                worker_cancelled,
                worker_operation_state,
                is_administrator,
            );
        })
        .expect("Failed to spawn worker thread");

    let mut window = WindowBuilder::new()
        .title("Binman")
        .size(LogicalSize::new(1080.0, 720.0))
        .min_size(LogicalSize::new(820.0, 560.0))
        .background_color(background_color(event_loop.theme()))
        .remember_window_state()
        .center()
        .build();

    let mut webview = WebviewBuilder::new(&window)
        .on_event(event_loop.create_proxy(), AppEvent::Webview)
        .load_rust_embed::<WebAssets>()
        .build();

    event_loop.run(move |event| match event {
        Event::Window(_, bwindow::WindowEvent::ThemeChanged(theme)) => {
            window.set_background_color(background_color(theme));
        }
        Event::UserEvent(AppEvent::UserEvent(json)) => {
            if let Some(json) = json.strip_prefix(PROGRESS_EVENT_PREFIX) {
                if let Ok(state) = serde_json::from_str(json) {
                    update_progress(&mut window, state);
                }
            } else {
                webview.send_ipc_message(json);
            }
        }
        Event::UserEvent(AppEvent::Webview(_window_id, WebviewEvent::PageTitleChange(title))) => {
            window.set_title(title)
        }
        Event::UserEvent(AppEvent::Webview(_window_id, WebviewEvent::MessageReceive(message))) => {
            match serde_json::from_str::<IpcRequest>(&message) {
                Ok(request) => match request {
                    IpcRequest::Initialize => {
                        if sender.send(WorkerCommand::Initialize).is_err() {
                            let push = IpcPush::FatalError {
                                message: "The cleanup worker is unavailable".to_string(),
                            };
                            if let Ok(json) = serde_json::to_string(&push) {
                                webview.send_ipc_message(json);
                            }
                        }
                    }
                    IpcRequest::RestartElevated => {
                        let result = if operation_state.load(Ordering::Acquire) == OPERATION_IDLE {
                            elevation::restart()
                        } else {
                            Err(std::io::Error::other(
                                "Wait for the current operation before restarting",
                            ))
                        };
                        match result {
                            Ok(()) => window.close(),
                            Err(error) => {
                                let push = IpcPush::ElevationError {
                                    message: error.to_string(),
                                };
                                if let Ok(json) = serde_json::to_string(&push) {
                                    webview.send_ipc_message(json);
                                }
                            }
                        }
                    }
                    IpcRequest::StartScan => {
                        if operation_state
                            .compare_exchange(
                                OPERATION_IDLE,
                                OPERATION_SCAN,
                                Ordering::AcqRel,
                                Ordering::Acquire,
                            )
                            .is_ok()
                        {
                            cancelled.store(false, Ordering::Release);
                            if sender.send(WorkerCommand::Scan).is_err() {
                                operation_state.store(OPERATION_IDLE, Ordering::Release);
                                let push = IpcPush::FatalError {
                                    message: "The cleanup worker is unavailable".to_string(),
                                };
                                if let Ok(json) = serde_json::to_string(&push) {
                                    webview.send_ipc_message(json);
                                }
                            }
                        }
                    }
                    IpcRequest::CancelScan => {
                        cancelled.store(true, Ordering::Relaxed);
                    }
                    IpcRequest::StartClean { cleanup_ids } => {
                        if operation_state
                            .compare_exchange(
                                OPERATION_IDLE,
                                OPERATION_CLEAN,
                                Ordering::AcqRel,
                                Ordering::Acquire,
                            )
                            .is_ok()
                            && sender.send(WorkerCommand::Clean(cleanup_ids)).is_err()
                        {
                            operation_state.store(OPERATION_IDLE, Ordering::Release);
                            let push = IpcPush::FatalError {
                                message: "The cleanup worker is unavailable".to_string(),
                            };
                            if let Ok(json) = serde_json::to_string(&push) {
                                webview.send_ipc_message(json);
                            }
                        }
                    }
                },
                Err(error) => {
                    let push = IpcPush::FatalError {
                        message: format!("Invalid application request: {error}"),
                    };
                    if let Ok(json) = serde_json::to_string(&push) {
                        webview.send_ipc_message(json);
                    }
                }
            }
        }
        _ => {}
    });
}
