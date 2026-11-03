/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Win32 window, icon, dialog and package declarations.

#![allow(non_snake_case, clippy::upper_case_acronyms)]

use std::ffi::c_void;

pub(crate) type HWND = *mut c_void;
pub(crate) type BOOL = i32;
pub(crate) type WPARAM = usize;
pub(crate) type LPARAM = isize;
pub(crate) type LRESULT = isize;

pub(crate) const WS_CLIPCHILDREN: u32 = 0x0200_0000;
pub(crate) const PBS_MARQUEE: u32 = 0x08;
pub(crate) const PBM_SETMARQUEE: u32 = 0x0400 + 10;
pub(crate) const ICC_PROGRESS_CLASS: u32 = 0x20;
pub(crate) const WS_CHILD: u32 = 0x4000_0000;
pub(crate) const WS_VISIBLE: u32 = 0x1000_0000;
pub(crate) const WS_CLIPSIBLINGS: u32 = 0x0400_0000;
pub(crate) const GWLP_USERDATA: i32 = -21;

pub(crate) const WM_DESTROY: u32 = 0x0002;
pub(crate) const WM_SIZE: u32 = 0x0005;
pub(crate) const WM_ACTIVATE: u32 = 0x0006;
pub(crate) const WM_SETFOCUS: u32 = 0x0007;
pub(crate) const WM_ERASEBKGND: u32 = 0x0014;
pub(crate) const WM_TIMER: u32 = 0x0113;
pub(crate) const WM_USER: u32 = 0x0400;
pub(crate) const WM_APP: u32 = 0x8000;
pub(crate) const GCLP_HBRBACKGROUND: i32 = -10;
pub(crate) const BLACK_BRUSH: i32 = 4;
pub(crate) const DWMWA_SYSTEMBACKDROP_TYPE: u32 = 38;
pub(crate) const DWMSBT_MAINWINDOW: u32 = 2;

pub(crate) const WA_INACTIVE: usize = 0;
pub(crate) const SWP_NOZORDER: u32 = 0x0004;
pub(crate) const SWP_NOACTIVATE: u32 = 0x0010;
pub(crate) const MB_OK: u32 = 0x0000;
pub(crate) const MB_YESNO: u32 = 0x0004;
pub(crate) const MB_ICONERROR: u32 = 0x0010;
pub(crate) const MB_ICONWARNING: u32 = 0x0030;
pub(crate) const IDYES: i32 = 6;
pub(crate) const SW_SHOWNORMAL: i32 = 1;
pub(crate) const ERROR_INSUFFICIENT_BUFFER: i32 = 122;
pub(crate) const IDC_ARROW: *const u16 = 32512 as *const u16;
pub(crate) const IMAGE_ICON: u32 = 1;
pub(crate) const DIB_RGB_COLORS: u32 = 0;

#[repr(C)]
#[derive(Default)]
pub(crate) struct RECT {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) right: i32,
    pub(crate) bottom: i32,
}

pub(crate) type WNDPROC = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

#[repr(C)]
pub(crate) struct WNDCLASSEXW {
    pub(crate) cbSize: u32,
    pub(crate) style: u32,
    pub(crate) lpfnWndProc: WNDPROC,
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

#[repr(C)]
pub(crate) struct ICONINFO {
    pub(crate) fIcon: BOOL,
    pub(crate) xHotspot: u32,
    pub(crate) yHotspot: u32,
    pub(crate) hbmMask: *mut c_void,
    pub(crate) hbmColor: *mut c_void,
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
pub(crate) struct MARGINS {
    pub(crate) cxLeftWidth: i32,
    pub(crate) cxRightWidth: i32,
    pub(crate) cyTopHeight: i32,
    pub(crate) cyBottomHeight: i32,
}

#[repr(C)]
pub(crate) struct INITCOMMONCONTROLSEX {
    pub(crate) dwSize: u32,
    pub(crate) dwICC: u32,
}

#[link(name = "comctl32")]
unsafe extern "system" {
    pub(crate) fn InitCommonControlsEx(controls: *const INITCOMMONCONTROLSEX) -> BOOL;
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
    pub(crate) fn MoveWindow(
        hwnd: HWND,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        repaint: BOOL,
    ) -> BOOL;
    pub(crate) fn SendMessageW(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT;
    pub(crate) fn GetClientRect(hwnd: HWND, rect: *mut RECT) -> BOOL;
    pub(crate) fn GetWindowRect(hwnd: HWND, rect: *mut RECT) -> BOOL;
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
    pub(crate) fn SetTimer(hwnd: HWND, id: usize, elapse: u32, function: *const c_void) -> usize;
    pub(crate) fn KillTimer(hwnd: HWND, id: usize) -> BOOL;
    pub(crate) fn GetDpiForWindow(hwnd: HWND) -> u32;
    pub(crate) fn LoadCursorW(instance: *mut c_void, name: *const u16) -> *mut c_void;
    pub(crate) fn GetWindowTextW(hwnd: HWND, text: *mut u16, max: i32) -> i32;
    pub(crate) fn GetWindowTextLengthW(hwnd: HWND) -> i32;
    pub(crate) fn GetActiveWindow() -> HWND;
    pub(crate) fn PostMessageW(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> BOOL;
    pub(crate) fn SetClassLongPtrW(hwnd: HWND, index: i32, value: isize) -> usize;
    pub(crate) fn MessageBoxW(hwnd: HWND, text: *const u16, caption: *const u16, kind: u32) -> i32;
    pub(crate) fn FillRect(hdc: *mut c_void, rect: *const RECT, brush: *mut c_void) -> i32;
    pub(crate) fn GetDC(hwnd: HWND) -> *mut c_void;
    pub(crate) fn ReleaseDC(hwnd: HWND, hdc: *mut c_void) -> i32;
    pub(crate) fn LoadImageW(
        instance: *mut c_void,
        name: *const u16,
        kind: u32,
        width: i32,
        height: i32,
        flags: u32,
    ) -> *mut c_void;
    pub(crate) fn GetIconInfo(icon: *mut c_void, info: *mut ICONINFO) -> BOOL;
    pub(crate) fn DestroyIcon(icon: *mut c_void) -> BOOL;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    pub(crate) fn CreateSolidBrush(color: u32) -> *mut c_void;
    pub(crate) fn DeleteObject(object: *mut c_void) -> BOOL;
    pub(crate) fn GetStockObject(object: i32) -> *mut c_void;
    pub(crate) fn GetDIBits(
        hdc: *mut c_void,
        bitmap: *mut c_void,
        start: u32,
        lines: u32,
        bits: *mut c_void,
        info: *mut BITMAPINFOHEADER,
        usage: u32,
    ) -> i32;
}

#[link(name = "dwmapi")]
unsafe extern "system" {
    pub(crate) fn DwmExtendFrameIntoClientArea(hwnd: HWND, margins: *const MARGINS) -> i32;
    pub(crate) fn DwmSetWindowAttribute(
        hwnd: HWND,
        attribute: u32,
        value: *const c_void,
        size: u32,
    ) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    pub(crate) fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    pub(crate) fn LoadLibraryW(name: *const u16) -> *mut c_void;
    pub(crate) fn GetProcAddress(module: *mut c_void, name: *const i8) -> *mut c_void;
    pub(crate) fn GetProcessHeap() -> *mut c_void;
    pub(crate) fn HeapFree(heap: *mut c_void, flags: u32, memory: *mut c_void) -> BOOL;
    pub(crate) fn GetCurrentPackageFullName(length: *mut u32, name: *mut u16) -> i32;
}

#[link(name = "shell32")]
unsafe extern "system" {
    pub(crate) fn ShellExecuteW(
        hwnd: HWND,
        operation: *const u16,
        file: *const u16,
        parameters: *const u16,
        directory: *const u16,
        show: i32,
    ) -> *mut c_void;
}

pub(crate) type SUBCLASSPROC =
    unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM, usize, usize) -> LRESULT;

#[link(name = "comctl32")]
unsafe extern "system" {
    pub(crate) fn SetWindowSubclass(
        hwnd: HWND,
        subclass: SUBCLASSPROC,
        id: usize,
        data: usize,
    ) -> BOOL;
    pub(crate) fn RemoveWindowSubclass(hwnd: HWND, subclass: SUBCLASSPROC, id: usize) -> BOOL;
    pub(crate) fn DefSubclassProc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT;
}

/// Encode a string as a null-terminated UTF-16 string.
pub(crate) fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}
