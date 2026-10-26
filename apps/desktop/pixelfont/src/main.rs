/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use bwebview::{WebviewBuilder, WebviewEvent};
use bwindow::{
    Accelerator, Event, EventLoopBuilder, FileDialog, KeyCode, LogicalSize, MenuBarBuilder,
    MenuBuilder, MenuItem, MessageButtons, MessageDialog, MessageDialogResult, MessageLevel,
    Modifiers, Theme, WindowBuilder, WindowEvent,
};
use rust_embed::Embed;
use serde::{Deserialize, Serialize};

const FONT_SIZE: usize = 256 * 8;

#[derive(Embed)]
#[folder = "web"]
struct WebAssets;

// MARK: IPC messages
#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum IpcMessage {
    Ready,
    ShowError {
        title: String,
        description: String,
    },
    ShowErrorResponse,
    MenuAction {
        action: String,
    },
    RestoreLastFile,
    OpenFile {
        path: String,
    },
    OpenFileDialog,
    OpenFileDialogResponse {
        path: Option<String>,
    },
    OpenFont {
        path: String,
    },
    OpenFontResponse {
        ok: bool,
        data: Option<Vec<u8>>,
        error: Option<String>,
    },
    SaveFileDialog {
        filename: String,
    },
    SaveFileDialogResponse {
        path: Option<String>,
    },
    SaveFont {
        path: String,
        data: Vec<u8>,
    },
    SaveFontResponse {
        ok: bool,
        error: Option<String>,
    },
    ExportFileDialog {
        filename: String,
    },
    ExportFileDialogResponse {
        path: Option<String>,
    },
    ExportFile {
        path: String,
        text: String,
    },
    ExportFileResponse {
        ok: bool,
        error: Option<String>,
    },
    ConfirmSaveChanges {
        filename: String,
    },
    ConfirmSaveChangesResponse {
        choice: String,
    },
    CloseRequested,
    CloseWindow,
    #[cfg(target_os = "macos")]
    MacosDocumentEdited {
        edited: bool,
    },
}

// MARK: Main
pub(crate) enum AppEvent {
    Webview(bwindow::WindowId, WebviewEvent),
}

const fn background_color(theme: Theme) -> u32 {
    match theme {
        Theme::Light => 0xffffff,
        Theme::Dark => 0x222222,
    }
}

fn main() {
    let startup_path = if cfg!(any(target_os = "macos", windows)) {
        std::env::args().nth(1)
    } else {
        None
    };
    let event_loop = EventLoopBuilder::new()
        .with_user_event::<AppEvent>()
        .app_id("nl", "bplaat", "PixelFontEditor")
        .single_instance(false)
        .menu(
            MenuBarBuilder::new()
                .menu(
                    MenuBuilder::new("File")
                        .item(
                            MenuItem::new("New", "new")
                                .accelerator(Accelerator::new(Modifiers::PRIMARY, KeyCode::KeyN)),
                        )
                        .item(
                            MenuItem::new("Open…", "open")
                                .accelerator(Accelerator::new(Modifiers::PRIMARY, KeyCode::KeyO)),
                        )
                        .item(
                            MenuItem::new("Save", "save")
                                .accelerator(Accelerator::new(Modifiers::PRIMARY, KeyCode::KeyS)),
                        )
                        .item(
                            MenuItem::new("Save As…", "saveAs").accelerator(Accelerator::new(
                                Modifiers::PRIMARY | Modifiers::SHIFT,
                                KeyCode::KeyS,
                            )),
                        )
                        .separator()
                        .item(
                            MenuItem::new("Export as Assembly…", "exportAsm").accelerator(
                                Accelerator::new(
                                    Modifiers::PRIMARY | Modifiers::OPTION,
                                    KeyCode::KeyA,
                                ),
                            ),
                        )
                        .item(MenuItem::new("Export as C Header…", "exportC").accelerator(
                            Accelerator::new(Modifiers::PRIMARY | Modifiers::OPTION, KeyCode::KeyC),
                        ))
                        .separator(),
                )
                .menu(
                    MenuBuilder::new("Edit")
                        .item(
                            MenuItem::new("Copy", "copy")
                                .accelerator(Accelerator::new(Modifiers::PRIMARY, KeyCode::KeyC)),
                        )
                        .item(
                            MenuItem::new("Paste", "paste")
                                .accelerator(Accelerator::new(Modifiers::PRIMARY, KeyCode::KeyV)),
                        ),
                )
                .menu(
                    MenuBuilder::new("Character")
                        .item(
                            MenuItem::new("Clear", "clear").accelerator(Accelerator::new(
                                Modifiers::PRIMARY,
                                KeyCode::Backspace,
                            )),
                        )
                        .item(
                            MenuItem::new("Invert", "invert")
                                .accelerator(Accelerator::new(Modifiers::PRIMARY, KeyCode::KeyI)),
                        )
                        .item(
                            MenuItem::new("Rotate", "rotate")
                                .accelerator(Accelerator::new(Modifiers::PRIMARY, KeyCode::KeyR)),
                        )
                        .item(MenuItem::new("Mirror Horizontally", "mirrorH").accelerator(
                            Accelerator::new(Modifiers::PRIMARY | Modifiers::SHIFT, KeyCode::KeyH),
                        ))
                        .item(MenuItem::new("Mirror Vertically", "mirrorV").accelerator(
                            Accelerator::new(Modifiers::PRIMARY | Modifiers::SHIFT, KeyCode::KeyV),
                        ))
                        .separator()
                        .item(
                            MenuItem::new("Clear All", "clearAll").accelerator(Accelerator::new(
                                Modifiers::PRIMARY | Modifiers::OPTION,
                                KeyCode::Backspace,
                            )),
                        )
                        .item(MenuItem::new("Invert All", "invertAll").accelerator(
                            Accelerator::new(Modifiers::PRIMARY | Modifiers::OPTION, KeyCode::KeyI),
                        ))
                        .item(MenuItem::new("Rotate All", "rotateAll").accelerator(
                            Accelerator::new(Modifiers::PRIMARY | Modifiers::OPTION, KeyCode::KeyR),
                        ))
                        .item(
                            MenuItem::new("Mirror All Horizontally", "mirrorHAll").accelerator(
                                Accelerator::new(
                                    Modifiers::PRIMARY | Modifiers::OPTION | Modifiers::SHIFT,
                                    KeyCode::KeyH,
                                ),
                            ),
                        )
                        .item(
                            MenuItem::new("Mirror All Vertically", "mirrorVAll").accelerator(
                                Accelerator::new(
                                    Modifiers::PRIMARY | Modifiers::OPTION | Modifiers::SHIFT,
                                    KeyCode::KeyV,
                                ),
                            ),
                        ),
                ),
        )
        .build();

    #[allow(unused_mut)]
    let mut window = WindowBuilder::new()
        .title("8x8 Pixel Font Editor")
        .size(LogicalSize::new(640.0, 860.0))
        .min_size(LogicalSize::new(640.0, 520.0))
        .background_color(background_color(event_loop.theme()))
        .center()
        .remember_window_state()
        .allow_file_drop(true)
        .build();

    let mut webview = WebviewBuilder::new(&window)
        .on_event(event_loop.create_proxy(), AppEvent::Webview)
        .load_rust_embed::<WebAssets>()
        .build();

    let mut page_ready = false;
    let mut pending_open_path = startup_path;
    let mut pending_menu_action: Option<String> = None;
    event_loop.run(move |event| {
        if let Event::Window(_, WindowEvent::ThemeChanged(theme)) = &event {
            window.set_background_color(background_color(*theme));
        }
        if let Event::UserEvent(AppEvent::Webview(_window_id, WebviewEvent::PageLoadStart)) = &event
        {
            page_ready = false;
        }
        if let Event::OpenFiles(paths) = &event
            && let Some(path) = paths.first()
        {
            let path = path.to_string_lossy().into_owned();
            if page_ready {
                webview.send_ipc_message(
                    serde_json::to_string(&IpcMessage::OpenFile { path })
                        .expect("Failed to serialize open file message"),
                );
            } else {
                pending_open_path = Some(path);
            }
        }
        if let Event::UserEvent(AppEvent::Webview(
            _window_id,
            WebviewEvent::PageTitleChange(title),
        )) = &event
        {
            window.set_title(title);
        }
        if let Event::Action(action) = &event {
            if page_ready {
                webview.send_ipc_message(
                    serde_json::to_string(&IpcMessage::MenuAction {
                        action: action.clone(),
                    })
                    .expect("Failed to serialize menu action"),
                );
            } else {
                pending_menu_action = Some(action.clone());
            }
        }
        if let Event::Window(_, WindowEvent::CloseRequested(request)) = &event {
            request.prevent_default();
            if page_ready {
                webview.send_ipc_message(
                    serde_json::to_string(&IpcMessage::CloseRequested)
                        .expect("Failed to serialize close request"),
                );
            } else {
                window.close();
            }
        }
        if let Event::Window(_, WindowEvent::DroppedFile(path)) = &event {
            let path = path.to_string_lossy().into_owned();
            if page_ready {
                webview.send_ipc_message(
                    serde_json::to_string(&IpcMessage::OpenFile { path })
                        .expect("Failed to serialize open file message"),
                );
            } else {
                pending_open_path = Some(path);
            }
        }
        if let Event::UserEvent(AppEvent::Webview(
            _window_id,
            WebviewEvent::MessageReceive(message),
        )) = event
        {
            let Ok(ipc_message) = serde_json::from_str::<IpcMessage>(&message) else {
                return;
            };
            match ipc_message {
                IpcMessage::Ready => {
                    page_ready = true;
                    if let Some(path) = pending_open_path.take() {
                        webview.send_ipc_message(
                            serde_json::to_string(&IpcMessage::OpenFile { path })
                                .expect("Failed to serialize open file message"),
                        );
                    } else {
                        webview.send_ipc_message(
                            serde_json::to_string(&IpcMessage::RestoreLastFile)
                                .expect("Failed to serialize restore last file message"),
                        );
                    }
                    if let Some(action) = pending_menu_action.take() {
                        webview.send_ipc_message(
                            serde_json::to_string(&IpcMessage::MenuAction { action })
                                .expect("Failed to serialize menu action"),
                        );
                    }
                }
                IpcMessage::RestoreLastFile => {}
                IpcMessage::ShowError { title, description } => {
                    MessageDialog::new()
                        .parent(&window)
                        .title(title)
                        .description(description)
                        .level(MessageLevel::Error)
                        .show();
                    webview.send_ipc_message(
                        serde_json::to_string(&IpcMessage::ShowErrorResponse)
                            .expect("Failed to serialize response"),
                    );
                }
                IpcMessage::OpenFileDialog => {
                    let path = FileDialog::new()
                        .parent(&window)
                        .title("Open Pixel Font File")
                        .add_filter("Pixel Font files", &["pf"])
                        .pick_file()
                        .map(|p| p.to_string_lossy().into_owned());
                    let response = IpcMessage::OpenFileDialogResponse { path };
                    webview.send_ipc_message(
                        serde_json::to_string(&response).expect("Failed to serialize response"),
                    );
                }
                IpcMessage::OpenFont { path } => {
                    let response = match std::fs::read(&path) {
                        Ok(bytes) if bytes.len() >= FONT_SIZE => IpcMessage::OpenFontResponse {
                            ok: true,
                            data: Some(bytes[..FONT_SIZE].to_vec()),
                            error: None,
                        },
                        Ok(_) => IpcMessage::OpenFontResponse {
                            ok: false,
                            data: None,
                            error: Some("File is too small (need at least 2048 bytes)".to_string()),
                        },
                        Err(e) => IpcMessage::OpenFontResponse {
                            ok: false,
                            data: None,
                            error: Some(e.to_string()),
                        },
                    };
                    webview.send_ipc_message(
                        serde_json::to_string(&response).expect("Failed to serialize response"),
                    );
                }
                IpcMessage::SaveFileDialog { filename } => {
                    let path = FileDialog::new()
                        .parent(&window)
                        .title("Save Pixel Font File")
                        .file_name(&filename)
                        .add_filter("Pixel Font files", &["pf"])
                        .save_file()
                        .map(|p| p.to_string_lossy().into_owned());
                    let response = IpcMessage::SaveFileDialogResponse { path };
                    webview.send_ipc_message(
                        serde_json::to_string(&response).expect("Failed to serialize response"),
                    );
                }
                IpcMessage::SaveFont { path, data } => {
                    let response = if data.len() != FONT_SIZE {
                        IpcMessage::SaveFontResponse {
                            ok: false,
                            error: Some(format!(
                                "Invalid font data size: expected {FONT_SIZE} bytes, got {}",
                                data.len()
                            )),
                        }
                    } else {
                        match std::fs::write(&path, &data) {
                            Ok(()) => IpcMessage::SaveFontResponse {
                                ok: true,
                                error: None,
                            },
                            Err(e) => IpcMessage::SaveFontResponse {
                                ok: false,
                                error: Some(e.to_string()),
                            },
                        }
                    };
                    webview.send_ipc_message(
                        serde_json::to_string(&response).expect("Failed to serialize response"),
                    );
                }
                IpcMessage::ExportFileDialog { filename } => {
                    let path = FileDialog::new()
                        .parent(&window)
                        .title("Export Font File")
                        .file_name(&filename)
                        .save_file()
                        .map(|p| p.to_string_lossy().into_owned());
                    let response = IpcMessage::ExportFileDialogResponse { path };
                    webview.send_ipc_message(
                        serde_json::to_string(&response).expect("Failed to serialize response"),
                    );
                }
                IpcMessage::ExportFile { path, text } => {
                    let response = match std::fs::write(&path, text.as_bytes()) {
                        Ok(()) => IpcMessage::ExportFileResponse {
                            ok: true,
                            error: None,
                        },
                        Err(e) => IpcMessage::ExportFileResponse {
                            ok: false,
                            error: Some(e.to_string()),
                        },
                    };
                    webview.send_ipc_message(
                        serde_json::to_string(&response).expect("Failed to serialize response"),
                    );
                }
                IpcMessage::ConfirmSaveChanges { filename } => {
                    let choice = match MessageDialog::new()
                        .parent(&window)
                        .title(format!(
                            "Do you want to save the changes to \"{filename}\"?"
                        ))
                        .description("Your changes will be lost if you don't save them.")
                        .level(MessageLevel::Warning)
                        .buttons(MessageButtons::YesNoCancelCustom(
                            "Save".to_string(),
                            "Don't Save".to_string(),
                            "Cancel".to_string(),
                        ))
                        .show()
                    {
                        MessageDialogResult::Custom(choice) if choice == "Save" => "save",
                        MessageDialogResult::Custom(choice) if choice == "Don't Save" => "discard",
                        _ => "cancel",
                    }
                    .to_string();
                    webview.send_ipc_message(
                        serde_json::to_string(&IpcMessage::ConfirmSaveChangesResponse { choice })
                            .expect("Failed to serialize response"),
                    );
                }
                IpcMessage::CloseWindow => window.close(),
                #[cfg(target_os = "macos")]
                IpcMessage::MacosDocumentEdited { edited } => {
                    window.macos_set_document_edited(edited);
                }
                _ => {}
            }
        }
    });
}
