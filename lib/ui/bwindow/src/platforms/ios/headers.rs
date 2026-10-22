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
unsafe extern "C" {}

#[derive(Clone, Copy)]
#[repr(C)]
pub(crate) struct CGPoint {
    pub(crate) x: f64,
    pub(crate) y: f64,
}
unsafe impl Encode for CGPoint {
    const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
}

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

#[derive(Clone, Copy)]
#[repr(C)]
pub(crate) struct CGRect {
    pub(crate) origin: CGPoint,
    pub(crate) size: CGSize,
}
unsafe impl Encode for CGRect {
    const ENCODING: Encoding = Encoding::Struct("CGRect", &[CGPoint::ENCODING, CGSize::ENCODING]);
}

#[derive(Clone, Copy)]
#[repr(C)]
pub(crate) struct UIEdgeInsets {
    pub(crate) top: f64,
    pub(crate) left: f64,
    pub(crate) bottom: f64,
    pub(crate) right: f64,
}
unsafe impl Encode for UIEdgeInsets {
    const ENCODING: Encoding = Encoding::Struct(
        "UIEdgeInsets",
        &[f64::ENCODING, f64::ENCODING, f64::ENCODING, f64::ENCODING],
    );
}

pub(crate) const NS_UTF8_STRING_ENCODING: u64 = 4;

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
