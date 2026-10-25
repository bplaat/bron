/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

pub(crate) use crate::platforms::gtk::content::release_pointer_lock;
#[cfg(feature = "dialog")]
pub(crate) use crate::platforms::gtk::dialog::{PlatformFileDialog, PlatformMessageDialog};
pub(crate) use crate::platforms::gtk::event_loop::{
    PlatformEventLoop, PlatformEventLoopProxy, PlatformMonitor,
};
pub(crate) use crate::platforms::gtk::frame::FrameSource;
pub(crate) use crate::platforms::gtk::window::PlatformWindow;

mod content;
#[cfg(feature = "dialog")]
mod dialog;
mod event_loop;
#[cfg(feature = "file_drop")]
pub(crate) mod file_drop;
mod frame;
pub(crate) mod headers;
mod input;
mod menu;
#[cfg(feature = "progress_bar")]
mod progress_bar;
mod window;
#[cfg(feature = "remember_window_state")]
mod window_state;
