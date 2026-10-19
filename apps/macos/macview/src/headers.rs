/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Native framework declarations and constants.

#![allow(non_snake_case, non_upper_case_globals)]

use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, msg_send};

pub(crate) const NS_ALERT_FIRST_BUTTON_RETURN: isize = 1000;
pub(crate) const NS_APPLICATION_ACTIVATION_POLICY_REGULAR: i64 = 0;
pub(crate) const NS_BACKING_STORE_BUFFERED: u64 = 2;
pub(crate) const NS_CHANGE_DONE: u64 = 0;
pub(crate) const NS_CHANGE_CLEARED: u64 = 2;
pub(crate) const NS_CONTROL_STATE_VALUE_OFF: isize = 0;
pub(crate) const NS_CONTROL_STATE_VALUE_ON: isize = 1;
pub(crate) const NS_ESCAPE_CHARACTER: u16 = 0x1b;
pub(crate) const NS_EVENT_MODIFIER_FLAG_SHIFT: u64 = 1 << 17;
pub(crate) const NS_EVENT_MODIFIER_FLAG_CONTROL: u64 = 1 << 18;
pub(crate) const NS_EVENT_MODIFIER_FLAG_OPTION: u64 = 1 << 19;
pub(crate) const NS_EVENT_MODIFIER_FLAG_COMMAND: u64 = 1 << 20;
/// The characters AppKit reports for the arrow keys, which live in the private use area.
pub(crate) const NS_LEFT_ARROW_FUNCTION_KEY: u16 = 0xf702;
pub(crate) const NS_RIGHT_ARROW_FUNCTION_KEY: u16 = 0xf703;
pub(crate) const NS_LAYOUT_ATTRIBUTE_LEADING: i64 = 5;
pub(crate) const NS_NO_BORDER: u64 = 0;
pub(crate) const NS_POPOVER_BEHAVIOR_APPLICATION_DEFINED: i64 = 0;
pub(crate) const NS_PRINTING_PAGINATION_MODE_FIT: u64 = 1;
pub(crate) const NS_PROGRESS_INDICATOR_STYLE_SPINNING: u64 = 1;
pub(crate) const NS_RECT_EDGE_MAX_Y: u64 = 3;
pub(crate) const NS_SCROLLER_STYLE_OVERLAY: i64 = 1;
pub(crate) const NS_TEXT_ALIGNMENT_RIGHT: i64 = 2;
pub(crate) const NS_TOOLBAR_DISPLAY_MODE_ICON_ONLY: u64 = 2;
pub(crate) const NS_TOOLBAR_ITEM_GROUP_SELECTION_MODE_SELECT_ONE: i64 = 0;
pub(crate) const NS_TOOLBAR_ITEM_GROUP_SELECTION_MODE_MOMENTARY: i64 = 2;
pub(crate) const NS_TRACKING_ACTIVE_IN_KEY_WINDOW: u64 = 0x20;
pub(crate) const NS_TRACKING_CURSOR_UPDATE: u64 = 0x04;
pub(crate) const NS_TRACKING_IN_VISIBLE_RECT: u64 = 0x200;
pub(crate) const NS_TRACKING_MOUSE_MOVED: u64 = 0x02;
pub(crate) const NS_USER_INTERFACE_LAYOUT_ORIENTATION_HORIZONTAL: i64 = 0;
pub(crate) const NS_USER_INTERFACE_LAYOUT_ORIENTATION_VERTICAL: i64 = 1;
pub(crate) const NS_WINDOW_TOOLBAR_STYLE_UNIFIED: i64 = 3;
pub(crate) const NS_WINDING_RULE_EVEN_ODD: u64 = 1;
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

/// The insets of a view's content, which match the `NSEdgeInsets` ABI.
#[derive(Clone, Copy)]
#[repr(C)]
pub(crate) struct EdgeInsets {
    pub(crate) top: f64,
    pub(crate) left: f64,
    pub(crate) bottom: f64,
    pub(crate) right: f64,
}

impl EdgeInsets {
    /// Returns the same inset on every side.
    pub(crate) const fn uniform(inset: f64) -> Self {
        Self {
            top: inset,
            left: inset,
            bottom: inset,
            right: inset,
        }
    }
}

// SAFETY: EdgeInsets matches the NSEdgeInsets ABI of four CGFloat fields.
unsafe impl objc2::Encode for EdgeInsets {
    const ENCODING: objc2::Encoding = objc2::Encoding::Struct(
        "NSEdgeInsets",
        &[
            objc2::Encoding::Double,
            objc2::Encoding::Double,
            objc2::Encoding::Double,
            objc2::Encoding::Double,
        ],
    );
}

/// Converts a Rust boolean to an Objective-C one.
pub(crate) const fn objc_bool(value: bool) -> Bool {
    if value { Bool::YES } else { Bool::NO }
}

/// Returns an autoreleased `NSArray` of `objects`.
pub(crate) fn ns_array(objects: &[*mut Object]) -> *mut Object {
    // SAFETY: NSArray retains the live objects the caller passes.
    unsafe {
        msg_send![class!(NSArray),
            arrayWithObjects: objects.as_ptr().cast::<std::ffi::c_void>(),
            count: objects.len()
        ]
    }
}

/// Returns an autoreleased `NSArray` of strings.
pub(crate) fn ns_string_array(strings: &[&str]) -> *mut Object {
    let strings: Vec<*mut Object> = strings
        .iter()
        .map(|string| macview_appkit::ns_string(string))
        .collect();
    ns_array(&strings)
}
