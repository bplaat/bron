/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(missing_docs, reason = "Raw UIKit declarations")]

use std::ffi::{c_char, c_void};
use std::fmt::{self, Display, Formatter};
use std::ops::Deref;

use objc2::rc::{Allocated, Retained};
use objc2::runtime::AnyObject as Object;
use objc2::{Encode, Encoding, class, msg_send};

#[link(name = "Foundation", kind = "framework")]
unsafe extern "C" {
    pub static __CFConstantStringClassReference: Object;
    pub static NSKeyValueChangeNewKey: *const Object;
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}
impl CGPoint {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}
unsafe impl Encode for CGPoint {
    const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct CGSize {
    pub width: f64,
    pub height: f64,
}
impl CGSize {
    pub const fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }
}
unsafe impl Encode for CGSize {
    const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct CGRect {
    pub origin: CGPoint,
    pub size: CGSize,
}
impl CGRect {
    pub const fn new(origin: CGPoint, size: CGSize) -> Self {
        Self { origin, size }
    }
}
unsafe impl Encode for CGRect {
    const ENCODING: Encoding = Encoding::Struct("CGRect", &[CGPoint::ENCODING, CGSize::ENCODING]);
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct UIEdgeInsets {
    pub top: f64,
    pub left: f64,
    pub bottom: f64,
    pub right: f64,
}
unsafe impl Encode for UIEdgeInsets {
    const ENCODING: Encoding = Encoding::Struct(
        "UIEdgeInsets",
        &[f64::ENCODING, f64::ENCODING, f64::ENCODING, f64::ENCODING],
    );
}

pub const NS_UTF8_STRING_ENCODING: u64 = 4;

pub const NS_KEY_VALUE_OBSERVING_OPTION_NEW: u64 = 0x1;

pub const UI_VIEW_AUTORESIZING_FLEXIBLE_WIDTH: u64 = 1 << 1;
pub const UI_VIEW_AUTORESIZING_FLEXIBLE_HEIGHT: u64 = 1 << 4;

#[repr(transparent)]
pub struct NSString(Retained<Object>);

unsafe impl Encode for NSString {
    const ENCODING: Encoding = Encoding::Object;
    const IS_OBJECT_OWNERSHIP: bool = true;

    unsafe fn from_object_return(pointer: *mut c_void, owned: bool) -> Self {
        Self(unsafe { <Retained<Object> as Encode>::from_object_return(pointer, owned) })
    }
}

impl NSString {
    pub fn new(str: impl AsRef<str>) -> Self {
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
// __CFConstantStringClassReference (provided by CoreFoundation, transitively linked via Foundation).
// cfinfo 0x07C8 = ASCII, immutable, not inline, not freed, has NUL terminator.
#[repr(C)]
pub struct CFConstString {
    pub isa: *const c_void,
    pub cfinfo: u32,
    #[cfg(target_pointer_width = "64")]
    pub _rc: u32,
    pub data: *const u8,
    pub len: usize,
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
#[doc(hidden)]
#[macro_export]
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
        static CFSTRING: $crate::ffi::CFConstString = unsafe {
            $crate::ffi::CFConstString {
                isa: &$crate::ffi::__CFConstantStringClassReference
                    as *const objc2::runtime::AnyObject
                    as *const ::std::ffi::c_void,
                cfinfo: 0x07C8,
                #[cfg(target_pointer_width = "64")]
                _rc: 0,
                data: DATA.as_ptr(),
                len: BYTES.len(),
            }
        };
        &CFSTRING as *const $crate::ffi::CFConstString as *mut objc2::runtime::AnyObject
    }};
}
pub use ns_string;
