/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#[cfg(feature = "window")]
pub(crate) use platform::PlatformCanvas;
pub(crate) use platform::{PlatformCanvasContext, PlatformOffscreenCanvas};

#[cfg(target_os = "android")]
use crate::platforms::android as platform;
#[cfg(target_vendor = "apple")]
use crate::platforms::apple as platform;
#[cfg(not(any(target_vendor = "apple", target_os = "android", windows)))]
use crate::platforms::gtk as platform;
#[cfg(windows)]
use crate::platforms::windows as platform;

#[cfg(target_os = "android")]
mod android;

#[cfg(target_vendor = "apple")]
mod apple;

#[cfg(windows)]
mod windows;

#[cfg(not(any(target_vendor = "apple", target_os = "android", windows)))]
mod gtk;
