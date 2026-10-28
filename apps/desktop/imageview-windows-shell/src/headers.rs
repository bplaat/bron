/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! COM, shell, GDI and window declarations.

#![allow(non_snake_case, clippy::upper_case_acronyms)]

use std::ffi::c_void;

pub(crate) type HRESULT = i32;
pub(crate) type HWND = *mut c_void;
pub(crate) type BOOL = i32;
pub(crate) type WPARAM = usize;
pub(crate) type LPARAM = isize;
pub(crate) type LRESULT = isize;

pub(crate) const S_OK: HRESULT = 0;
pub(crate) const S_FALSE: HRESULT = 1;
pub(crate) const E_NOINTERFACE: HRESULT = 0x8000_4002_u32 as i32;
pub(crate) const E_POINTER: HRESULT = 0x8000_4003_u32 as i32;
pub(crate) const E_FAIL: HRESULT = 0x8000_4005_u32 as i32;
pub(crate) const CLASS_E_NOAGGREGATION: HRESULT = 0x8004_0110_u32 as i32;
pub(crate) const CLASS_E_CLASSNOTAVAILABLE: HRESULT = 0x8004_0111_u32 as i32;
pub(crate) const WTSAT_ARGB: u32 = 2;

pub(crate) const WS_CHILD: u32 = 0x4000_0000;
pub(crate) const WS_VISIBLE: u32 = 0x1000_0000;
pub(crate) const WS_CLIPSIBLINGS: u32 = 0x0400_0000;
pub(crate) const GWLP_USERDATA: i32 = -21;
pub(crate) const WM_PAINT: u32 = 0x000f;
pub(crate) const WM_ERASEBKGND: u32 = 0x0014;
pub(crate) const WM_TIMER: u32 = 0x0113;
pub(crate) const WM_NCDESTROY: u32 = 0x0082;
pub(crate) const SWP_NOZORDER: u32 = 0x0004;
pub(crate) const SWP_NOACTIVATE: u32 = 0x0010;
pub(crate) const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2: isize = -4;
pub(crate) const HKEY_CURRENT_USER: isize = 0x8000_0001_u32 as i32 as isize;
pub(crate) const RRF_RT_REG_DWORD: u32 = 0x0000_0010;

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct GUID {
    pub(crate) data1: u32,
    pub(crate) data2: u16,
    pub(crate) data3: u16,
    pub(crate) data4: [u8; 8],
}

impl GUID {
    pub(crate) const fn new(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> Self {
        Self {
            data1,
            data2,
            data3,
            data4,
        }
    }
}

pub(crate) const IID_IUNKNOWN: GUID = GUID::new(0, 0, 0, [0xc0, 0, 0, 0, 0, 0, 0, 0x46]);
pub(crate) const IID_ICLASS_FACTORY: GUID = GUID::new(1, 0, 0, [0xc0, 0, 0, 0, 0, 0, 0, 0x46]);
pub(crate) const IID_IINITIALIZE_WITH_STREAM: GUID = GUID::new(
    0xb824_b49d,
    0x22ac,
    0x4161,
    [0xac, 0x8a, 0x99, 0x16, 0xe8, 0xfa, 0x3f, 0x7f],
);
pub(crate) const IID_ITHUMBNAIL_PROVIDER: GUID = GUID::new(
    0xe357_fccd,
    0xa995,
    0x4576,
    [0xb0, 0x1f, 0x23, 0x46, 0x30, 0x15, 0x4e, 0x96],
);
pub(crate) const IID_IPREVIEW_HANDLER: GUID = GUID::new(
    0x8895_b1c6,
    0xb41f,
    0x4c1c,
    [0xa5, 0x62, 0x0d, 0x56, 0x42, 0x50, 0x83, 0x6f],
);
pub(crate) const IID_IOBJECT_WITH_SITE: GUID = GUID::new(
    0xfc48_01a3,
    0x2ba9,
    0x11cf,
    [0xa2, 0x29, 0x00, 0xaa, 0x00, 0x3d, 0x73, 0x52],
);
pub(crate) const IID_IOLE_WINDOW: GUID = GUID::new(0x114, 0, 0, [0xc0, 0, 0, 0, 0, 0, 0, 0x46]);

/// The vtable of any COM interface, starting with IUnknown.
#[repr(C)]
pub(crate) struct IUnknownVtbl {
    pub(crate) QueryInterface:
        unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    pub(crate) AddRef: unsafe extern "system" fn(*mut c_void) -> u32,
    pub(crate) Release: unsafe extern "system" fn(*mut c_void) -> u32,
}

#[repr(C)]
pub(crate) struct IStreamVtbl {
    pub(crate) base: IUnknownVtbl,
    pub(crate) Read: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, *mut u32) -> HRESULT,
}

#[repr(C)]
#[derive(Default)]
pub(crate) struct RECT {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) right: i32,
    pub(crate) bottom: i32,
}

#[repr(C)]
pub(crate) struct BITMAPINFOHEADER {
    pub(crate) biSize: u32,
    pub(crate) biWidth: i32,
    pub(crate) biHeight: i32,
    pub(crate) biPlanes: u16,
    pub(crate) biBitCount: u16,
    pub(crate) biCompression: u32,
    pub(crate) biSizeImage: u32,
    pub(crate) biXPelsPerMeter: i32,
    pub(crate) biYPelsPerMeter: i32,
    pub(crate) biClrUsed: u32,
    pub(crate) biClrImportant: u32,
}

#[repr(C)]
pub(crate) struct WNDCLASSEXW {
    pub(crate) cbSize: u32,
    pub(crate) style: u32,
    pub(crate) lpfnWndProc: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
    pub(crate) cbClsExtra: i32,
    pub(crate) cbWndExtra: i32,
    pub(crate) hInstance: *mut c_void,
    pub(crate) hIcon: *mut c_void,
    pub(crate) hCursor: *mut c_void,
    pub(crate) hbrBackground: *mut c_void,
    pub(crate) lpszMenuName: *const u16,
    pub(crate) lpszClassName: *const u16,
    pub(crate) hIconSm: *mut c_void,
}

#[link(name = "gdi32")]
unsafe extern "system" {
    pub(crate) fn CreateDIBSection(
        dc: *mut c_void,
        info: *const BITMAPINFOHEADER,
        usage: u32,
        bits: *mut *mut c_void,
        section: *mut c_void,
        offset: u32,
    ) -> *mut c_void;
}

#[link(name = "user32")]
unsafe extern "system" {
    pub(crate) fn RegisterClassExW(class: *const WNDCLASSEXW) -> u16;
    pub(crate) fn CreateWindowExW(
        ex_style: u32,
        class: *const u16,
        name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: HWND,
        menu: *mut c_void,
        instance: *mut c_void,
        param: *mut c_void,
    ) -> HWND;
    pub(crate) fn DestroyWindow(hwnd: HWND) -> BOOL;
    pub(crate) fn DefWindowProcW(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT;
    pub(crate) fn SetWindowLongPtrW(hwnd: HWND, index: i32, value: isize) -> isize;
    pub(crate) fn GetWindowLongPtrW(hwnd: HWND, index: i32) -> isize;
    pub(crate) fn SetWindowPos(
        hwnd: HWND,
        after: HWND,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        flags: u32,
    ) -> BOOL;
    pub(crate) fn InvalidateRect(hwnd: HWND, rect: *const RECT, erase: BOOL) -> BOOL;
    pub(crate) fn SetFocus(hwnd: HWND) -> HWND;
    pub(crate) fn GetFocus() -> HWND;
    pub(crate) fn SetTimer(hwnd: HWND, id: usize, elapse: u32, function: *const c_void) -> usize;
    pub(crate) fn SetThreadDpiAwarenessContext(context: isize) -> isize;
}

pub(crate) const GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT: u32 = 0x2;
pub(crate) const GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS: u32 = 0x4;

#[link(name = "kernel32")]
unsafe extern "system" {
    pub(crate) fn GetModuleHandleExW(
        flags: u32,
        name: *const c_void,
        module: *mut *mut c_void,
    ) -> BOOL;
}

#[link(name = "advapi32")]
unsafe extern "system" {
    pub(crate) fn RegGetValueW(
        key: isize,
        sub_key: *const u16,
        value: *const u16,
        flags: u32,
        kind: *mut u32,
        data: *mut c_void,
        size: *mut u32,
    ) -> i32;
}

/// Encode a string as a null-terminated UTF-16 string.
pub(crate) fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}
