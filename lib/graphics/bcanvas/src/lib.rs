/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]
#![allow(unsafe_code)]
#![allow(clippy::undocumented_unsafe_blocks)]

pub use crate::canvas::*;
#[cfg(feature = "window")]
pub use crate::window::*;
// Cairo builds gradients and paths cheaply, the other backends cache theirs.
#[cfg(any(target_vendor = "apple", target_os = "android", windows))]
mod cache;
mod canvas;
#[cfg(all(feature = "window", any(windows, target_os = "android")))]
mod composite;
mod path;
mod platforms;
mod recording;
mod transform;
#[cfg(feature = "window")]
mod window;
