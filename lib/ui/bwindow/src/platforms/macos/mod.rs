/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

pub(crate) use crate::platforms::macos::content::embed_native_view;
#[cfg(feature = "dialog")]
pub(crate) use crate::platforms::macos::dialog::{PlatformFileDialog, PlatformMessageDialog};
pub(crate) use crate::platforms::macos::event_loop::{
    PlatformEventLoop, PlatformEventLoopProxy, PlatformMonitor,
};
pub(crate) use crate::platforms::macos::frame::FrameSource;
pub(crate) use crate::platforms::macos::window::{PlatformWindow, release_pointer_lock};

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
mod window;
