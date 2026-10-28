/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#[cfg(feature = "dialog")]
pub(crate) use crate::platforms::windows::dialog::{PlatformFileDialog, PlatformMessageDialog};
pub(crate) use crate::platforms::windows::event_loop::{
    PlatformEventLoop, PlatformEventLoopProxy, PlatformMonitor,
};
pub(crate) use crate::platforms::windows::frame::FrameSource;
pub(crate) use crate::platforms::windows::interaction::release_pointer_lock;
pub(crate) use crate::platforms::windows::window::{
    PlatformWindow, config_dir as windows_config_dir, embed_native_view, resize_native_view,
};

#[cfg(feature = "dialog")]
mod dialog;
mod event_loop;
#[cfg(feature = "file_drop")]
mod file_drop;
mod frame;
pub(crate) mod headers;
pub(crate) mod input;
mod instance;
mod interaction;
#[cfg(feature = "menu")]
mod menu;
#[cfg(feature = "progress_bar")]
mod progress_bar;
mod window;
#[cfg(feature = "remember_window_state")]
mod window_state;
