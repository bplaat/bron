/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A custom menu bar example
//!
//! Native macOS menus, GTK desktop global menus and application shortcuts.

use bwebview::WebviewBuilder;
use bwindow::{
    Accelerator, Event, EventLoopBuilder, KeyCode, MenuBarBuilder, MenuBuilder, MenuItem,
    Modifiers, Theme, WindowBuilder, WindowEvent,
};

const fn background_color(theme: Theme) -> u32 {
    match theme {
        Theme::Light => 0xffffff,
        Theme::Dark => 0x222222,
    }
}

fn main() {
    let event_loop = EventLoopBuilder::new()
        .app_id("nl", "bplaat", "BwebviewMenuExample")
        .menu(
            MenuBarBuilder::new()
                .menu(
                    MenuBuilder::new("File")
                        .item(
                            MenuItem::new("New Tab", "file.new_tab")
                                .accelerator(Accelerator::new(Modifiers::PRIMARY, KeyCode::KeyT)),
                        )
                        .item(
                            MenuItem::new("Open...", "file.open")
                                .accelerator(Accelerator::new(Modifiers::PRIMARY, KeyCode::KeyO)),
                        )
                        .item(
                            MenuItem::new("Save", "file.save")
                                .accelerator(Accelerator::new(Modifiers::PRIMARY, KeyCode::KeyS)),
                        )
                        .separator(),
                )
                .menu(
                    MenuBuilder::new("View")
                        .item(
                            MenuItem::new("Reload", "view.reload")
                                .accelerator(Accelerator::new(Modifiers::PRIMARY, KeyCode::KeyR)),
                        )
                        .item(
                            MenuItem::new("Toggle Sidebar", "view.toggle_sidebar").accelerator(
                                Accelerator::new(
                                    Modifiers::PRIMARY | Modifiers::OPTION,
                                    KeyCode::KeyS,
                                ),
                            ),
                        )
                        .separator()
                        .item(MenuItem::new(
                            "Enter Presentation Mode",
                            "view.presentation",
                        )),
                )
                .menu(MenuBuilder::new("Help").item(
                    MenuItem::new("Documentation", "help.docs").accelerator(Accelerator::new(
                        Modifiers::PRIMARY | Modifiers::SHIFT,
                        KeyCode::Slash,
                    )),
                )),
        )
        .build();
    let smoke = std::env::args().any(|arg| arg == "--smoke");
    let mut received = 0;
    if smoke {
        let proxy = event_loop.create_proxy();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(15));
            let _ = proxy.exit();
        });
    }

    let mut window = WindowBuilder::new()
        .title("Menu Example")
        .background_color(background_color(event_loop.theme()))
        .center()
        .build();
    let mut webview = WebviewBuilder::new(&window)
        .load_html(
            r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Menu Example</title>
<style>
:root { color-scheme: light dark; background: #fff; }
@media (prefers-color-scheme: dark) { :root { background: #222; } }
body { font: 16px system-ui, sans-serif; padding: 1rem 2rem; }
#log { margin-top: 1rem; padding: 1rem; border: 1px solid; border-radius: 4px;
       min-height: 6rem; font-family: monospace; white-space: pre-wrap; }
</style>
</head>
<body>
<h1>Menu Example</h1>
<p>Use application shortcuts or select an item from the native menu when available.</p>
<div id="log">No menu item selected yet</div>
<script>
const log = document.getElementById('log');
let actions = [];
window.ipc.addEventListener('message', e => {
    actions.unshift(e.data);
    log.textContent = actions.slice(0, 10).join('\n');
});
</script>
</body>
</html>"#,
        )
        .build();

    event_loop.run(move |event| {
        if let Event::OpenFiles(paths) = &event
            && smoke
        {
            println!("Opened: {paths:?}");
        }
        if let Event::Window(_, WindowEvent::ThemeChanged(theme)) = &event {
            window.set_background_color(background_color(*theme));
        }
        if let Event::Action(action) = event {
            webview.send_ipc_message(format!("Selected: {action}"));
            if smoke {
                let expected = ["file.new_tab", "view.toggle_sidebar", "help.docs"];
                assert_eq!(action, expected[received]);
                received += 1;
                if received == expected.len() {
                    window.close();
                }
            }
        } else if matches!(event, Event::LoopExiting) && smoke {
            assert_eq!(received, 3, "native shortcuts were not delivered");
        }
    });
}
