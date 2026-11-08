/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Native framework declarations and constants.

use std::ffi::{CStr, c_char, c_void};

use imageview_core::macos::Rect;
use objc2::msg_send;
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
pub(crate) const NS_GRID_CELL_PLACEMENT_TRAILING: isize = 3;
pub(crate) const NS_GRID_ROW_ALIGNMENT_FIRST_BASELINE: isize = 2;
pub(crate) const NS_LAYOUT_ATTRIBUTE_LEADING: isize = 5;
pub(crate) const NS_PRINTING_PAGINATION_MODE_FIT: u64 = 1;
pub(crate) const NS_PROGRESS_INDICATOR_STYLE_SPINNING: u64 = 1;
// AppKit numbers its alignments like UIKit on Apple silicon.
pub(crate) const NS_TEXT_ALIGNMENT_CENTER: isize = if cfg!(target_arch = "x86_64") { 2 } else { 1 };
pub(crate) const NS_TEXT_ALIGNMENT_RIGHT: isize = if cfg!(target_arch = "x86_64") { 1 } else { 2 };
pub(crate) const NS_USER_INTERFACE_LAYOUT_ORIENTATION_VERTICAL: isize = 1;
// Flexible margins on all sides, which keep a view in the middle of its superview.
pub(crate) const NS_VIEW_CENTERED: u64 = 1 | 4 | 8 | 32;
pub(crate) const NS_WINDOW_COLLECTION_BEHAVIOR_FULL_SCREEN_PRIMARY: u64 = 1 << 7;
pub(crate) const NS_WINDOW_OCCLUSION_STATE_VISIBLE: u64 = 1 << 1;
pub(crate) const NS_WINDOW_STYLE_MASK_FULL_SIZE_CONTENT_VIEW: u64 = 1 << 15;

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {
    pub(crate) static NSSplitViewDidResizeSubviewsNotification: *mut Object;
    pub(crate) static NSToolbarFlexibleSpaceItemIdentifier: *mut Object;
    pub(crate) static NSToolbarToggleSidebarItemIdentifier: *mut Object;
    pub(crate) static NSViewBoundsDidChangeNotification: *mut Object;
    pub(crate) static NSViewFrameDidChangeNotification: *mut Object;
    pub(crate) static NSWindowDidChangeOcclusionStateNotification: *mut Object;
}

#[link(name = "QuartzCore", kind = "framework")]
unsafe extern "C" {
    pub(crate) static kCAFilterLinear: *mut Object;
    pub(crate) static kCAFilterNearest: *mut Object;
    pub(crate) static kCAFilterTrilinear: *mut Object;
}

// Premultiplied alpha first in little endian 32-bit pixels, which is BGRA in memory.
pub(crate) const CG_BITMAP_PREMULTIPLIED_BGRA: u32 = 2 | (2 << 12);
// The same pixels without alpha, which is skipped, so they are opaque.
pub(crate) const CG_BITMAP_OPAQUE_BGRX: u32 = 6 | (2 << 12);

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    pub(crate) static kCGColorSpaceSRGB: *const c_void;
    pub(crate) fn CGColorSpaceCreateWithName(name: *const c_void) -> *mut c_void;
    pub(crate) fn CGColorSpaceRelease(space: *mut c_void);
    pub(crate) fn CGBitmapContextCreate(
        data: *mut c_void,
        width: usize,
        height: usize,
        bits_per_component: usize,
        bytes_per_row: usize,
        space: *mut c_void,
        bitmap_info: u32,
    ) -> *mut c_void;
    pub(crate) fn CGBitmapContextCreateImage(context: *mut c_void) -> *mut c_void;
    pub(crate) fn CGContextRelease(context: *mut c_void);
    pub(crate) fn CGContextClearRect(context: *mut c_void, rect: Rect);
    pub(crate) fn CGContextTranslateCTM(context: *mut c_void, x: f64, y: f64);
    pub(crate) fn CGContextScaleCTM(context: *mut c_void, x: f64, y: f64);
    pub(crate) fn CGDataProviderCreateWithData(
        info: *mut c_void,
        data: *const c_void,
        size: usize,
        release: unsafe extern "C" fn(*mut c_void, *const c_void, usize),
    ) -> *mut c_void;
    pub(crate) fn CGDataProviderRelease(provider: *mut c_void);
    pub(crate) fn CGImageCreate(
        width: usize,
        height: usize,
        bits_per_component: usize,
        bits_per_pixel: usize,
        bytes_per_row: usize,
        space: *mut c_void,
        bitmap_info: u32,
        provider: *mut c_void,
        decode: *const f64,
        should_interpolate: bool,
        intent: i32,
    ) -> *mut c_void;
    pub(crate) fn CGImageRelease(image: *mut c_void);
}

/// The contents of an NSString, empty for nil.
///
/// # Safety
///
/// `string` must be nil or a valid NSString.
pub(crate) unsafe fn string(string: *mut Object) -> String {
    if string.is_null() {
        return String::new();
    }
    // SAFETY: The string is valid, its UTF-8 representation lives until it is copied.
    unsafe {
        let bytes: *const c_char = msg_send![string, UTF8String];
        if bytes.is_null() {
            return String::new();
        }
        CStr::from_ptr(bytes).to_string_lossy().into_owned()
    }
}

/// An Objective-C boolean of a Rust one.
pub(crate) const fn objc_bool(value: bool) -> Bool {
    if value { Bool::YES } else { Bool::NO }
}
