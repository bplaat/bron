/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Native framework declarations and constants.

use objc2::runtime::AnyObject as Object;

pub(crate) const KEY_HOME: u16 = 115;
pub(crate) const KEY_PAGE_UP: u16 = 116;
pub(crate) const KEY_END: u16 = 119;
pub(crate) const KEY_PAGE_DOWN: u16 = 121;
pub(crate) const KEY_DOWN: u16 = 125;
pub(crate) const KEY_UP: u16 = 126;
pub(crate) const NS_EVENT_MODIFIER_FLAG_COMMAND: u64 = 1 << 20;
pub(crate) const NS_PRINTING_PAGINATION_MODE_FIT: u64 = 1;
pub(crate) const NS_WINDOW_COLLECTION_BEHAVIOR_FULL_SCREEN_PRIMARY: u64 = 1 << 7;

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {
    pub(crate) static NSViewBoundsDidChangeNotification: *mut Object;
}
