/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Native framework declarations and constants.

#![allow(non_snake_case, non_upper_case_globals)]

use objc2::runtime::AnyObject as Object;

pub(crate) const NS_APPLICATION_ACTIVATION_POLICY_REGULAR: i64 = 0;
pub(crate) const NS_BACKING_STORE_BUFFERED: u64 = 2;
pub(crate) const NS_EVENT_MODIFIER_FLAG_SHIFT: u64 = 1 << 17;
pub(crate) const NS_EVENT_MODIFIER_FLAG_CONTROL: u64 = 1 << 18;
pub(crate) const NS_EVENT_MODIFIER_FLAG_OPTION: u64 = 1 << 19;
pub(crate) const NS_EVENT_MODIFIER_FLAG_COMMAND: u64 = 1 << 20;
/// The characters AppKit reports for the arrow keys, which live in the private use area.
pub(crate) const NS_LEFT_ARROW_FUNCTION_KEY: u16 = 0xf702;
pub(crate) const NS_RIGHT_ARROW_FUNCTION_KEY: u16 = 0xf703;
pub(crate) const NS_NO_BORDER: u64 = 0;
pub(crate) const NS_PRINTING_PAGINATION_MODE_FIT: u64 = 1;
pub(crate) const NS_SCROLLER_STYLE_OVERLAY: i64 = 1;
pub(crate) const NS_WINDOW_COLLECTION_BEHAVIOR_FULL_SCREEN_PRIMARY: u64 = 1 << 7;
pub(crate) const NS_WINDOW_STYLE_MASK_CLOSABLE: u64 = 1 << 1;
pub(crate) const NS_WINDOW_STYLE_MASK_MINIATURIZABLE: u64 = 1 << 2;
pub(crate) const NS_WINDOW_STYLE_MASK_RESIZABLE: u64 = 1 << 3;
pub(crate) const NS_WINDOW_STYLE_MASK_TITLED: u64 = 1;

#[link(name = "UniformTypeIdentifiers", kind = "framework")]
unsafe extern "C" {}

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {
    pub(crate) static NSAppearanceNameAqua: *mut Object;
    pub(crate) static NSAppearanceNameDarkAqua: *mut Object;
}
