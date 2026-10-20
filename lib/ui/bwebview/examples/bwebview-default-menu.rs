/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Standard native menu commands, including editing the focused webview.

use bwebview::{WebviewBuilder, WebviewEvent};
use bwindow::{Event, EventLoopBuilder, LogicalSize, WindowBuilder, WindowEvent};

fn main() {
    let smoke = std::env::args().any(|arg| arg == "--smoke");
    let event_loop = EventLoopBuilder::new()
        .with_user_event::<(bwindow::WindowId, WebviewEvent)>()
        .app_id("nl", "bplaat", "BwebviewDefaultMenuExample")
        .build();
    if smoke {
        let proxy = event_loop.create_proxy();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(20));
            let _ = proxy.exit();
        });
    }
    let mut window = WindowBuilder::new()
        .title("Default Menu Example")
        .size(LogicalSize::new(640.0, 400.0))
        .build();
    let webview = WebviewBuilder::new(&window)
        .on_event(event_loop.create_proxy(), |window, event| (window, event))
        .load_html(
            r#"<!doctype html>
<html lang="en">
<meta charset="utf-8">
<title>Default Menu Example</title>
<h1>Native editing commands</h1>
<p>Use the Edit menu or keyboard shortcuts in this text field.</p>
<textarea id="editor" rows="8" cols="60" autofocus></textarea>
<script>
const editor = document.getElementById('editor');
editor.addEventListener('input', () => { document.title = 'Default Menu: ' + editor.value; });
</script>
</html>"#,
        )
        .build();
    let mut close_requests = 0;
    event_loop.run(move |event| {
        let _keep_alive = &webview;
        match event {
            Event::UserEvent((_, WebviewEvent::PageTitleChange(title))) => window.set_title(title),
            Event::Window(_, WindowEvent::CloseRequested(request)) => {
                close_requests += 1;
                if smoke && close_requests == 1 {
                    request.prevent_default();
                    window.set_title("Close cancelled");
                }
            }
            Event::LoopExiting if smoke => {
                assert_eq!(
                    close_requests, 2,
                    "native close and quit were not delivered"
                );
            }
            _ => {}
        }
    });
}
