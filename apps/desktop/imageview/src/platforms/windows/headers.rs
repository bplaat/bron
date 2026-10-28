/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Win32 window, scroll bar and theme declarations.

#![allow(non_snake_case, clippy::upper_case_acronyms)]

use std::ffi::c_void;

pub(crate) type HWND = *mut c_void;
pub(crate) type BOOL = i32;
pub(crate) type WPARAM = usize;
pub(crate) type LPARAM = isize;
pub(crate) type LRESULT = isize;

pub(crate) const WS_CHILD: u32 = 0x4000_0000;
pub(crate) const WS_VISIBLE: u32 = 0x1000_0000;
pub(crate) const WS_CLIPSIBLINGS: u32 = 0x0400_0000;
pub(crate) const WS_HSCROLL: u32 = 0x0010_0000;
pub(crate) const WS_VSCROLL: u32 = 0x0020_0000;
pub(crate) const GWLP_USERDATA: i32 = -21;

pub(crate) const WM_NCDESTROY: u32 = 0x0082;
pub(crate) const WM_SIZE: u32 = 0x0005;
pub(crate) const WM_PAINT: u32 = 0x000f;
pub(crate) const WM_ERASEBKGND: u32 = 0x0014;
pub(crate) const WM_SETCURSOR: u32 = 0x0020;
pub(crate) const WM_KEYDOWN: u32 = 0x0100;
pub(crate) const WM_TIMER: u32 = 0x0113;
pub(crate) const WM_HSCROLL: u32 = 0x0114;
pub(crate) const WM_VSCROLL: u32 = 0x0115;
pub(crate) const WM_MOUSEMOVE: u32 = 0x0200;
pub(crate) const WM_LBUTTONDOWN: u32 = 0x0201;
pub(crate) const WM_LBUTTONUP: u32 = 0x0202;
pub(crate) const WM_MOUSEWHEEL: u32 = 0x020a;
pub(crate) const WM_MOUSEHWHEEL: u32 = 0x020e;
pub(crate) const WM_CAPTURECHANGED: u32 = 0x0215;

pub(crate) const SB_HORZ: i32 = 0;
pub(crate) const SB_VERT: i32 = 1;
pub(crate) const SB_LINEUP: u16 = 0;
pub(crate) const SB_LINEDOWN: u16 = 1;
pub(crate) const SB_PAGEUP: u16 = 2;
pub(crate) const SB_PAGEDOWN: u16 = 3;
pub(crate) const SB_THUMBTRACK: u16 = 5;
pub(crate) const SB_TOP: u16 = 6;
pub(crate) const SB_BOTTOM: u16 = 7;
pub(crate) const SIF_RANGE: u32 = 0x1;
pub(crate) const SIF_PAGE: u32 = 0x2;
pub(crate) const SIF_POS: u32 = 0x4;
pub(crate) const SIF_TRACKPOS: u32 = 0x10;

pub(crate) const VK_SHIFT: i32 = 0x10;
pub(crate) const VK_CONTROL: i32 = 0x11;
pub(crate) const VK_PRIOR: usize = 0x21;
pub(crate) const VK_NEXT: usize = 0x22;
pub(crate) const VK_END: usize = 0x23;
pub(crate) const VK_HOME: usize = 0x24;
pub(crate) const VK_UP: usize = 0x26;
pub(crate) const VK_DOWN: usize = 0x28;
pub(crate) const WHEEL_DELTA: f64 = 120.0;
pub(crate) const HTCLIENT: u16 = 1;
pub(crate) const IDC_ARROW: *const u16 = 32512 as *const u16;
pub(crate) const IDC_SIZEALL: *const u16 = 32646 as *const u16;

#[repr(C)]
#[derive(Default)]
pub(crate) struct RECT {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) right: i32,
    pub(crate) bottom: i32,
}

#[repr(C)]
pub(crate) struct POINT {
    pub(crate) x: i32,
    pub(crate) y: i32,
}

#[repr(C)]
pub(crate) struct SCROLLINFO {
    pub(crate) cbSize: u32,
    pub(crate) fMask: u32,
    pub(crate) nMin: i32,
    pub(crate) nMax: i32,
    pub(crate) nPage: u32,
    pub(crate) nPos: i32,
    pub(crate) nTrackPos: i32,
}

impl SCROLLINFO {
    pub(crate) const fn new(mask: u32) -> Self {
        Self {
            cbSize: size_of::<Self>() as u32,
            fMask: mask,
            nMin: 0,
            nMax: 0,
            nPage: 0,
            nPos: 0,
            nTrackPos: 0,
        }
    }
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
    pub(crate) fn IsWindow(hwnd: HWND) -> BOOL;
    pub(crate) fn DefWindowProcW(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT;
    pub(crate) fn SetWindowLongPtrW(hwnd: HWND, index: i32, value: isize) -> isize;
    pub(crate) fn GetWindowLongPtrW(hwnd: HWND, index: i32) -> isize;
    pub(crate) fn GetClientRect(hwnd: HWND, rect: *mut RECT) -> BOOL;
    pub(crate) fn InvalidateRect(hwnd: HWND, rect: *const RECT, erase: BOOL) -> BOOL;
    pub(crate) fn SetScrollInfo(hwnd: HWND, bar: i32, info: *const SCROLLINFO, redraw: BOOL)
    -> i32;
    pub(crate) fn GetScrollInfo(hwnd: HWND, bar: i32, info: *mut SCROLLINFO) -> BOOL;
    pub(crate) fn SetCapture(hwnd: HWND) -> HWND;
    pub(crate) fn ReleaseCapture() -> BOOL;
    pub(crate) fn GetCapture() -> HWND;
    pub(crate) fn SetFocus(hwnd: HWND) -> HWND;
    pub(crate) fn ScreenToClient(hwnd: HWND, point: *mut POINT) -> BOOL;
    pub(crate) fn SetTimer(hwnd: HWND, id: usize, elapse: u32, function: *const c_void) -> usize;
    pub(crate) fn KillTimer(hwnd: HWND, id: usize) -> BOOL;
    pub(crate) fn GetKeyState(key: i32) -> i16;
    pub(crate) fn GetDpiForWindow(hwnd: HWND) -> u32;
    pub(crate) fn LoadCursorW(instance: *mut c_void, name: *const u16) -> *mut c_void;
    pub(crate) fn SetCursor(cursor: *mut c_void) -> *mut c_void;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    pub(crate) fn GetModuleHandleW(name: *const u16) -> *mut c_void;
}

#[link(name = "uxtheme")]
unsafe extern "system" {
    pub(crate) fn SetWindowTheme(hwnd: HWND, app: *const u16, id_list: *const u16) -> i32;
}

/// Encode a string as a null-terminated UTF-16 string.
pub(crate) fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}
