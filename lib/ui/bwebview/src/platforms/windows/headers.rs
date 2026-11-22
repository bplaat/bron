/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(clippy::upper_case_acronyms)]
#![allow(unused)]

use std::ffi::{c_char, c_void};

// MARK: Win32
pub(crate) type BOOL = i32;
pub(crate) const TRUE: BOOL = 1;
pub(crate) type w_char = u16;
pub(crate) type HANDLE = *mut c_void;
pub(crate) type HMODULE = *const c_void;

#[link(name = "kernel32")]
unsafe extern "system" {
    pub(crate) fn LoadLibraryW(lpLibFileName: *const u16) -> HMODULE;
    pub(crate) fn GetProcAddress(hModule: HMODULE, lpProcName: *const c_char) -> *const c_void;
    pub(crate) fn GetLastError() -> u32;
}

pub(crate) type HWND = HANDLE;
pub(crate) type LPARAM = isize;

#[repr(C)]
pub(crate) struct POINT {
    pub(crate) x: i32,
    pub(crate) y: i32,
}

#[repr(C)]
#[derive(Default, Clone)]
pub(crate) struct RECT {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) right: i32,
    pub(crate) bottom: i32,
}

#[link(name = "user32")]
unsafe extern "system" {
    pub(crate) fn EnumChildWindows(
        hwndParent: HWND,
        lpEnumFunc: unsafe extern "system" fn(HWND, LPARAM) -> BOOL,
        lParam: LPARAM,
    ) -> BOOL;
    pub(crate) fn GetClientRect(hWnd: HWND, rect: *mut RECT) -> i32;
}

pub(crate) type HDROP = HANDLE;

#[link(name = "shell32")]
unsafe extern "system" {
    pub(crate) fn DragQueryFileW(
        drop: HDROP,
        file: u32,
        buffer: *mut w_char,
        buffer_size: u32,
    ) -> u32;
}

pub(crate) type HRESULT = i32;

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

pub(crate) const S_OK: HRESULT = 0;
pub(crate) const E_NOINTERFACE: HRESULT = 0x80004002u32 as HRESULT;
pub(crate) const E_POINTER: HRESULT = 0x80004003u32 as HRESULT;
pub(crate) const DRAGDROP_E_INVALIDHWND: HRESULT = 0x80040102u32 as HRESULT;
pub(crate) const CF_HDROP: u16 = 15;
pub(crate) const DVASPECT_CONTENT: u32 = 1;
pub(crate) const TYMED_HGLOBAL: u32 = 1;
pub(crate) const DROPEFFECT_NONE: u32 = 0;
pub(crate) const DROPEFFECT_COPY: u32 = 1;

#[repr(C)]
pub(crate) struct FORMATETC {
    pub(crate) cfFormat: u16,
    pub(crate) ptd: *mut c_void,
    pub(crate) dwAspect: u32,
    pub(crate) lindex: i32,
    pub(crate) tymed: u32,
}

#[repr(C)]
pub(crate) struct STGMEDIUM {
    pub(crate) tymed: u32,
    pub(crate) data: *mut c_void,
    pub(crate) pUnkForRelease: *mut c_void,
}

#[repr(C)]
pub(crate) struct IDataObject {
    pub(crate) vtbl: *const IDataObjectVtbl,
}

#[repr(C)]
pub(crate) struct IDataObjectVtbl {
    pub(crate) base: IUnknownVtbl,
    pub(crate) GetData:
        unsafe extern "system" fn(*mut IDataObject, *mut FORMATETC, *mut STGMEDIUM) -> HRESULT,
}

#[repr(C)]
pub(crate) struct IDropTarget {
    pub(crate) vtbl: *const IDropTargetVtbl,
}

#[repr(C)]
pub(crate) struct IDropTargetVtbl {
    pub(crate) base: IUnknownVtbl,
    pub(crate) DragEnter: unsafe extern "system" fn(
        *mut IDropTarget,
        *mut IDataObject,
        u32,
        POINT,
        *mut u32,
    ) -> HRESULT,
    pub(crate) DragOver:
        unsafe extern "system" fn(*mut IDropTarget, u32, POINT, *mut u32) -> HRESULT,
    pub(crate) DragLeave: unsafe extern "system" fn(*mut IDropTarget) -> HRESULT,
    pub(crate) Drop: unsafe extern "system" fn(
        *mut IDropTarget,
        *mut IDataObject,
        u32,
        POINT,
        *mut u32,
    ) -> HRESULT,
}

#[link(name = "ole32")]
unsafe extern "system" {
    pub(crate) fn RegisterDragDrop(hwnd: HWND, drop_target: *mut IDropTarget) -> HRESULT;
    pub(crate) fn RevokeDragDrop(hwnd: HWND) -> HRESULT;
    pub(crate) fn ReleaseStgMedium(medium: *mut STGMEDIUM);
}

pub(crate) const IID_IUNKNOWN: GUID = GUID {
    data1: 0,
    data2: 0,
    data3: 0,
    data4: [0xc0, 0, 0, 0, 0, 0, 0, 0x46],
};

/// Encode a string as a null-terminated UTF-16 string.
pub(crate) fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}
