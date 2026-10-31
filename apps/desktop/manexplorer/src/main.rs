/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]

use std::collections::HashSet;
use std::process::{Command, exit};

use anyhow::Result;
use bwebview::{WebviewBuilder, WebviewEvent};
use bwindow::{Event, EventLoopBuilder, LogicalSize, Theme, WindowBuilder};
use preferences::{Preferences, Value};
use rust_embed::Embed;
use serde::{Deserialize, Serialize};
use small_http::{Request, Response, Status};
use small_router::RouterBuilder;

#[derive(Embed)]
#[folder = "web"]
struct WebAssets;

#[derive(Serialize)]
struct ManPage {
    page: i32,
    names: Vec<String>,
}

fn man_index(_req: &Request, _ctx: &()) -> Result<Response> {
    // List all directories in /usr/share/man
    let mut pages = Vec::new();
    if let Ok(dir_iter) = std::fs::read_dir("/usr/share/man") {
        for dir in dir_iter.flatten() {
            let path = dir.path();
            if path.is_dir()
                && let Some(page_str) = path.file_name().and_then(|n| n.to_str())
            {
                // Try to parse the page number from "man1", "man2", etc.
                if let Some(page_num) = page_str
                    .strip_prefix("man")
                    .and_then(|n| n.parse::<i32>().ok())
                {
                    // List all files in this manX directory
                    let mut names = HashSet::new();
                    if let Ok(file_iter) = std::fs::read_dir(&path) {
                        for file in file_iter.flatten() {
                            if let Some(file_name) = file.file_name().to_str() {
                                // Remove file extension if present (e.g., "ls.1.gz" -> "ls")
                                let name = file_name.split('.').next().unwrap_or(file_name);
                                if !name.is_empty() {
                                    names.insert(name.to_string());
                                }
                            }
                        }
                    }
                    let mut names: Vec<_> = names.into_iter().collect();
                    names.sort_by_cached_key(|name| (name.to_lowercase(), name.clone()));
                    pages.push(ManPage {
                        page: page_num,
                        names,
                    });
                }
            }
        }
    }
    pages.sort_by_key(|entry| entry.page);
    Ok(Response::with_json(&pages))
}

fn man_show(req: &Request, _ctx: &()) -> Result<Response> {
    let page = req.params.get("page").expect("Should be some");
    let name = req.params.get("name").expect("Should be some");
    // Names starting with - would be options and names with / paths to any file
    if name.starts_with('-')
        || name.contains('/')
        || !page.bytes().all(|b| b.is_ascii_alphanumeric())
    {
        return Ok(Response::with_status(Status::BadRequest));
    }
    let output = Command::new("man")
        .arg("-P")
        .arg("col -b")
        .arg("--")
        .arg(page)
        .arg(name)
        .output()?;
    Ok(Response::with_body(String::from_utf8_lossy_owned(
        output.stdout,
    )))
}

const LAST_HASH: &str = "last_hash";

// MARK: IPC messages
#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum IpcMessage {
    Ready,
    HashChange { hash: String },
    RestoreHash { hash: String },
}

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
    if !cfg!(unix) {
        eprintln!("ManExplorer can only be run on Unix systems");
        exit(1);
    }

    let event_loop = EventLoopBuilder::new()
        .with_user_event::<AppEvent>()
        .app_id("nl", "bplaat", "ManExplorer")
        .build();

    let router = RouterBuilder::new()
        .get("/api/man", man_index)
        .get("/api/man/:page/:name", man_show)
        .build();

    #[allow(unused_mut)]
    let mut window_builder = WindowBuilder::new()
        .title("Man Explorer")
        .size(LogicalSize::new(1024.0, 768.0))
        .min_size(LogicalSize::new(800.0, 480.0))
        .background_color(background_color(event_loop.theme()))
        .center()
        .remember_window_state();
    #[cfg(target_os = "macos")]
    {
        window_builder = window_builder.macos_titlebar_style(bwindow::MacosTitlebarStyle::Hidden);
    }
    let mut window = window_builder.build();

    let prefs = Preferences::new("nl.bplaat.ManExplorer").expect("Failed to open preferences");
    #[allow(unused)]
    let mut webview = WebviewBuilder::new(&window)
        .on_event(event_loop.create_proxy(), AppEvent::Webview)
        .load_rust_embed_with_custom_handler::<WebAssets>(move |req| {
            let res = router.handle(req);
            if res.status != Status::NotFound {
                Some(res)
            } else {
                None
            }
        })
        .build();

    #[cfg(target_os = "macos")]
    webview.add_user_script(
        format!(
            "document.documentElement.style.setProperty('--macos-titlebar-height', '{}px');",
            window.macos_titlebar_size().height
        ),
        bwebview::InjectionTime::DocumentStart,
    );

    event_loop.run(move |event| match event {
        Event::Window(_, bwindow::WindowEvent::ThemeChanged(theme)) => {
            window.set_background_color(background_color(theme));
        }
        Event::UserEvent(AppEvent::Webview(_window_id, WebviewEvent::PageTitleChange(title))) => {
            window.set_title(title)
        }
        Event::UserEvent(AppEvent::Webview(_window_id, WebviewEvent::MessageReceive(message))) => {
            match serde_json::from_str(&message) {
                Ok(IpcMessage::Ready) => {
                    if let Ok(Some(Value::String(hash))) = prefs.get(LAST_HASH) {
                        webview.send_ipc_message(
                            serde_json::to_string(&IpcMessage::RestoreHash { hash })
                                .expect("Failed to serialize restore hash message"),
                        );
                    }
                }
                Ok(IpcMessage::HashChange { hash }) => {
                    if let Err(error) = prefs.set(LAST_HASH, hash) {
                        eprintln!("Failed to save last hash: {error}");
                    }
                }
                _ => {}
            }
        }
        #[cfg(target_os = "macos")]
        Event::Window(_, bwindow::WindowEvent::MacosFullscreenChange(is_fullscreen)) => {
            if is_fullscreen {
                webview.evaluate_script("document.body.classList.add('is-fullscreen');");
            } else {
                webview.evaluate_script("document.body.classList.remove('is-fullscreen');");
            }
        }
        _ => {}
    });
}
