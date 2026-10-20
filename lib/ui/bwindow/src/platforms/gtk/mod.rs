/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#[cfg(feature = "dialog")]
pub(crate) use crate::platforms::gtk::dialog::{PlatformFileDialog, PlatformMessageDialog};
pub(crate) use crate::platforms::gtk::event_loop::{
    PlatformEventLoop, PlatformEventLoopProxy, PlatformMonitor,
};
pub(crate) use crate::platforms::gtk::headers as native_headers;
pub(crate) use crate::platforms::gtk::window::PlatformWindow;

#[cfg(feature = "dialog")]
mod dialog;
mod event_loop;
#[cfg(feature = "file_drop")]
pub(crate) mod file_drop;
pub(crate) mod headers;
pub(crate) mod input;
#[cfg(feature = "menu")]
mod menu;
#[cfg(feature = "progress_bar")]
mod progress_bar;
mod window;
#[cfg(feature = "remember_window_state")]
mod window_state;
