/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// Minimal Win32 ABI shared by the Direct2D and GDI+ backends: window, device context and
// bitmap functions of user32 and gdi32, COM basics and wide strings.
#![allow(non_camel_case_types, non_snake_case)]
#![allow(clippy::upper_case_acronyms)]
#![allow(unused)]

use std::ffi::c_void;

pub(crate) type BOOL = i32;
pub(crate) type HANDLE = *mut c_void;
pub(crate) type HDC = HANDLE;
pub(crate) type HWND = HANDLE;
pub(crate) type HRESULT = i32;

#[repr(C)]
#[derive(Default, Clone)]
pub(crate) struct RECT {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) right: i32,
    pub(crate) bottom: i32,
}

pub(crate) const TRUE: BOOL = 1;
pub(crate) const FALSE: BOOL = 0;
pub(crate) const E_NOTIMPL: HRESULT = 0x80004001u32 as i32;

#[link(name = "kernel32")]
unsafe extern "system" {
    pub(crate) fn GetModuleHandleA(name: *const i8) -> HANDLE;
    pub(crate) fn GetSystemDirectoryW(buffer: *mut u16, size: u32) -> u32;
    pub(crate) fn LoadLibraryW(name: *const u16) -> HANDLE;
    pub(crate) fn GetProcAddress(module: HANDLE, name: *const i8) -> *const c_void;
}

/// Loads a DLL from the system directory by full path, so a missing DLL is never searched for in
/// the current directory or PATH. The DLL stays loaded for the process lifetime.
#[cfg(target_vendor = "winxp")]
pub(crate) fn load_system_library(name: &str) -> HANDLE {
    const MAX_PATH: usize = 260;
    let mut path = vec![0u16; MAX_PATH];
    let length = unsafe { GetSystemDirectoryW(path.as_mut_ptr(), path.len() as u32) } as usize;
    if length == 0 || length >= path.len() {
        return std::ptr::null_mut();
    }
    path.truncate(length);
    path.extend(format!("\\{name}\0").encode_utf16());
    unsafe { LoadLibraryW(path.as_ptr()) }
}

#[link(name = "user32")]
unsafe extern "system" {
    pub(crate) fn GetClientRect(hwnd: HWND, rect: *mut RECT) -> BOOL;
    pub(crate) fn InvalidateRect(hwnd: HWND, rect: *const RECT, erase: BOOL) -> BOOL;
    pub(crate) fn ValidateRect(hwnd: HWND, rect: *const RECT) -> BOOL;
    pub(crate) fn GetDC(hwnd: HWND) -> HDC;
    pub(crate) fn ReleaseDC(hwnd: HWND, dc: HDC) -> i32;
}

#[repr(C)]
#[derive(Default)]
pub(crate) struct BITMAPINFOHEADER {
    pub(crate) size: u32,
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) planes: u16,
    pub(crate) bit_count: u16,
    pub(crate) compression: u32,
    pub(crate) size_image: u32,
    pub(crate) x_pixels_per_meter: i32,
    pub(crate) y_pixels_per_meter: i32,
    pub(crate) colors_used: u32,
    pub(crate) colors_important: u32,
}

#[repr(C)]
#[derive(Default)]
pub(crate) struct BITMAPINFO {
    pub(crate) header: BITMAPINFOHEADER,
    pub(crate) colors: [u32; 3],
}

pub(crate) const DIB_RGB_COLORS: u32 = 0;
pub(crate) const SRCCOPY: u32 = 0x00cc0020;
pub(crate) const LOGPIXELSX: i32 = 88;

#[link(name = "gdi32")]
unsafe extern "system" {
    pub(crate) fn GetDeviceCaps(dc: HDC, index: i32) -> i32;
    pub(crate) fn CreateCompatibleDC(dc: HDC) -> HDC;
    pub(crate) fn CreateDIBSection(
        dc: HDC,
        info: *const BITMAPINFO,
        usage: u32,
        bits: *mut *mut c_void,
        section: HANDLE,
        offset: u32,
    ) -> HANDLE;
    pub(crate) fn SelectObject(dc: HDC, object: HANDLE) -> HANDLE;
    pub(crate) fn DeleteDC(dc: HDC) -> BOOL;
    pub(crate) fn DeleteObject(object: HANDLE) -> BOOL;
    pub(crate) fn GdiFlush() -> BOOL;
    pub(crate) fn BitBlt(
        dest: HDC,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        source: HDC,
        source_x: i32,
        source_y: i32,
        rop: u32,
    ) -> BOOL;
}

#[repr(C)]
#[derive(PartialEq, Eq)]
pub(crate) struct GUID {
    pub(crate) data1: u32,
    pub(crate) data2: u16,
    pub(crate) data3: u16,
    pub(crate) data4: [u8; 8],
}

#[repr(C)]
pub(crate) struct IUnknownVtbl {
    pub(crate) QueryInterface:
        unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    pub(crate) AddRef: unsafe extern "system" fn(*mut c_void) -> u32,
    pub(crate) Release: unsafe extern "system" fn(*mut c_void) -> u32,
}

/// Encode a string as a null-terminated UTF-16 string.
pub(crate) fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

/// Encode a string literal as a null-terminated UTF-16 string at compile time.
macro_rules! w {
    ($text:literal) => {{
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
pub(crate) use w;
