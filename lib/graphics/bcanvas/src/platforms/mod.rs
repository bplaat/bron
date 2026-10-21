/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#[cfg(target_os = "android")]
pub(crate) use crate::platforms::android::{
    PlatformCanvas, PlatformCanvasContext, PlatformOffscreenCanvas,
};
#[cfg(target_vendor = "apple")]
pub(crate) use crate::platforms::apple::{
    PlatformCanvas, PlatformCanvasContext, PlatformOffscreenCanvas,
};
#[cfg(not(any(target_vendor = "apple", target_os = "android", windows)))]
pub(crate) use crate::platforms::gtk::{
    PlatformCanvas, PlatformCanvasContext, PlatformOffscreenCanvas,
};
#[cfg(windows)]
pub(crate) use crate::platforms::windows::{
    PlatformCanvas, PlatformCanvasContext, PlatformOffscreenCanvas,
};

#[cfg(target_os = "android")]
mod android;

#[cfg(target_vendor = "apple")]
mod apple;

#[cfg(windows)]
mod windows;

#[cfg(not(any(target_vendor = "apple", target_os = "android", windows)))]
mod gtk;
