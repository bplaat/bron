/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use bwebview::WebviewBuilder;
use bwindow::{Event, EventLoopBuilder, LogicalSize, Theme, WindowBuilder, WindowEvent};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "web"]
struct WebAssets;

const fn background_color(theme: Theme) -> u32 {
    match theme {
        Theme::Light => 0xfaf8ef,
        Theme::Dark => 0x1b1814,
    }
}

fn main() {
    let event_loop = EventLoopBuilder::new()
        .app_id("nl", "bplaat", "game2048")
        .build();

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

    #[allow(unused)]
    let mut webview = WebviewBuilder::new(&window)
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

    event_loop.run(move |event| {
        if let Event::Window(_, WindowEvent::ThemeChanged(theme)) = event {
            window.set_background_color(background_color(theme));
        }
    });
}
