/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#[cfg(target_vendor = "winxp")]
pub(crate) use self::miniblink::PlatformWebview;
#[cfg(not(target_vendor = "winxp"))]
pub(crate) use self::webview2::PlatformWebview;

#[cfg(feature = "file_drop")]
mod file_drop;
mod headers;
#[cfg(target_vendor = "winxp")]
mod miniblink;
#[cfg(not(target_vendor = "winxp"))]
mod webview2;
