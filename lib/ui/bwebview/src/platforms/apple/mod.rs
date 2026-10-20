/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

pub(crate) use crate::platforms::apple::webview::PlatformWebview;

#[cfg(all(target_os = "macos", feature = "file_drop"))]
mod file_drop;
mod headers;
mod webview;
