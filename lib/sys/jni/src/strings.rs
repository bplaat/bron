/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! JNI string types.

use std::ffi::{CStr, CString, c_char};
use std::fmt::{self, Debug, Formatter};
use std::ops::Deref;

/// A borrowed, null terminated modified UTF-8 string, like a class, method, or signature name.
///
/// Create literals with the [`crate::jni_str!`] macro.
#[repr(transparent)]
#[derive(PartialEq, Eq)]
pub struct JNIStr {
    internal: CStr,
}

impl JNIStr {
    /// Wrap a C string that is valid modified UTF-8.
    ///
    /// # Safety
    /// `cstr` must be valid modified UTF-8, which has no supplementary characters.
    pub const unsafe fn from_cstr_unchecked(cstr: &CStr) -> &Self {
        unsafe { &*(cstr as *const CStr as *const Self) }
    }

    /// Wrap a C string, or `None` when it is not plain UTF-8 without supplementary characters,
    /// which is a subset of modified UTF-8.
    pub const fn from_cstr(cstr: &CStr) -> Option<&Self> {
        let bytes = cstr.to_bytes();
        let mut index = 0;
        while index < bytes.len() {
            // Four byte UTF-8 sequences encode supplementary characters differently.
            if bytes[index] >= 0xf0 {
                return None;
            }
            index += 1;
        }
        match std::str::from_utf8(bytes) {
            Ok(_) => Some(unsafe { Self::from_cstr_unchecked(cstr) }),
            Err(_) => None,
        }
    }

    /// The raw null terminated string pointer.
    pub const fn as_ptr(&self) -> *const c_char {
        self.internal.as_ptr()
    }

    /// The string bytes without the null terminator.
    #[allow(clippy::missing_const_for_fn)] // Upstream API is not const
    pub fn to_bytes(&self) -> &[u8] {
        self.internal.to_bytes()
    }
}

impl AsRef<JNIStr> for JNIStr {
    fn as_ref(&self) -> &JNIStr {
        self
    }
}

impl Debug for JNIStr {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.internal.fmt(f)
    }
}

/// An owned, null terminated modified UTF-8 string.
#[derive(Debug, Clone)]
pub struct JNIString {
    internal: CString,
}

impl JNIString {
    /// Encode UTF-8 text as modified UTF-8, which encodes null characters as two bytes and
    /// supplementary characters as two three byte surrogates.
    pub fn new(from: impl AsRef<str>) -> Self {
        let mut bytes = Vec::with_capacity(from.as_ref().len() + 1);
        for char in from.as_ref().chars() {
            match char {
                '\0' => bytes.extend_from_slice(&[0xc0, 0x80]),
                '\u{10000}'.. => {
                    for unit in char.encode_utf16(&mut [0; 2]) {
                        bytes.extend_from_slice(&[
                            0xe0 | (*unit >> 12) as u8,
                            0x80 | ((*unit >> 6) & 0x3f) as u8,
                            0x80 | (*unit & 0x3f) as u8,
                        ]);
                    }
                }
                _ => bytes.extend_from_slice(char.encode_utf8(&mut [0; 4]).as_bytes()),
            }
        }
        Self {
            internal: CString::new(bytes).expect("modified UTF-8 has no null bytes"),
        }
    }
}

impl Deref for JNIString {
    type Target = JNIStr;

    fn deref(&self) -> &JNIStr {
        unsafe { JNIStr::from_cstr_unchecked(&self.internal) }
    }
}

impl AsRef<JNIStr> for JNIString {
    fn as_ref(&self) -> &JNIStr {
        self
    }
}
