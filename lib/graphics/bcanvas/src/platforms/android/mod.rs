/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

pub(crate) use crate::platforms::android::context::PlatformCanvasContext;
pub(crate) use crate::platforms::android::offscreen::PlatformOffscreenCanvas;
#[cfg(feature = "window")]
pub(crate) use crate::platforms::android::window::PlatformCanvas;

mod context;
mod offscreen;
#[cfg(feature = "window")]
mod window;
