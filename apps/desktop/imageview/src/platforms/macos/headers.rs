/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Native framework declarations and constants.

use objc2::runtime::{AnyObject as Object, Bool};

pub(crate) const KEY_RETURN: u16 = 36;
pub(crate) const KEY_ESCAPE: u16 = 53;
pub(crate) const KEY_ENTER: u16 = 76;
pub(crate) const KEY_HOME: u16 = 115;
pub(crate) const KEY_PAGE_UP: u16 = 116;
pub(crate) const KEY_END: u16 = 119;
pub(crate) const KEY_PAGE_DOWN: u16 = 121;
pub(crate) const KEY_DOWN: u16 = 125;
pub(crate) const KEY_UP: u16 = 126;
pub(crate) const NS_EVENT_MODIFIER_FLAG_SHIFT: u64 = 1 << 17;
pub(crate) const NS_EVENT_MODIFIER_FLAG_COMMAND: u64 = 1 << 20;
pub(crate) const NS_PRINTING_PAGINATION_MODE_FIT: u64 = 1;
pub(crate) const NS_PROGRESS_INDICATOR_STYLE_SPINNING: u64 = 1;
// Flexible margins on all sides, which keep a view in the middle of its superview.
pub(crate) const NS_VIEW_CENTERED: u64 = 1 | 4 | 8 | 32;
pub(crate) const NS_WINDOW_COLLECTION_BEHAVIOR_FULL_SCREEN_PRIMARY: u64 = 1 << 7;

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {
    pub(crate) static NSToolbarFlexibleSpaceItemIdentifier: *mut Object;
    pub(crate) static NSViewBoundsDidChangeNotification: *mut Object;
    pub(crate) static NSViewFrameDidChangeNotification: *mut Object;
}

/// An Objective-C boolean of a Rust one.
pub(crate) const fn objc_bool(value: bool) -> Bool {
    if value { Bool::YES } else { Bool::NO }
}
