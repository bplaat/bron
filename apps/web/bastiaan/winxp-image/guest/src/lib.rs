/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The v86 guest integration for Windows XP: the host protocol the programs and drivers share,
//! with the bindings of the Windows APIs they use. The drivers build for the
//! `i686-winxp-windows-driver` target, which has no target environment, the programs for
//! `i686-winxp-windows-gnu` with std.

#![no_std]

#[cfg(all(windows, target_env = "gnu"))]
extern crate std;

pub mod protocol;

#[cfg(all(windows, not(target_env = "gnu")))]
pub mod ddk;
#[cfg(target_arch = "x86")]
pub mod port;
#[cfg(all(windows, target_env = "gnu"))]
pub mod win32;

#[cfg(all(windows, target_env = "gnu"))]
/// A NUL terminated UTF-16 string
pub fn wide(text: &(impl AsRef<std::ffi::OsStr> + ?Sized)) -> std::vec::Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    text.as_ref().encode_wide().chain([0]).collect()
}

/// Encode a string literal as a null-terminated UTF-16 string at compile time.
#[macro_export]
macro_rules! w {
    ($text:expr) => {{
        const TEXT: &str = $text;
        const LEN: usize = {
            let bytes = TEXT.as_bytes();
            let mut len = 1;
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] & 0xc0 != 0x80 {
                    len += if bytes[i] >= 0xf0 { 2 } else { 1 };
                }
                i += 1;
            }
            len
        };
        const WIDE: &[u16] = &{
            let bytes = TEXT.as_bytes();
            let mut result = [0; LEN];
            let mut i = 0;
            let mut j = 0;
            while i < bytes.len() {
                let first = bytes[i];
                let (mut code, extra) = match first {
                    0..=0x7f => (first as u32, 0),
                    0xc0..=0xdf => ((first & 0x1f) as u32, 1),
                    0xe0..=0xef => ((first & 0x0f) as u32, 2),
                    _ => ((first & 0x07) as u32, 3),
                };
                let end = i + extra + 1;
                i += 1;
                while i < end {
                    code = (code << 6) | (bytes[i] & 0x3f) as u32;
                    i += 1;
                }
                assert!(code != 0, "wide string literal contains a null character");
                if code > 0xffff {
                    code -= 0x10000;
                    result[j] = 0xd800 | (code >> 10) as u16;
                    j += 1;
                    result[j] = 0xdc00 | (code & 0x3ff) as u16;
                } else {
                    result[j] = code as u16;
                }
                j += 1;
            }
            result
        };
        WIDE
    }};
}
