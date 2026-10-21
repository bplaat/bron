/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use bwebview::WebviewBuilder;
use bwindow::{EventLoopBuilder, LogicalSize, Theme, WindowBuilder};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "web"]
struct WebAssets;

fn main() {
    let event_loop = EventLoopBuilder::new()
        .app_id("nl", "bplaat", "game2048")
        .build();

    #[allow(unused_mut)]
    let mut window_builder = WindowBuilder::new()
        .title("2048")
        .size(LogicalSize::new(600.0, 850.0))
        .resizable(false)
        .theme(Theme::Light)
        .background_color(0xfaf8ef)
        .center()
        .remember_window_state();
    #[cfg(target_os = "macos")]
    {
        window_builder = window_builder.macos_titlebar_style(bwindow::MacosTitlebarStyle::Hidden);
    }
    let window = window_builder.build();

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

    event_loop.run(move |_event| {});
}
