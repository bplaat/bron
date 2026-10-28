/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

pub(crate) use crate::platforms::gtk::context::{PlatformCanvasContext, PlatformImage};
pub(crate) use crate::platforms::gtk::offscreen::PlatformOffscreenCanvas;
#[cfg(feature = "window")]
pub(crate) use crate::platforms::gtk::window::PlatformCanvas;

mod context;
mod headers;
mod offscreen;
#[cfg(feature = "window")]
mod window;
