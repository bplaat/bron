/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#[cfg(target_vendor = "winxp")]
pub(crate) use self::loader::runtime_available;
pub(crate) use self::webview::PlatformWebview;

mod accelerators;
mod callback;
mod headers;
mod loader;
mod webview;
