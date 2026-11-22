/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

pub(crate) use crate::platforms::gtk::content::release_pointer_lock;
#[cfg(feature = "dialog")]
pub(crate) use crate::platforms::gtk::dialog::{PlatformFileDialog, PlatformMessageDialog};
pub(crate) use crate::platforms::gtk::event_loop::{
    PlatformEventLoop, PlatformEventLoopProxy, PlatformMonitor, open_url,
};
pub(crate) use crate::platforms::gtk::frame::FrameSource;
pub(crate) use crate::platforms::gtk::layers::{
    add_layer_view, native_layers, remove_layer_view, set_layer_view_frame, set_layer_view_hidden,
};
#[cfg(feature = "menu")]
pub(crate) use crate::platforms::gtk::menu::set_action_enabled;
pub(crate) use crate::platforms::gtk::window::{PlatformWindow, embed_native_view};

mod content;
#[cfg(feature = "dialog")]
mod dialog;
mod event_loop;
#[cfg(feature = "file_drop")]
pub(crate) mod file_drop;
mod frame;
pub(crate) mod headers;
mod input;
mod layers;
mod menu;
mod pointer_lock;
#[cfg(feature = "progress_bar")]
mod progress_bar;
mod scroll;
pub(crate) mod wayland;
mod window;
#[cfg(feature = "remember_window_state")]
mod window_state;
