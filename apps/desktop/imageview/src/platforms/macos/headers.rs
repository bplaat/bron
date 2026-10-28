/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Native framework declarations and constants.

#![allow(non_snake_case, non_upper_case_globals)]

use objc2::runtime::AnyObject as Object;

pub(crate) const NS_EVENT_MODIFIER_FLAG_COMMAND: u64 = 1 << 20;
pub(crate) const NS_NO_BORDER: u64 = 0;
pub(crate) const NS_PRINTING_PAGINATION_MODE_FIT: u64 = 1;
pub(crate) const NS_SCROLLER_STYLE_OVERLAY: i64 = 1;
pub(crate) const NS_WINDOW_COLLECTION_BEHAVIOR_FULL_SCREEN_PRIMARY: u64 = 1 << 7;

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {
    pub(crate) static NSAppearanceNameAqua: *mut Object;
    pub(crate) static NSAppearanceNameDarkAqua: *mut Object;
}
