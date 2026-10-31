/*
 * Copyright (c) 2023-2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use bwebview::{WebviewBuilder, WebviewEvent};
use bwindow::{Event, EventLoopBuilder, LogicalSize, Theme, WindowBuilder};
use log::{info, warn};
use rust_embed::Embed;
use small_http::{Response, Status};
use small_websocket::Message;

use crate::config::Config;
use crate::ipc::{IPC_CONNECTIONS, IpcConnection, ipc_message_handler};

mod config;
mod dmx;
mod ipc;
mod usb;

// MARK: Internal HTTP server
const PORT: u16 = 39027;
pub(crate) static CONFIG: Mutex<Option<Config>> = Mutex::new(None);

#[derive(Embed)]
#[folder = "$OUT_DIR/web"]
struct WebAssets;

fn random_token() -> String {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).expect("Can't get random bytes");
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

// MARK: Main
pub(crate) enum AppEvent {
    Webview(bwindow::WindowId, WebviewEvent),
    UserEvent(String),
}

fn main() {
    // Init logger
    simple_logger::init_with_level(if cfg!(debug_assertions) {
        log::LevelFilter::Trace
    } else {
        log::LevelFilter::Info
    })
    .expect("Failed to init logger");

    // Create event loop
    let event_loop = EventLoopBuilder::new()
        .with_user_event::<AppEvent>()
        .app_id("nl", "bplaat", "BassieLight")
        .build();

    // Load config
    let config = Config::load();
    info!("Config: {config:?}");
    let cloned_config = config.clone();
    *CONFIG.lock().expect("Failed to lock config") = Some(config);

    // Start DMX thread
    thread::Builder::new()
        .name("dmx".to_string())
        .spawn(move || dmx::dmx_thread(cloned_config))
        .expect("Failed to spawn DMX thread");

    // Try to get local IP address, fallback to localhost if it fails
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, PORT))
        .unwrap_or_else(|_| panic!("Can't start local http server"));
    let local_addr = listener
        .local_addr()
        .expect("Can't get local http server port");
    // The LAN server controls the lights, so WebSocket clients need this token from the QR-code URL
    let ipc_token = random_token();
    let ipc_query = format!("token={ipc_token}");
    let url = if let Ok(ip) = local_ip_address::local_ip() {
        format!("http://{}:{}", ip, local_addr.port())
    } else {
        format!("http://127.0.0.1:{}", local_addr.port())
    };

    // Start internal http server thread
    info!("Starting internal HTTP server at {url}");
    thread::Builder::new()
        .name("http-server".to_string())
        .spawn(move || {
            small_http::serve_single_threaded(listener, move |req| {
                let mut path = req.url.path().to_string();
                if path.ends_with('/') {
                    path = format!("{path}index.html");
                }

                if req.url.path() == "/ipc" {
                    if req.url.query() != Some(ipc_query.as_str()) {
                        return Response::with_status(Status::Unauthorized)
                            .body("401 Unauthorized");
                    }
                    return small_websocket::upgrade(req, |mut ws| {
                        if let Err(error) = ws.set_write_timeout(Some(Duration::from_millis(250))) {
                            warn!("Can't configure WebSocket write timeout: {error}");
                            return;
                        }
                        IPC_CONNECTIONS
                            .lock()
                            .expect("Failed to lock IPC connections")
                            .push(IpcConnection::WebSocket(ws.clone()));
                        loop {
                            let message = match ws.recv_non_blocking() {
                                Ok(message) => message,
                                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                                    continue;
                                }
                                Err(err) => {
                                    warn!("WebSocket recv error: {err}");
                                    break;
                                }
                            };
                            match message {
                                Some(Message::Close(_, _)) => break,
                                Some(Message::Text(text))
                                    if !ipc_message_handler(
                                        IpcConnection::WebSocket(ws.clone()),
                                        &text,
                                    ) =>
                                {
                                    break;
                                }
                                None => {
                                    // FIXME: Create async framework don't do micro sleeps
                                    thread::sleep(Duration::from_millis(100));
                                }
                                _ => {}
                            }
                        }
                        IPC_CONNECTIONS
                            .lock()
                            .expect("Failed to lock IPC connections")
                            .retain(|conn| conn != &IpcConnection::WebSocket(ws.clone()));
                    });
                }

                if let Some(file) = WebAssets::get(path.trim_start_matches('/')) {
                    let mime = mime_guess::from_path(&path).first_or_octet_stream();
                    Response::with_header("Content-Type", mime.to_string()).body(file.data)
                } else {
                    let file = WebAssets::get("index.html").expect("index.html not found");
                    Response::with_header("Content-Type", "text/html").body(file.data)
                }
            });
        })
        .expect("Failed to spawn HTTP server thread");

    // Create webview
    #[allow(unused_mut)]
    let mut window_builder = WindowBuilder::new()
        .title("BassieLight")
        .size(LogicalSize::new(1024.0, 768.0))
        .min_size(LogicalSize::new(640.0, 480.0))
        .center()
        .remember_window_state()
        .theme(Theme::Dark)
        .background_color(0x18181b);
    #[cfg(target_os = "macos")]
    {
        window_builder = window_builder.macos_titlebar_style(bwindow::MacosTitlebarStyle::Hidden);
    }
    let mut window = window_builder.build();

    let mut webview = WebviewBuilder::new(&window)
        .on_event(event_loop.create_proxy(), AppEvent::Webview)
        .load_url(format!("{url}/?token={ipc_token}"))
        .build();

    #[cfg(target_os = "macos")]
    webview.add_user_script(
        format!(
            "document.documentElement.style.setProperty('--macos-titlebar-height', '{}px');",
            window.macos_titlebar_size().height
        ),
        bwebview::InjectionTime::DocumentStart,
    );

    let event_loop_proxy = Arc::new(event_loop.create_proxy());
    event_loop.run(move |event| match event {
        // Window events
        Event::UserEvent(AppEvent::Webview(_window_id, WebviewEvent::PageTitleChange(title))) => {
            window.set_title(title)
        }
        #[cfg(target_os = "macos")]
        Event::Window(_, bwindow::WindowEvent::MacosFullscreenChange(is_fullscreen)) => {
            if is_fullscreen {
                webview.evaluate_script("document.body.classList.add('is-fullscreen');");
            } else {
                webview.evaluate_script("document.body.classList.remove('is-fullscreen');");
            }
        }

        // IPC events
        Event::UserEvent(AppEvent::Webview(_window_id, WebviewEvent::PageLoadStart)) => {
            IPC_CONNECTIONS
                .lock()
                .expect("Failed to lock IPC connections")
                .push(IpcConnection::WebviewIpc(event_loop_proxy.clone()));
        }
        Event::UserEvent(AppEvent::Webview(_window_id, WebviewEvent::MessageReceive(message))) => {
            ipc_message_handler(
                IpcConnection::WebviewIpc(event_loop_proxy.clone()),
                &message,
            );
        }
        Event::UserEvent(AppEvent::UserEvent(data)) => webview.send_ipc_message(&data),

        _ => {}
    });
}
