/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#[cfg(feature = "dialog")]
pub(crate) use crate::platforms::macos::dialog::{PlatformFileDialog, PlatformMessageDialog};
pub(crate) use crate::platforms::macos::event_loop::{
    PlatformEventLoop, PlatformEventLoopProxy, PlatformMonitor,
};
pub(crate) use crate::platforms::macos::window::PlatformWindow;

#[cfg(feature = "dialog")]
mod dialog;
mod event_loop;
#[cfg(feature = "file_drop")]
pub(crate) mod file_drop;
pub(crate) mod headers;
pub(crate) mod input;
mod menu;
mod window;
