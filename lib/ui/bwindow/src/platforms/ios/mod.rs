/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

pub(crate) use crate::platforms::ios::event_loop::{
    PlatformEventLoop, PlatformEventLoopProxy, PlatformMonitor,
};
pub(crate) use crate::platforms::ios::frame::FrameSource;
pub(crate) use crate::platforms::ios::window::PlatformWindow;

mod content;
mod event_loop;
mod frame;
pub(crate) mod headers;
mod input;
mod window;
