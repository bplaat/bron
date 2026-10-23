/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(missing_docs, reason = "Raw Cocoa declarations")]

use std::ffi::{c_char, c_void};
use std::fmt::{self, Display, Formatter};
use std::ops::Deref;

use objc2::rc::{Allocated, Retained};
use objc2::runtime::AnyObject as Object;
use objc2::{Encode, Encoding, class, msg_send};

#[link(name = "Foundation", kind = "framework")]
unsafe extern "C" {
    pub(crate) static __CFConstantStringClassReference: Object;
}

#[link(name = "Cocoa", kind = "framework")]
unsafe extern "C" {
    pub(crate) static NSApp: *mut Object;
    pub(crate) static NSAppearanceNameAqua: *const Object;
    pub(crate) static NSAppearanceNameDarkAqua: *const Object;
    #[cfg(feature = "file_drop")]
    pub(crate) static NSFilenamesPboardType: *mut Object;
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    pub(crate) fn CGAssociateMouseAndMouseCursorPosition(connected: u32) -> i32;
}

#[derive(Clone, Copy)]
#[repr(C)]
pub(crate) struct CGPoint {
    pub(crate) x: f64,
    pub(crate) y: f64,
}
impl CGPoint {
    pub(crate) const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}
unsafe impl Encode for CGPoint {
    const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
}
pub(crate) type NSPoint = CGPoint;

#[derive(Clone, Copy)]
#[repr(C)]
pub(crate) struct CGSize {
    pub(crate) width: f64,
    pub(crate) height: f64,
}
impl CGSize {
    pub(crate) const fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }
}
unsafe impl Encode for CGSize {
    const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
}
pub(crate) type NSSize = CGSize;

#[derive(Clone, Copy)]
#[repr(C)]
pub(crate) struct CGRect {
    pub(crate) origin: CGPoint,
    pub(crate) size: CGSize,
}
impl CGRect {
    pub(crate) const fn new(origin: CGPoint, size: CGSize) -> Self {
        Self { origin, size }
    }
}
unsafe impl Encode for CGRect {
    const ENCODING: Encoding = Encoding::Struct("CGRect", &[CGPoint::ENCODING, CGSize::ENCODING]);
}
pub(crate) type NSRect = CGRect;

pub(crate) const NS_APPLICATION_ACTIVATION_POLICY_REGULAR: i64 = 0;

pub(crate) const NS_UTF8_STRING_ENCODING: u64 = 4;

pub(crate) const NS_VIEW_WIDTH_SIZABLE: u64 = 2;
pub(crate) const NS_VIEW_MAX_Y_MARGIN: u64 = 32;

pub(crate) const NS_WINDOW_CLOSE_BUTTON: u64 = 0;
pub(crate) const NS_WINDOW_MINIATURIZE_BUTTON: u64 = 1;
pub(crate) const NS_WINDOW_ZOOM_BUTTON: u64 = 2;
pub(crate) const NS_EVENT_TYPE_LEFT_MOUSE_DOWN: u64 = 1;
pub(crate) const NS_EVENT_TYPE_LEFT_MOUSE_DRAGGED: u64 = 6;
pub(crate) const NS_WINDOW_STYLE_MASK_TITLED: u64 = 1 << 0;
pub(crate) const NS_WINDOW_STYLE_MASK_CLOSABLE: u64 = 1 << 1;
pub(crate) const NS_WINDOW_STYLE_MASK_MINIATURIZABLE: u64 = 1 << 2;
pub(crate) const NS_WINDOW_STYLE_MASK_RESIZABLE: u64 = 1 << 3;
pub(crate) const NS_WINDOW_STYLE_MASK_FULLSCREEN: u64 = 1 << 14;
pub(crate) const NS_WINDOW_STYLE_MASK_FULL_SIZE_CONTENT_VIEW: u64 = 1 << 15;

pub(crate) const NS_BACKING_STORE_BUFFERED: u64 = 2;

pub(crate) const NS_WINDOW_TITLE_VISIBILITY_HIDDEN: i64 = 1;

pub(crate) const NS_EVENT_MODIFIER_FLAG_SHIFT: u64 = 1 << 17;
pub(crate) const NS_EVENT_MODIFIER_FLAG_CONTROL: u64 = 1 << 18;
pub(crate) const NS_EVENT_MODIFIER_FLAG_OPTION: u64 = 1 << 19;
pub(crate) const NS_EVENT_MODIFIER_FLAG_COMMAND: u64 = 1 << 20;

#[cfg(feature = "dialog")]
pub(crate) const NS_MODAL_RESPONSE_OK: i64 = 1;
#[cfg(feature = "dialog")]
pub(crate) const NS_ALERT_STYLE_WARNING: i64 = 0;
#[cfg(feature = "dialog")]
pub(crate) const NS_ALERT_STYLE_INFORMATIONAL: i64 = 1;
#[cfg(feature = "dialog")]
pub(crate) const NS_ALERT_STYLE_CRITICAL: i64 = 2;
#[cfg(feature = "dialog")]
pub(crate) const NS_ALERT_FIRST_BUTTON_RETURN: i64 = 1000;
#[cfg(feature = "file_drop")]
pub(crate) const NS_DRAG_OPERATION_COPY: u64 = 1;

#[repr(transparent)]
pub(crate) struct NSString(Retained<Object>);

unsafe impl Encode for NSString {
    const ENCODING: Encoding = Encoding::Object;
    const IS_OBJECT_OWNERSHIP: bool = true;

    unsafe fn from_object_return(pointer: *mut c_void, owned: bool) -> Self {
        Self(unsafe { <Retained<Object> as Encode>::from_object_return(pointer, owned) })
    }
}

impl NSString {
    pub(crate) fn new(str: impl AsRef<str>) -> Self {
        let str = str.as_ref();
        unsafe {
            let ns_string: Allocated<Object> = msg_send![class!(NSString), alloc];
            let ns_string: Retained<Object> = msg_send![ns_string, initWithBytes:str.as_ptr().cast::<c_void>(), length:str.len(), encoding:NS_UTF8_STRING_ENCODING];
            Self(ns_string)
        }
    }
}
impl Deref for NSString {
    type Target = Object;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl Display for NSString {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", unsafe {
            let bytes: *const c_char = msg_send![&*self.0, UTF8String];
            let len: usize =
                msg_send![&*self.0, lengthOfBytesUsingEncoding:NS_UTF8_STRING_ENCODING];
            if len == 0 {
                String::new().into()
            } else {
                assert!(
                    !bytes.is_null(),
                    "NSString returned null UTF8String for non-empty data"
                );
                String::from_utf8_lossy(std::slice::from_raw_parts(bytes.cast::<u8>(), len))
            }
        })
    }
}

// CFConstString mirrors the layout of Apple's __CFConstantString (CFRuntimeBase + data + len).
// Statics of this type placed in __DATA,__cfstring are recognised by dyld as NSString literals,
// equivalent to Clang's @"..." syntax. The ISA is fixed up at load time via
// __CFConstantStringClassReference (provided by CoreFoundation, transitively linked via Cocoa).
// cfinfo 0x07C8 = ASCII, immutable, not inline, not freed, has NUL terminator.
#[repr(C)]
pub(crate) struct CFConstString {
    pub(crate) isa: *const c_void,
    pub(crate) cfinfo: u32,
    #[cfg(target_pointer_width = "64")]
    pub(crate) _rc: u32,
    pub(crate) data: *const u8,
    pub(crate) len: usize,
}
unsafe impl Send for CFConstString {}
unsafe impl Sync for CFConstString {}

// Creates a zero-cost NSString literal equivalent to Clang's @"..." syntax.
// The string must be ASCII with no interior NUL bytes; this is checked at compile time.
// Returns *mut Object pointing to a static CFConstString in __DATA,__cfstring.
// NOTE: Do not call inside closures or trait methods - rustc may split the static
// definition into a separate CGU with internal linkage, making it invisible to the
// linker. Hoist the call to an ordinary free function instead (known rustc bug:
// madsmtm/objc2#258).
/// Internal native string literal.
macro_rules! ns_string {
    ($s:expr) => {{
        const INPUT: &str = $s;
        const BYTES: &[u8] = INPUT.as_bytes();
        const _: () = {
            let mut i = 0usize;
            while i < BYTES.len() {
                if !BYTES[i].is_ascii() || BYTES[i] == b'\0' {
                    panic!("ns_string! only supports ASCII strings without NUL bytes");
                }
                i += 1;
            }
        };
        #[unsafe(link_section = "__TEXT,__cstring,cstring_literals")]
        static DATA: [u8; BYTES.len() + 1] = {
            let mut arr = [0u8; BYTES.len() + 1];
            let mut i = 0usize;
            while i < BYTES.len() {
                arr[i] = BYTES[i];
                i += 1;
            }
            arr
        };
        #[unsafe(link_section = "__DATA,__cfstring")]
        static CFSTRING: $crate::platforms::macos::headers::CFConstString = unsafe {
            $crate::platforms::macos::headers::CFConstString {
                isa: &$crate::platforms::macos::headers::__CFConstantStringClassReference
                    as *const objc2::runtime::AnyObject
                    as *const ::std::ffi::c_void,
                cfinfo: 0x07C8,
                #[cfg(target_pointer_width = "64")]
                _rc: 0,
                data: DATA.as_ptr(),
                len: BYTES.len(),
            }
        };
        &CFSTRING as *const $crate::platforms::macos::headers::CFConstString
            as *mut objc2::runtime::AnyObject
    }};
}
pub(crate) use ns_string;

#[cfg(test)]
mod tests {
    use objc2::msg_send;
    use objc2::rc::autoreleasepool;

    use super::NSString;

    #[test]
    fn owned_string_outlives_the_pool_where_it_was_created() {
        let string = autoreleasepool(|_| NSString::new("safe string"));
        autoreleasepool(|_| assert_eq!(string.to_string(), "safe string"));
    }

    #[test]
    fn owned_string_preserves_utf8() {
        autoreleasepool(|_| {
            let string = NSString::new("caf\u{e9}");
            assert_eq!(string.to_string(), "caf\u{e9}");
        });
    }

    #[test]
    fn string_return_values_are_retained() {
        let description: NSString = autoreleasepool(|_| unsafe {
            let string = NSString::new("retained return");
            msg_send![&*string, description]
        });
        autoreleasepool(|_| assert_eq!(description.to_string(), "retained return"));
    }
}
