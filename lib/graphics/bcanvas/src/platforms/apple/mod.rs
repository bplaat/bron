/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

pub(crate) use crate::platforms::apple::context::PlatformCanvasContext;
#[cfg(all(feature = "window", target_os = "ios"))]
pub(crate) use crate::platforms::apple::ios::PlatformCanvas;
#[cfg(all(feature = "window", target_os = "macos"))]
pub(crate) use crate::platforms::apple::macos::PlatformCanvas;
pub(crate) use crate::platforms::apple::offscreen::PlatformOffscreenCanvas;

mod context;
mod headers;
#[cfg(all(feature = "window", target_os = "ios"))]
mod ios;
#[cfg(all(feature = "window", target_os = "macos"))]
mod macos;
mod offscreen;
