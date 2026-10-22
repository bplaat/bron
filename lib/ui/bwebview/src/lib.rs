/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]
#![allow(unsafe_code)]
#![allow(clippy::undocumented_unsafe_blocks)]

pub use crate::event::*;
pub use crate::webview::*;
mod event;
mod platforms;
mod webview;
