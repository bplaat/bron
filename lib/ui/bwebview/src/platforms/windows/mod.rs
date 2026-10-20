/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

pub(crate) use crate::platforms::windows::webview::PlatformWebview;

mod accelerators;
mod callback;
#[cfg(feature = "file_drop")]
mod file_drop;
mod headers;
mod loader;
mod webview;
