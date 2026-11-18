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
pub(crate) const FALSE: BOOL = 0;
pub(crate) type w_char = u16;
pub(crate) type HANDLE = *mut c_void;
pub(crate) type HKEY = HANDLE;
pub(crate) const HKEY_CURRENT_USER: HKEY = 0x80000001usize as HKEY;
pub(crate) const HKEY_LOCAL_MACHINE: HKEY = 0x80000002usize as HKEY;
pub(crate) const RRF_RT_REG_SZ: u32 = 0x00000002;
pub(crate) const RRF_SUBKEY_WOW6432KEY: u32 = 0x00020000;
pub(crate) const ERROR_SUCCESS: i32 = 0;
#[repr(C)]
pub(crate) struct FILETIME {
    pub(crate) dwLowDateTime: u32,
    pub(crate) dwHighDateTime: u32,
}

pub(crate) type HMODULE = *const c_void;

#[link(name = "kernel32")]
unsafe extern "system" {
    pub(crate) fn LoadLibraryW(lpLibFileName: *const u16) -> HMODULE;
    pub(crate) fn GetProcAddress(hModule: HMODULE, lpProcName: *const c_char) -> *const c_void;
    pub(crate) fn FreeLibrary(hLibModule: HMODULE) -> BOOL;
    pub(crate) fn GetLastError() -> u32;
}

#[link(name = "advapi32")]
unsafe extern "system" {
    pub(crate) fn RegGetValueW(
        hkey: HKEY,
        lpSubKey: *const w_char,
        lpValue: *const w_char,
        dwFlags: u32,
        pdwType: *mut u32,
        pvData: *mut c_void,
        pcbData: *mut u32,
    ) -> i32;
}

pub(crate) type HWND = HANDLE;
pub(crate) type WPARAM = usize;
pub(crate) type LPARAM = isize;

pub(crate) const IDYES: i32 = 6;
pub(crate) const MB_OK: u32 = 0x00000000;
pub(crate) const MB_YESNO: u32 = 0x00000004;
pub(crate) const MB_ICONERROR: u32 = 0x00000010;
pub(crate) const MB_ICONWARNING: u32 = 0x00000030;

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
    pub(crate) fn MessageBoxW(
        hWnd: HWND,
        lpText: *const w_char,
        lpCaption: *const w_char,
        uType: u32,
    ) -> i32;
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
pub(crate) struct STATSTG {
    pub(crate) pwcsName: *mut w_char,
    pub(crate) type_: u32,
    pub(crate) cbSize: u64,
    pub(crate) mtime: FILETIME,
    pub(crate) ctime: FILETIME,
    pub(crate) atime: FILETIME,
    pub(crate) grfMode: u32,
    pub(crate) grfLocksSupported: u32,
    pub(crate) clsid: GUID,
    pub(crate) grfStateBits: u32,
    pub(crate) reserved: u32,
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
    pub(crate) lpVtbl: *const IDataObjectVtbl,
}

#[repr(C)]
pub(crate) struct IDataObjectVtbl {
    pub(crate) QueryInterface:
        unsafe extern "system" fn(*mut IDataObject, *const GUID, *mut *mut c_void) -> HRESULT,
    pub(crate) AddRef: unsafe extern "system" fn(*mut IDataObject) -> u32,
    pub(crate) Release: unsafe extern "system" fn(*mut IDataObject) -> u32,
    pub(crate) GetData:
        unsafe extern "system" fn(*mut IDataObject, *mut FORMATETC, *mut STGMEDIUM) -> HRESULT,
}

#[repr(C)]
pub(crate) struct IDropTarget {
    pub(crate) lpVtbl: *const IDropTargetVtbl,
}

#[repr(C)]
pub(crate) struct IDropTargetVtbl {
    pub(crate) QueryInterface:
        unsafe extern "system" fn(*mut IDropTarget, *const GUID, *mut *mut c_void) -> HRESULT,
    pub(crate) AddRef: unsafe extern "system" fn(*mut IDropTarget) -> u32,
    pub(crate) Release: unsafe extern "system" fn(*mut IDropTarget) -> u32,
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
    pub(crate) fn CoTaskMemFree(pv: *mut c_void);
}

pub(crate) const STATFLAG_NONAME: u32 = 1;

#[repr(C)]
pub(crate) struct IStream {
    pub(crate) lpVtbl: *const IStream_Vtbl,
}

impl IStream {
    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// The caller must own a reference and must not use it after releasing it.
    pub(crate) unsafe fn Release(&self) -> u32 {
        unsafe { ((*self.lpVtbl).Release)(self as *const _ as *mut _) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native Read contract.
    pub(crate) unsafe fn Read(&self, pv: *mut c_void, cb: u32, pcbRead: *mut u32) -> HRESULT {
        unsafe { ((*self.lpVtbl).Read)(self as *const _ as *mut _, pv, cb, pcbRead) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native Stat contract.
    pub(crate) unsafe fn Stat(&self, pstatstg: *mut STATSTG, grfStatFlag: u32) -> HRESULT {
        unsafe { ((*self.lpVtbl).Stat)(self as *const _ as *mut _, pstatstg, grfStatFlag) }
    }
}

#[repr(C)]
pub(crate) struct IStream_Vtbl {
    pub(crate) QueryInterface: unsafe extern "system" fn(
        This: *mut IStream,
        riid: *const GUID,
        ppvObject: *mut *mut c_void,
    ) -> HRESULT,
    pub(crate) AddRef: unsafe extern "system" fn(This: *mut IStream) -> u32,
    pub(crate) Release: unsafe extern "system" fn(This: *mut IStream) -> u32,
    pub(crate) Read: unsafe extern "system" fn(
        This: *mut IStream,
        pv: *mut c_void,
        cb: u32,
        pcbRead: *mut u32,
    ) -> HRESULT,
    padding1: [usize; 8],
    pub(crate) Stat: unsafe extern "system" fn(
        This: *mut IStream,
        pstatstg: *mut STATSTG,
        grfStatFlag: u32,
    ) -> HRESULT,
}

// shlwapi.dll
#[link(name = "shlwapi")]
unsafe extern "system" {
    pub(crate) fn SHCreateMemStream(pInit: *const u8, cbInit: u32) -> *mut IStream;
}

#[derive(Default)]
pub(crate) struct LPWSTR(*mut w_char);
impl LPWSTR {
    pub(crate) const fn as_ptr(&self) -> *const w_char {
        self.0
    }

    pub(crate) const fn as_mut_ptr(&mut self) -> *mut *mut w_char {
        &mut self.0
    }
}
impl std::fmt::Display for LPWSTR {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0.is_null() {
            return Ok(());
        }
        let mut len = 0;
        unsafe {
            while *self.0.add(len) != 0 {
                len += 1;
            }
        }
        let str = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(self.0, len) });
        write!(f, "{str}")
    }
}
impl Drop for LPWSTR {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CoTaskMemFree(self.0 as *mut c_void) };
        }
    }
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
