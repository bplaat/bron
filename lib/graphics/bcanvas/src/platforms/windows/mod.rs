/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

pub(crate) use crate::platforms::windows::context::PlatformCanvasContext;
pub(crate) use crate::platforms::windows::offscreen::PlatformOffscreenCanvas;
#[cfg(feature = "window")]
pub(crate) use crate::platforms::windows::window::PlatformCanvas;

mod com;
mod context;
mod headers;
mod offscreen;
#[cfg(feature = "window")]
mod window;
