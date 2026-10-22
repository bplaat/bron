/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#[cfg(target_os = "ios")]
pub(crate) use self::ios::PlatformWebview;
#[cfg(target_os = "macos")]
pub(crate) use self::webview::PlatformWebview;
#[cfg(target_os = "ios")]
mod ios;

#[cfg(all(target_os = "macos", feature = "file_drop"))]
mod file_drop;
mod headers;
mod webview;
