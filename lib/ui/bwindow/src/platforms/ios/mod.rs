/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

pub(crate) use crate::platforms::ios::event_loop::{
    PlatformEventLoop, PlatformEventLoopProxy, PlatformMonitor,
};
pub(crate) use crate::platforms::ios::headers as native_headers;
pub(crate) use crate::platforms::ios::window::PlatformWindow;

mod event_loop;
pub(crate) mod headers;
pub(crate) mod input;
mod window;
