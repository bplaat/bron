/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A bwebview custom titlebar example with repositioned macOS window buttons
//!
//! On macOS the native titlebar is hidden and the webview content fills the whole window,
//! so the page draws its own titlebar. Because the native titlebar is gone the page must
//! also handle window dragging: on mouse down it sends an IPC message to Rust, which starts
//! a native window drag or performs the titlebar double click action (zoom or minimize).
//! On other platforms the native titlebar stays and the messages are ignored.

use bwebview::{WebviewBuilder, WebviewEvent};
use bwindow::{Event, EventLoopBuilder, LogicalSize, Theme, WindowBuilder};

const APP_HTML: &str = r#"<body style="margin:0;height:100vh;display:flex;flex-direction:column;font-family:system-ui;color:#fff;background:#05445e;cursor:default;user-select:none;-webkit-user-select:none">
<header id="titlebar" style="height:52px;flex-shrink:0;display:flex;align-items:center;justify-content:center;background:#189ab4;font-weight:600">Custom Titlebar</header>
<main style="flex:1;display:flex;align-items:center;justify-content:center"><h1>Hello Webview!</h1></main>
<script>
// Ask Rust to drag the window, event.detail counts the clicks so 2 is a double click
document.getElementById('titlebar').addEventListener('mousedown', (event) => {
    if (event.button === 0) window.ipc.postMessage(event.detail === 2 ? 'doubleClick' : 'drag');
});
</script>"#;

enum AppEvent {
    Webview(WebviewEvent),
}

fn main() {
    let event_loop = EventLoopBuilder::new()
        .with_user_event::<AppEvent>()
        .build();

    #[allow(unused_mut)]
    let mut window_builder = WindowBuilder::new()
        .title("Webview Titlebar Example")
        .size(LogicalSize::new(1024.0, 768.0))
        .background_color(0x05445e)
        .theme(Theme::Dark)
        .center();
    #[cfg(target_os = "macos")]
    {
        // Hide the native titlebar and move the window buttons 19px from the top left corner.
        // The titlebar area grows to keep the same 19px gap below the buttons, so it is
        // 2 * 19 + button height = 52px high, matching the HTML header so the buttons are
        // vertically centered in it. Window::macos_titlebar_size() reports this height.
        window_builder = window_builder
            .macos_titlebar_style(bwindow::MacosTitlebarStyle::Hidden)
            .macos_traffic_light_position(bwindow::LogicalPoint::new(19.0, 19.0));
    }
    #[allow(unused_mut)]
    let mut window = window_builder.build();

    let mut _webview = WebviewBuilder::new(&window)
        .on_event(event_loop.create_proxy(), |_, event| {
            AppEvent::Webview(event)
        })
        .load_html(APP_HTML)
        .build();

    event_loop.run(move |event| {
        if let Event::UserEvent(AppEvent::Webview(WebviewEvent::MessageReceive(message))) = event {
            // The drag continues from the current native mouse event, so it only starts while
            // the left mouse button is still held down from the mousedown listener
            #[cfg(target_os = "macos")]
            match message.as_str() {
                "drag" => window.macos_start_window_drag(),
                "doubleClick" => window.macos_perform_titlebar_double_click(),
                _ => {}
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (&window, message);
        }
    });
}
