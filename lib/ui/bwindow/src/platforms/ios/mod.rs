/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

pub(crate) use crate::platforms::ios::content::{
    add_layer_view, native_layers, remove_layer_view, set_layer_view_frame, set_layer_view_hidden,
};
pub(crate) use crate::platforms::ios::event_loop::{
    PlatformEventLoop, PlatformEventLoopProxy, PlatformMonitor, open_url,
};
pub(crate) use crate::platforms::ios::frame::FrameSource;
pub(crate) use crate::platforms::ios::window::PlatformWindow;

mod content;
mod event_loop;
mod frame;
pub(crate) mod headers;
mod input;
mod scroll;
mod window;
