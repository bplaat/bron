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
pub(crate) const SW_SHOWNORMAL: i32 = 1;
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
    pub(crate) fn ShellExecuteW(
        hwnd: HWND,
        lpOperation: *const w_char,
        lpFile: *const w_char,
        lpParameters: *const w_char,
        lpDirectory: *const w_char,
        nShowCmd: i32,
    ) -> isize;
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

pub(crate) const fn wide_ascii<const N: usize>(value: &str) -> [w_char; N] {
    let bytes = value.as_bytes();
    assert!(N == bytes.len() + 1);
    let mut wide = [0; N];
    let mut i = 0;
    while i < bytes.len() {
        assert!(bytes[i].is_ascii());
        wide[i] = bytes[i] as w_char;
        i += 1;
    }
    wide
}

/// Internal native string literal.
macro_rules! wide {
    ($value:literal) => {{
        const WIDE: &[w_char] =
            &$crate::platforms::windows::headers::wide_ascii::<{ $value.len() + 1 }>($value);
        WIDE
    }};
}
pub(crate) use wide;

pub(crate) trait ToWideString {
    fn to_wide_string(&self) -> Vec<u16>;
}
impl ToWideString for &str {
    fn to_wide_string(&self) -> Vec<u16> {
        let mut v: Vec<u16> = self.encode_utf16().collect();
        v.push(0);
        v
    }
}
impl ToWideString for String {
    fn to_wide_string(&self) -> Vec<u16> {
        let mut v: Vec<u16> = self.encode_utf16().collect();
        v.push(0);
        v
    }
}

pub(crate) struct LPWSTR(*mut w_char);
impl Default for LPWSTR {
    fn default() -> Self {
        Self(std::ptr::null_mut())
    }
}
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

#[repr(C)]
pub(crate) struct EventRegistrationToken {
    pub(crate) value: i64,
}

#[repr(C)]
pub(crate) struct COREWEBVIEW2_COLOR {
    pub(crate) A: u8,
    pub(crate) R: u8,
    pub(crate) G: u8,
    pub(crate) B: u8,
}

pub(crate) const COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL: i32 = 0;

#[repr(C)]
pub(crate) struct ICoreWebView2EnvironmentOptions {
    pub(crate) lpVtbl: *const c_void,
}

pub(crate) const IID_ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler: GUID = GUID {
    data1: 0x4e8a3389,
    data2: 0xc9d8,
    data3: 0x4bd2,
    data4: [0xb6, 0xb5, 0x12, 0x4f, 0xee, 0x6c, 0xc1, 0x4d],
};

// ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler
#[repr(C)]
pub(crate) struct ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler {
    pub(crate) lpVtbl: *const ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandlerVtbl,
    pub(crate) user_data: *mut c_void,
}

#[repr(C)]
pub(crate) struct ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandlerVtbl {
    pub(crate) QueryInterface: unsafe extern "system" fn(
        This: *mut c_void,
        riid: *const GUID,
        ppvObject: *mut *mut c_void,
    ) -> HRESULT,
    pub(crate) AddRef: unsafe extern "system" fn(This: *mut c_void) -> u32,
    pub(crate) Release: unsafe extern "system" fn(This: *mut c_void) -> u32,
    pub(crate) Invoke: unsafe extern "system" fn(
        This: *mut ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler,
        hr: HRESULT,
        env: *mut ICoreWebView2Environment,
    ) -> HRESULT,
}

// ICoreWebView2Environment
#[repr(C)]
pub(crate) struct ICoreWebView2Environment {
    pub(crate) lpVtbl: *const ICoreWebView2EnvironmentVtbl,
}

impl ICoreWebView2Environment {
    pub(crate) unsafe fn QueryInterface(
        &self,
        riid: *const GUID,
        ppvObject: *mut *mut c_void,
    ) -> HRESULT {
        unsafe { ((*self.lpVtbl).QueryInterface)(self as *const _ as *mut _, riid, ppvObject) }
    }

    pub(crate) unsafe fn AddRef(&self) -> u32 {
        unsafe { ((*self.lpVtbl).AddRef)(self as *const _ as *mut _) }
    }

    pub(crate) unsafe fn CreateCoreWebView2Controller(
        &self,
        parentWindow: HWND,
        controllerCreatedHandler: *mut ICoreWebView2CreateCoreWebView2ControllerCompletedHandler,
    ) -> HRESULT {
        unsafe {
            ((*self.lpVtbl).CreateCoreWebView2Controller)(
                self as *const _ as *mut _,
                parentWindow,
                controllerCreatedHandler,
            )
        }
    }

    pub(crate) unsafe fn CreateWebResourceResponse(
        &self,
        content: *mut IStream,
        statusCode: i32,
        reasonPhrase: *mut w_char,
        headers: *mut w_char,
        response: *mut *mut ICoreWebView2WebResourceResponse,
    ) -> HRESULT {
        unsafe {
            ((*self.lpVtbl).CreateWebResourceResponse)(
                self as *const _ as *mut _,
                content,
                statusCode,
                reasonPhrase,
                headers,
                response,
            )
        }
    }
}

#[repr(C)]
pub(crate) struct ICoreWebView2EnvironmentVtbl {
    QueryInterface: unsafe extern "system" fn(
        This: *mut c_void,
        riid: *const GUID,
        ppvObject: *mut *mut c_void,
    ) -> HRESULT,
    AddRef: unsafe extern "system" fn(This: *mut c_void) -> u32,
    Release: unsafe extern "system" fn(This: *mut c_void) -> u32,
    CreateCoreWebView2Controller: unsafe extern "system" fn(
        This: *mut ICoreWebView2Environment,
        parentWindow: HWND,
        controllerCreatedHandler: *mut ICoreWebView2CreateCoreWebView2ControllerCompletedHandler,
    ) -> HRESULT,
    CreateWebResourceResponse: unsafe extern "system" fn(
        This: *mut ICoreWebView2Environment,
        content: *mut IStream,
        statusCode: i32,
        reasonPhrase: *mut w_char,
        headers: *mut w_char,
        response: *mut *mut ICoreWebView2WebResourceResponse,
    ) -> HRESULT,
}

// ICoreWebView2CreateCoreWebView2ControllerCompletedHandler
#[repr(C)]
pub(crate) struct ICoreWebView2CreateCoreWebView2ControllerCompletedHandler {
    pub(crate) lpVtbl: *const ICoreWebView2CreateCoreWebView2ControllerCompletedHandlerVtbl,
    pub(crate) user_data: *mut c_void,
}

#[repr(C)]
pub(crate) struct ICoreWebView2CreateCoreWebView2ControllerCompletedHandlerVtbl {
    pub(crate) QueryInterface: unsafe extern "system" fn(
        This: *mut c_void,
        riid: *const GUID,
        ppvObject: *mut *mut c_void,
    ) -> HRESULT,
    pub(crate) AddRef: unsafe extern "system" fn(This: *mut c_void) -> u32,
    pub(crate) Release: unsafe extern "system" fn(This: *mut c_void) -> u32,
    pub(crate) Invoke: unsafe extern "system" fn(
        This: *mut ICoreWebView2CreateCoreWebView2ControllerCompletedHandler,
        hr: HRESULT,
        controller: *mut ICoreWebView2Controller,
    ) -> HRESULT,
}

// ICoreWebView2Controller
#[repr(C)]
pub(crate) struct ICoreWebView2Controller {
    pub(crate) lpVtbl: *const ICoreWebView2ControllerVtbl,
}

impl ICoreWebView2Controller {
    pub(crate) unsafe fn Close(&self) -> HRESULT {
        unsafe { ((*self.lpVtbl).Close)(self as *const _ as *mut _) }
    }

    pub(crate) unsafe fn QueryInterface(
        &self,
        riid: *const GUID,
        ppvObject: *mut *mut c_void,
    ) -> HRESULT {
        unsafe { ((*self.lpVtbl).QueryInterface)(self as *const _ as *mut _, riid, ppvObject) }
    }

    pub(crate) unsafe fn AddRef(&self) -> u32 {
        unsafe { ((*self.lpVtbl).AddRef)(self as *const _ as *mut _) }
    }

    pub(crate) unsafe fn put_IsVisible(&self, isVisible: BOOL) -> HRESULT {
        unsafe { ((*self.lpVtbl).put_IsVisible)(self as *const _ as *mut _, isVisible) }
    }

    pub(crate) unsafe fn put_Bounds(&self, bounds: RECT) -> HRESULT {
        unsafe { ((*self.lpVtbl).put_Bounds)(self as *const _ as *mut _, bounds) }
    }

    pub(crate) unsafe fn get_CoreWebView2(&self, webview: *mut *mut ICoreWebView2) -> HRESULT {
        unsafe { ((*self.lpVtbl).get_CoreWebView2)(self as *const _ as *mut _, webview) }
    }
}

#[repr(C)]
pub(crate) struct ICoreWebView2ControllerVtbl {
    QueryInterface: unsafe extern "system" fn(
        This: *mut c_void,
        riid: *const GUID,
        ppvObject: *mut *mut c_void,
    ) -> HRESULT,
    AddRef: unsafe extern "system" fn(This: *mut c_void) -> u32,
    Release: unsafe extern "system" fn(This: *mut c_void) -> u32,
    padding: [usize; 1],
    put_IsVisible:
        unsafe extern "system" fn(This: *mut ICoreWebView2Controller, isVisible: BOOL) -> HRESULT,
    padding2: [usize; 1],
    put_Bounds:
        unsafe extern "system" fn(This: *mut ICoreWebView2Controller, bounds: RECT) -> HRESULT,
    padding3: [usize; 12],
    pub(super) add_AcceleratorKeyPressed: unsafe extern "system" fn(
        This: *mut ICoreWebView2Controller,
        handler: *mut c_void,
        token: *mut EventRegistrationToken,
    ) -> HRESULT,
    padding4: [usize; 4],
    Close: unsafe extern "system" fn(This: *mut ICoreWebView2Controller) -> HRESULT,
    get_CoreWebView2: unsafe extern "system" fn(
        This: *mut ICoreWebView2Controller,
        webview: *mut *mut ICoreWebView2,
    ) -> HRESULT,
}

// ICoreWebView2Controller2
pub(crate) const IID_ICoreWebView2Controller2: GUID = GUID {
    data1: 0xc979903e,
    data2: 0xd4ca,
    data3: 0x4228,
    data4: [0x92, 0xeb, 0x47, 0xee, 0x3f, 0xa9, 0x6e, 0xab],
};

#[repr(C)]
pub(crate) struct ICoreWebView2Controller2 {
    pub(crate) lpVtbl: *const ICoreWebView2Controller2Vtbl,
}

impl ICoreWebView2Controller2 {
    pub(crate) unsafe fn put_DefaultBackgroundColor(&self, color: COREWEBVIEW2_COLOR) -> HRESULT {
        unsafe { ((*self.lpVtbl).put_DefaultBackgroundColor)(self as *const _ as *mut _, color) }
    }
}

#[repr(C)]
pub(crate) struct ICoreWebView2Controller2Vtbl {
    QueryInterface: unsafe extern "system" fn(
        This: *mut c_void,
        riid: *const GUID,
        ppvObject: *mut *mut c_void,
    ) -> HRESULT,
    AddRef: unsafe extern "system" fn(This: *mut c_void) -> u32,
    Release: unsafe extern "system" fn(This: *mut c_void) -> u32,
    padding: [usize; 24],
    put_DefaultBackgroundColor: unsafe extern "system" fn(
        This: *mut ICoreWebView2Controller2,
        color: COREWEBVIEW2_COLOR,
    ) -> HRESULT,
}

// ICoreWebView2Controller4
pub(crate) const IID_ICoreWebView2Controller4: GUID = GUID {
    data1: 0x97d418d5,
    data2: 0xa426,
    data3: 0x4e49,
    data4: [0xa1, 0x51, 0xe1, 0xa1, 0x0f, 0x32, 0x7d, 0x9e],
};

#[repr(C)]
pub(crate) struct ICoreWebView2Controller4 {
    pub(crate) lpVtbl: *const ICoreWebView2Controller4Vtbl,
}

impl ICoreWebView2Controller4 {
    pub(crate) unsafe fn Release(&self) -> u32 {
        unsafe { ((*self.lpVtbl).Release)(self as *const _ as *mut _) }
    }

    pub(crate) unsafe fn put_AllowExternalDrop(&self, value: BOOL) -> HRESULT {
        unsafe { ((*self.lpVtbl).put_AllowExternalDrop)(self as *const _ as *mut _, value) }
    }
}

#[repr(C)]
pub(crate) struct ICoreWebView2Controller4Vtbl {
    QueryInterface: unsafe extern "system" fn(
        This: *mut c_void,
        riid: *const GUID,
        ppvObject: *mut *mut c_void,
    ) -> HRESULT,
    AddRef: unsafe extern "system" fn(This: *mut c_void) -> u32,
    Release: unsafe extern "system" fn(This: *mut c_void) -> u32,
    padding: [usize; 34],
    put_AllowExternalDrop:
        unsafe extern "system" fn(This: *mut ICoreWebView2Controller4, value: BOOL) -> HRESULT,
}

// ICoreWebView2Environment10
pub(crate) const IID_ICoreWebView2Environment10: GUID = GUID {
    data1: 0xee0eb9df,
    data2: 0x6f12,
    data3: 0x46ce,
    data4: [0xb5, 0x3f, 0x3f, 0x47, 0xb9, 0xc9, 0x28, 0xe0],
};

#[repr(C)]
pub(crate) struct ICoreWebView2Environment10 {
    pub(crate) lpVtbl: *const ICoreWebView2Environment10Vtbl,
}

impl ICoreWebView2Environment10 {
    pub(crate) unsafe fn Release(&self) -> u32 {
        unsafe { ((*self.lpVtbl).Release)(self as *const _ as *mut _) }
    }

    pub(crate) unsafe fn CreateCoreWebView2ControllerOptions(
        &self,
        value: *mut *mut ICoreWebView2ControllerOptions,
    ) -> HRESULT {
        unsafe {
            ((*self.lpVtbl).CreateCoreWebView2ControllerOptions)(self as *const _ as *mut _, value)
        }
    }

    pub(crate) unsafe fn CreateCoreWebView2ControllerWithOptions(
        &self,
        parentWindow: HWND,
        options: *mut ICoreWebView2ControllerOptions,
        handler: *mut ICoreWebView2CreateCoreWebView2ControllerCompletedHandler,
    ) -> HRESULT {
        unsafe {
            ((*self.lpVtbl).CreateCoreWebView2ControllerWithOptions)(
                self as *const _ as *mut _,
                parentWindow,
                options,
                handler,
            )
        }
    }
}

#[repr(C)]
pub(crate) struct ICoreWebView2Environment10Vtbl {
    QueryInterface: unsafe extern "system" fn(
        This: *mut c_void,
        riid: *const GUID,
        ppvObject: *mut *mut c_void,
    ) -> HRESULT,
    AddRef: unsafe extern "system" fn(This: *mut c_void) -> u32,
    Release: unsafe extern "system" fn(This: *mut c_void) -> u32,
    padding: [usize; 17],
    CreateCoreWebView2ControllerOptions: unsafe extern "system" fn(
        This: *mut ICoreWebView2Environment10,
        value: *mut *mut ICoreWebView2ControllerOptions,
    ) -> HRESULT,
    CreateCoreWebView2ControllerWithOptions: unsafe extern "system" fn(
        This: *mut ICoreWebView2Environment10,
        parentWindow: HWND,
        options: *mut ICoreWebView2ControllerOptions,
        handler: *mut ICoreWebView2CreateCoreWebView2ControllerCompletedHandler,
    ) -> HRESULT,
}

// ICoreWebView2ControllerOptions
#[repr(C)]
pub(crate) struct ICoreWebView2ControllerOptions {
    pub(crate) lpVtbl: *const ICoreWebView2ControllerOptionsVtbl,
}

impl ICoreWebView2ControllerOptions {
    pub(crate) unsafe fn QueryInterface(
        &self,
        riid: *const GUID,
        ppvObject: *mut *mut c_void,
    ) -> HRESULT {
        unsafe { ((*self.lpVtbl).QueryInterface)(self as *const _ as *mut _, riid, ppvObject) }
    }

    pub(crate) unsafe fn Release(&self) -> u32 {
        unsafe { ((*self.lpVtbl).Release)(self as *const _ as *mut _) }
    }
}

#[repr(C)]
pub(crate) struct ICoreWebView2ControllerOptionsVtbl {
    QueryInterface: unsafe extern "system" fn(
        This: *mut c_void,
        riid: *const GUID,
        ppvObject: *mut *mut c_void,
    ) -> HRESULT,
    AddRef: unsafe extern "system" fn(This: *mut c_void) -> u32,
    Release: unsafe extern "system" fn(This: *mut c_void) -> u32,
}

// ICoreWebView2ControllerOptions3
pub(crate) const IID_ICoreWebView2ControllerOptions3: GUID = GUID {
    data1: 0xb32b191a,
    data2: 0x8998,
    data3: 0x57ca,
    data4: [0xb7, 0xcb, 0xe0, 0x46, 0x17, 0xe4, 0xce, 0x4a],
};

#[repr(C)]
pub(crate) struct ICoreWebView2ControllerOptions3 {
    pub(crate) lpVtbl: *const ICoreWebView2ControllerOptions3Vtbl,
}

impl ICoreWebView2ControllerOptions3 {
    pub(crate) unsafe fn Release(&self) -> u32 {
        unsafe { ((*self.lpVtbl).Release)(self as *const _ as *mut _) }
    }

    pub(crate) unsafe fn put_DefaultBackgroundColor(&self, value: COREWEBVIEW2_COLOR) -> HRESULT {
        unsafe { ((*self.lpVtbl).put_DefaultBackgroundColor)(self as *const _ as *mut _, value) }
    }
}

#[repr(C)]
pub(crate) struct ICoreWebView2ControllerOptions3Vtbl {
    QueryInterface: unsafe extern "system" fn(
        This: *mut c_void,
        riid: *const GUID,
        ppvObject: *mut *mut c_void,
    ) -> HRESULT,
    AddRef: unsafe extern "system" fn(This: *mut c_void) -> u32,
    Release: unsafe extern "system" fn(This: *mut c_void) -> u32,
    padding: [usize; 7],
    put_DefaultBackgroundColor: unsafe extern "system" fn(
        This: *mut ICoreWebView2ControllerOptions3,
        value: COREWEBVIEW2_COLOR,
    ) -> HRESULT,
}

// ICoreWebView2

#[repr(C)]
pub(crate) struct ICoreWebView2 {
    pub(crate) lpVtbl: *const ICoreWebView2Vtbl,
}

impl ICoreWebView2 {
    pub(crate) unsafe fn get_Settings(&self, settings: *mut *mut ICoreWebView2Settings) -> HRESULT {
        unsafe { ((*self.lpVtbl).get_Settings)(self as *const _ as *mut _, settings) }
    }

    pub(crate) unsafe fn get_Source(&self, uri: *mut *mut w_char) -> HRESULT {
        unsafe { ((*self.lpVtbl).get_Source)(self as *const _ as *mut _, uri) }
    }

    pub(crate) unsafe fn Navigate(&self, uri: *const w_char) -> HRESULT {
        unsafe { ((*self.lpVtbl).Navigate)(self as *const _ as *mut _, uri) }
    }

    pub(crate) unsafe fn NavigateToString(&self, html: *const w_char) -> HRESULT {
        unsafe { ((*self.lpVtbl).NavigateToString)(self as *const _ as *mut _, html) }
    }

    pub(crate) unsafe fn add_NavigationStarting(
        &self,
        eventHandler: *mut ICoreWebView2NavigationStartingEventHandler,
        token: *mut EventRegistrationToken,
    ) -> HRESULT {
        unsafe {
            ((*self.lpVtbl).add_NavigationStarting)(self as *const _ as *mut _, eventHandler, token)
        }
    }

    pub(crate) unsafe fn add_NavigationCompleted(
        &self,
        eventHandler: *mut ICoreWebView2NavigationCompletedEventHandler,
        token: *mut EventRegistrationToken,
    ) -> HRESULT {
        unsafe {
            ((*self.lpVtbl).add_NavigationCompleted)(
                self as *const _ as *mut _,
                eventHandler,
                token,
            )
        }
    }

    pub(crate) unsafe fn AddScriptToExecuteOnDocumentCreated(
        &self,
        script: *const w_char,
        resultHandler: *mut c_void,
    ) -> HRESULT {
        unsafe {
            ((*self.lpVtbl).AddScriptToExecuteOnDocumentCreated)(
                self as *const _ as *mut _,
                script,
                resultHandler,
            )
        }
    }

    pub(crate) unsafe fn ExecuteScript(
        &self,
        script: *const w_char,
        resultHandler: *mut c_void,
    ) -> HRESULT {
        unsafe { ((*self.lpVtbl).ExecuteScript)(self as *const _ as *mut _, script, resultHandler) }
    }

    pub(crate) unsafe fn add_WebMessageReceived(
        &self,
        eventHandler: *mut ICoreWebView2WebMessageReceivedEventHandler,
        token: *mut EventRegistrationToken,
    ) -> HRESULT {
        unsafe {
            ((*self.lpVtbl).add_WebMessageReceived)(self as *const _ as *mut _, eventHandler, token)
        }
    }

    pub(crate) unsafe fn add_NewWindowRequested(
        &self,
        eventHandler: *mut ICoreWebView2NewWindowRequestedEventHandler,
        token: *mut EventRegistrationToken,
    ) -> HRESULT {
        unsafe {
            ((*self.lpVtbl).add_NewWindowRequested)(self as *const _ as *mut _, eventHandler, token)
        }
    }

    pub(crate) unsafe fn add_DocumentTitleChanged(
        &self,
        eventHandler: *mut ICoreWebView2DocumentTitleChangedEventHandler,
        token: *mut EventRegistrationToken,
    ) -> HRESULT {
        unsafe {
            ((*self.lpVtbl).add_DocumentTitleChanged)(
                self as *const _ as *mut _,
                eventHandler,
                token,
            )
        }
    }

    pub(crate) unsafe fn get_DocumentTitle(&self, title: *mut *mut w_char) -> HRESULT {
        unsafe { ((*self.lpVtbl).get_DocumentTitle)(self as *const _ as *mut _, title) }
    }

    pub(crate) unsafe fn add_WebResourceRequested(
        &self,
        eventHandler: *mut ICoreWebView2WebResourceRequestedEventHandler,
        token: *mut EventRegistrationToken,
    ) -> HRESULT {
        unsafe {
            ((*self.lpVtbl).add_WebResourceRequested)(
                self as *const _ as *mut _,
                eventHandler,
                token,
            )
        }
    }

    pub(crate) unsafe fn AddWebResourceRequestedFilter(
        &self,
        uri: *const w_char,
        resourceContext: i32,
    ) -> HRESULT {
        unsafe {
            ((*self.lpVtbl).AddWebResourceRequestedFilter)(
                self as *const _ as *mut _,
                uri,
                resourceContext,
            )
        }
    }
}

#[repr(C)]
pub(crate) struct ICoreWebView2Vtbl {
    QueryInterface: unsafe extern "system" fn(
        This: *mut c_void,
        riid: *const GUID,
        ppvObject: *mut *mut c_void,
    ) -> HRESULT,
    AddRef: unsafe extern "system" fn(This: *mut c_void) -> u32,
    Release: unsafe extern "system" fn(This: *mut c_void) -> u32,
    get_Settings: unsafe extern "system" fn(
        This: *mut ICoreWebView2,
        settings: *mut *mut ICoreWebView2Settings,
    ) -> HRESULT,
    get_Source:
        unsafe extern "system" fn(This: *mut ICoreWebView2, uri: *mut *mut w_char) -> HRESULT,
    Navigate: unsafe extern "system" fn(This: *mut ICoreWebView2, uri: *const w_char) -> HRESULT,
    NavigateToString:
        unsafe extern "system" fn(This: *mut ICoreWebView2, html: *const w_char) -> HRESULT,
    add_NavigationStarting: unsafe extern "system" fn(
        This: *mut ICoreWebView2,
        eventHandler: *mut ICoreWebView2NavigationStartingEventHandler,
        token: *mut EventRegistrationToken,
    ) -> HRESULT,
    padding2: [usize; 7],
    add_NavigationCompleted: unsafe extern "system" fn(
        This: *mut ICoreWebView2,
        eventHandler: *mut ICoreWebView2NavigationCompletedEventHandler,
        token: *mut EventRegistrationToken,
    ) -> HRESULT,
    padding3: [usize; 11],
    AddScriptToExecuteOnDocumentCreated: unsafe extern "system" fn(
        This: *mut ICoreWebView2,
        script: *const w_char,
        resultHandler: *mut c_void,
    ) -> HRESULT,
    padding4: [usize; 1],
    ExecuteScript: unsafe extern "system" fn(
        This: *mut ICoreWebView2,
        script: *const w_char,
        resultHandler: *mut c_void,
    ) -> HRESULT,
    padding5: [usize; 4],
    add_WebMessageReceived: unsafe extern "system" fn(
        This: *mut ICoreWebView2,
        eventHandler: *mut ICoreWebView2WebMessageReceivedEventHandler,
        token: *mut EventRegistrationToken,
    ) -> HRESULT,
    padding6: [usize; 9],
    add_NewWindowRequested: unsafe extern "system" fn(
        This: *mut ICoreWebView2,
        eventHandler: *mut ICoreWebView2NewWindowRequestedEventHandler,
        token: *mut EventRegistrationToken,
    ) -> HRESULT,
    padding7: [usize; 1],
    add_DocumentTitleChanged: unsafe extern "system" fn(
        This: *mut ICoreWebView2,
        eventHandler: *mut ICoreWebView2DocumentTitleChangedEventHandler,
        token: *mut EventRegistrationToken,
    ) -> HRESULT,
    padding8: [usize; 1],
    get_DocumentTitle:
        unsafe extern "system" fn(This: *mut ICoreWebView2, title: *mut *mut w_char) -> HRESULT,
    padding9: [usize; 6],
    add_WebResourceRequested: unsafe extern "system" fn(
        This: *mut ICoreWebView2,
        eventHandler: *mut ICoreWebView2WebResourceRequestedEventHandler,
        token: *mut EventRegistrationToken,
    ) -> HRESULT,
    padding10: [usize; 1],
    AddWebResourceRequestedFilter: unsafe extern "system" fn(
        This: *mut ICoreWebView2,
        uri: *const w_char,
        resourceContext: i32,
    ) -> HRESULT,
}

// ICoreWebView2WebMessageReceivedEventHandler
#[repr(C)]
pub(crate) struct ICoreWebView2WebMessageReceivedEventHandler {
    pub(crate) lpVtbl: *const ICoreWebView2WebMessageReceivedEventHandlerVtbl,
    pub(crate) user_data: *mut c_void,
}

#[repr(C)]
pub(crate) struct ICoreWebView2WebMessageReceivedEventHandlerVtbl {
    pub(crate) QueryInterface: unsafe extern "system" fn(
        This: *mut c_void,
        riid: *const GUID,
        ppvObject: *mut *mut c_void,
    ) -> HRESULT,
    pub(crate) AddRef: unsafe extern "system" fn(This: *mut c_void) -> u32,
    pub(crate) Release: unsafe extern "system" fn(This: *mut c_void) -> u32,
    pub(crate) Invoke: unsafe extern "system" fn(
        This: *mut ICoreWebView2WebMessageReceivedEventHandler,
        sender: *mut ICoreWebView2,
        args: *mut ICoreWebView2WebMessageReceivedEventArgs,
    ) -> HRESULT,
}

// ICoreWebView2WebMessageReceivedEventArgs
#[repr(C)]
pub(crate) struct ICoreWebView2WebMessageReceivedEventArgs {
    pub(crate) lpVtbl: *const ICoreWebView2WebMessageReceivedEventArgsVtbl,
}

impl ICoreWebView2WebMessageReceivedEventArgs {
    pub(crate) unsafe fn TryGetWebMessageAsString(&self, message: *mut *mut w_char) -> HRESULT {
        unsafe { ((*self.lpVtbl).TryGetWebMessageAsString)(self as *const _ as *mut _, message) }
    }
}

#[repr(C)]
pub(crate) struct ICoreWebView2WebMessageReceivedEventArgsVtbl {
    pub(crate) QueryInterface: unsafe extern "system" fn(
        This: *mut c_void,
        riid: *const GUID,
        ppvObject: *mut *mut c_void,
    ) -> HRESULT,
    pub(crate) AddRef: unsafe extern "system" fn(This: *mut c_void) -> u32,
    pub(crate) Release: unsafe extern "system" fn(This: *mut c_void) -> u32,
    padding: [usize; 2],
    pub(crate) TryGetWebMessageAsString: unsafe extern "system" fn(
        This: *mut ICoreWebView2WebMessageReceivedEventArgs,
        message: *mut *mut w_char,
    ) -> HRESULT,
}

// ICoreWebView2Settings
#[repr(C)]
pub(crate) struct ICoreWebView2Settings {
    pub(crate) lpVtbl: *const ICoreWebView2SettingsVtbl,
}

impl ICoreWebView2Settings {
    pub(crate) unsafe fn QueryInterface(
        &self,
        riid: *const GUID,
        ppvObject: *mut *mut c_void,
    ) -> HRESULT {
        unsafe { ((*self.lpVtbl).QueryInterface)(self as *const _ as *mut _, riid, ppvObject) }
    }
}

#[repr(C)]
pub(crate) struct ICoreWebView2SettingsVtbl {
    QueryInterface: unsafe extern "system" fn(
        This: *mut c_void,
        riid: *const GUID,
        ppvObject: *mut *mut c_void,
    ) -> HRESULT,
    AddRef: unsafe extern "system" fn(This: *mut c_void) -> u32,
    Release: unsafe extern "system" fn(This: *mut c_void) -> u32,
}

// ICoreWebView2Settings2
pub(crate) const IID_ICoreWebView2Settings2: GUID = GUID {
    data1: 0xee9a0f68,
    data2: 0xf46c,
    data3: 0x4e32,
    data4: [0xac, 0x23, 0xef, 0x8c, 0xac, 0x22, 0x4d, 0x2a],
};

#[repr(C)]
pub(crate) struct ICoreWebView2Settings2 {
    pub(crate) lpVtbl: *const ICoreWebView2Settings2Vtbl,
}

impl ICoreWebView2Settings2 {
    pub(crate) unsafe fn put_UserAgent(&self, userAgent: *const w_char) -> HRESULT {
        unsafe { ((*self.lpVtbl).put_UserAgent)(self as *const _ as *mut _, userAgent) }
    }
}

#[repr(C)]
pub(crate) struct ICoreWebView2Settings2Vtbl {
    QueryInterface: unsafe extern "system" fn(
        This: *mut c_void,
        riid: *const GUID,
        ppvObject: *mut *mut c_void,
    ) -> HRESULT,
    AddRef: unsafe extern "system" fn(This: *mut c_void) -> u32,
    Release: unsafe extern "system" fn(This: *mut c_void) -> u32,
    padding: [usize; 19],
    put_UserAgent: unsafe extern "system" fn(
        This: *mut ICoreWebView2Settings2,
        userAgent: *const w_char,
    ) -> HRESULT,
}

// ICoreWebView2NavigationStartingEventHandler
#[repr(C)]
pub(crate) struct ICoreWebView2NavigationStartingEventHandler {
    pub(crate) lpVtbl: *const ICoreWebView2NavigationStartingEventHandlerVtbl,
    pub(crate) user_data: *mut c_void,
}

#[repr(C)]
pub(crate) struct ICoreWebView2NavigationStartingEventArgs {
    pub(crate) lpVtbl: *const c_void,
}

#[repr(C)]
pub(crate) struct ICoreWebView2NavigationStartingEventHandlerVtbl {
    pub(crate) QueryInterface: unsafe extern "system" fn(
        This: *mut c_void,
        riid: *const GUID,
        ppvObject: *mut *mut c_void,
    ) -> HRESULT,
    pub(crate) AddRef: unsafe extern "system" fn(This: *mut c_void) -> u32,
    pub(crate) Release: unsafe extern "system" fn(This: *mut c_void) -> u32,
    pub(crate) Invoke: unsafe extern "system" fn(
        This: *mut ICoreWebView2NavigationStartingEventHandler,
        sender: *mut ICoreWebView2,
        args: *mut ICoreWebView2NavigationStartingEventArgs,
    ) -> HRESULT,
}

// ICoreWebView2NavigationCompletedEventHandler
#[repr(C)]
pub(crate) struct ICoreWebView2NavigationCompletedEventHandler {
    pub(crate) lpVtbl: *const ICoreWebView2NavigationCompletedEventHandlerVtbl,
    pub(crate) user_data: *mut c_void,
}

#[repr(C)]
pub(crate) struct ICoreWebView2NavigationCompletedEventArgs {
    pub(crate) lpVtbl: *const c_void,
}

#[repr(C)]
pub(crate) struct ICoreWebView2NavigationCompletedEventHandlerVtbl {
    pub(crate) QueryInterface: unsafe extern "system" fn(
        This: *mut c_void,
        riid: *const GUID,
        ppvObject: *mut *mut c_void,
    ) -> HRESULT,
    pub(crate) AddRef: unsafe extern "system" fn(This: *mut c_void) -> u32,
    pub(crate) Release: unsafe extern "system" fn(This: *mut c_void) -> u32,
    pub(crate) Invoke: unsafe extern "system" fn(
        This: *mut ICoreWebView2NavigationCompletedEventHandler,
        sender: *mut ICoreWebView2,
        args: *mut ICoreWebView2NavigationCompletedEventArgs,
    ) -> HRESULT,
}

// ICoreWebView2DocumentTitleChangedEventHandler
#[repr(C)]
pub(crate) struct ICoreWebView2DocumentTitleChangedEventHandler {
    pub(crate) lpVtbl: *const ICoreWebView2DocumentTitleChangedEventHandlerVtbl,
    pub(crate) user_data: *mut c_void,
}

#[repr(C)]
pub(crate) struct ICoreWebView2DocumentTitleChangedEventHandlerVtbl {
    pub(crate) QueryInterface: unsafe extern "system" fn(
        This: *mut c_void,
        riid: *const GUID,
        ppvObject: *mut *mut c_void,
    ) -> HRESULT,
    pub(crate) AddRef: unsafe extern "system" fn(This: *mut c_void) -> u32,
    pub(crate) Release: unsafe extern "system" fn(This: *mut c_void) -> u32,
    pub(crate) Invoke: unsafe extern "system" fn(
        This: *mut ICoreWebView2DocumentTitleChangedEventHandler,
        sender: *mut ICoreWebView2,
        args: *mut c_void,
    ) -> HRESULT,
}

// ICoreWebView2NewWindowRequestedEventHandlerVtbl
#[repr(C)]
pub(crate) struct ICoreWebView2NewWindowRequestedEventHandler {
    pub(crate) lpVtbl: *const ICoreWebView2NewWindowRequestedEventHandlerVtbl,
    pub(crate) user_data: *mut c_void,
}

#[repr(C)]
pub(crate) struct ICoreWebView2NewWindowRequestedEventHandlerVtbl {
    pub(crate) QueryInterface: unsafe extern "system" fn(
        This: *mut c_void,
        riid: *const GUID,
        ppvObject: *mut *mut c_void,
    ) -> HRESULT,
    pub(crate) AddRef: unsafe extern "system" fn(This: *mut c_void) -> u32,
    pub(crate) Release: unsafe extern "system" fn(This: *mut c_void) -> u32,
    pub(crate) Invoke: unsafe extern "system" fn(
        This: *mut ICoreWebView2NewWindowRequestedEventHandler,
        sender: *mut ICoreWebView2,
        args: *mut ICoreWebView2NewWindowRequestedEventArgs,
    ) -> HRESULT,
}

// ICoreWebView2NewWindowRequestedEventArgs
#[repr(C)]
pub(crate) struct ICoreWebView2NewWindowRequestedEventArgs {
    pub(crate) lpVtbl: *const ICoreWebView2NewWindowRequestedEventArgsVtbl,
}

impl ICoreWebView2NewWindowRequestedEventArgs {
    pub(crate) unsafe fn get_Uri(&self, uri: *mut *mut w_char) -> HRESULT {
        unsafe { ((*self.lpVtbl).get_Uri)(self as *const _ as *mut _, uri) }
    }

    pub(crate) unsafe fn put_Handled(&self, handled: BOOL) -> HRESULT {
        unsafe { ((*self.lpVtbl).put_Handled)(self as *const _ as *mut _, handled) }
    }
}

#[repr(C)]
pub(crate) struct ICoreWebView2NewWindowRequestedEventArgsVtbl {
    pub(crate) QueryInterface: unsafe extern "system" fn(
        This: *mut c_void,
        riid: *const GUID,
        ppvObject: *mut *mut c_void,
    ) -> HRESULT,
    pub(crate) AddRef: unsafe extern "system" fn(This: *mut c_void) -> u32,
    pub(crate) Release: unsafe extern "system" fn(This: *mut c_void) -> u32,
    pub(crate) get_Uri: unsafe extern "system" fn(
        This: *mut ICoreWebView2NewWindowRequestedEventArgs,
        uri: *mut *mut w_char,
    ) -> HRESULT,
    padding: [usize; 2],
    pub(crate) put_Handled: unsafe extern "system" fn(
        This: *mut ICoreWebView2NewWindowRequestedEventArgs,
        handled: BOOL,
    ) -> HRESULT,
}

// ICoreWebView2WebResourceRequestedEventHandler
#[repr(C)]
pub(crate) struct ICoreWebView2WebResourceRequestedEventHandler {
    pub(crate) lpVtbl: *const ICoreWebView2WebResourceRequestedEventHandlerVtbl,
    pub(crate) user_data: *mut c_void,
}

#[repr(C)]
pub(crate) struct ICoreWebView2WebResourceRequestedEventHandlerVtbl {
    pub(crate) QueryInterface: unsafe extern "system" fn(
        This: *mut c_void,
        riid: *const GUID,
        ppvObject: *mut *mut c_void,
    ) -> HRESULT,
    pub(crate) AddRef: unsafe extern "system" fn(This: *mut c_void) -> u32,
    pub(crate) Release: unsafe extern "system" fn(This: *mut c_void) -> u32,
    pub(crate) Invoke: unsafe extern "system" fn(
        This: *mut ICoreWebView2WebResourceRequestedEventHandler,
        sender: *mut ICoreWebView2,
        args: *mut ICoreWebView2WebResourceRequestedEventArgs,
    ) -> HRESULT,
}

// ICoreWebView2WebResourceRequestedEventArgs
#[repr(C)]
pub(crate) struct ICoreWebView2WebResourceRequestedEventArgs {
    pub(crate) lpVtbl: *const ICoreWebView2WebResourceRequestedEventArgsVtbl,
}

impl ICoreWebView2WebResourceRequestedEventArgs {
    pub(crate) unsafe fn get_Request(
        &self,
        request: *mut *mut ICoreWebView2WebResourceRequest,
    ) -> HRESULT {
        unsafe { ((*self.lpVtbl).get_Request)(self as *const _ as *mut _, request) }
    }

    pub(crate) unsafe fn put_Response(
        &self,
        response: *mut ICoreWebView2WebResourceResponse,
    ) -> HRESULT {
        unsafe { ((*self.lpVtbl).put_Response)(self as *const _ as *mut _, response) }
    }
}

#[repr(C)]
pub(crate) struct ICoreWebView2WebResourceRequestedEventArgsVtbl {
    pub(crate) QueryInterface: unsafe extern "system" fn(
        This: *mut c_void,
        riid: *const GUID,
        ppvObject: *mut *mut c_void,
    ) -> HRESULT,
    pub(crate) AddRef: unsafe extern "system" fn(This: *mut c_void) -> u32,
    pub(crate) Release: unsafe extern "system" fn(This: *mut c_void) -> u32,
    pub(crate) get_Request: unsafe extern "system" fn(
        This: *mut ICoreWebView2WebResourceRequestedEventArgs,
        request: *mut *mut ICoreWebView2WebResourceRequest,
    ) -> HRESULT,
    padding: [usize; 1],
    pub(crate) put_Response: unsafe extern "system" fn(
        This: *mut ICoreWebView2WebResourceRequestedEventArgs,
        response: *mut ICoreWebView2WebResourceResponse,
    ) -> HRESULT,
}

// ICoreWebView2WebResourceRequest
#[repr(C)]
pub(crate) struct ICoreWebView2WebResourceRequest {
    pub(crate) lpVtbl: *const ICoreWebView2WebResourceRequestVtbl,
}

impl ICoreWebView2WebResourceRequest {
    pub(crate) unsafe fn Release(&self) -> u32 {
        unsafe { ((*self.lpVtbl).Release)(self as *const _ as *mut _) }
    }

    pub(crate) unsafe fn get_Uri(&self, uri: *mut *mut w_char) -> HRESULT {
        unsafe { ((*self.lpVtbl).get_Uri)(self as *const _ as *mut _, uri) }
    }

    pub(crate) unsafe fn get_Method(&self, method: *mut *mut w_char) -> HRESULT {
        unsafe { ((*self.lpVtbl).get_Method)(self as *const _ as *mut _, method) }
    }

    pub(crate) unsafe fn get_Content(&self, content: *mut *mut IStream) -> HRESULT {
        unsafe { ((*self.lpVtbl).get_Content)(self as *const _ as *mut _, content) }
    }

    pub(crate) unsafe fn get_Headers(
        &self,
        headers: *mut *mut ICoreWebView2HttpRequestHeaders,
    ) -> HRESULT {
        unsafe { ((*self.lpVtbl).get_Headers)(self as *const _ as *mut _, headers) }
    }
}

#[repr(C)]
pub(crate) struct ICoreWebView2WebResourceRequestVtbl {
    pub(crate) QueryInterface: unsafe extern "system" fn(
        This: *mut c_void,
        riid: *const GUID,
        ppvObject: *mut *mut c_void,
    ) -> HRESULT,
    pub(crate) AddRef: unsafe extern "system" fn(This: *mut c_void) -> u32,
    pub(crate) Release: unsafe extern "system" fn(This: *mut c_void) -> u32,
    pub(crate) get_Uri: unsafe extern "system" fn(
        This: *mut ICoreWebView2WebResourceRequest,
        uri: *mut *mut w_char,
    ) -> HRESULT,
    padding1: [usize; 1],
    pub(crate) get_Method: unsafe extern "system" fn(
        This: *mut ICoreWebView2WebResourceRequest,
        method: *mut *mut w_char,
    ) -> HRESULT,
    padding2: [usize; 1],
    get_Content: unsafe extern "system" fn(
        This: *mut ICoreWebView2WebResourceRequest,
        content: *mut *mut IStream,
    ) -> HRESULT,
    padding3: [usize; 1],
    pub(crate) get_Headers: unsafe extern "system" fn(
        This: *mut ICoreWebView2WebResourceRequest,
        headers: *mut *mut ICoreWebView2HttpRequestHeaders,
    ) -> HRESULT,
}

// ICoreWebView2HttpRequestHeaders
#[repr(C)]
pub(crate) struct ICoreWebView2HttpRequestHeaders {
    pub(crate) lpVtbl: *const ICoreWebView2HttpRequestHeadersVtbl,
}

impl ICoreWebView2HttpRequestHeaders {
    pub(crate) unsafe fn Release(&self) -> u32 {
        unsafe { ((*self.lpVtbl).Release)(self as *const _ as *mut _) }
    }

    pub(crate) unsafe fn GetIterator(
        &self,
        iterator: *mut *mut ICoreWebView2HttpRequestHeadersIterator,
    ) -> HRESULT {
        unsafe { ((*self.lpVtbl).GetIterator)(self as *const _ as *mut _, iterator) }
    }
}

#[repr(C)]
pub(crate) struct ICoreWebView2HttpRequestHeadersVtbl {
    pub(crate) QueryInterface: unsafe extern "system" fn(
        This: *mut c_void,
        riid: *const GUID,
        ppvObject: *mut *mut c_void,
    ) -> HRESULT,
    pub(crate) AddRef: unsafe extern "system" fn(This: *mut c_void) -> u32,
    pub(crate) Release: unsafe extern "system" fn(This: *mut c_void) -> u32,
    padding: [usize; 5],
    pub(crate) GetIterator: unsafe extern "system" fn(
        This: *mut ICoreWebView2HttpRequestHeaders,
        iterator: *mut *mut ICoreWebView2HttpRequestHeadersIterator,
    ) -> HRESULT,
}

// ICoreWebView2HttpRequestHeadersIterator
#[repr(C)]
pub(crate) struct ICoreWebView2HttpRequestHeadersIterator {
    pub(crate) lpVtbl: *const ICoreWebView2HttpRequestHeadersIteratorVtbl,
}

impl ICoreWebView2HttpRequestHeadersIterator {
    pub(crate) unsafe fn Release(&self) -> u32 {
        unsafe { ((*self.lpVtbl).Release)(self as *const _ as *mut _) }
    }

    pub(crate) unsafe fn GetCurrentHeader(
        &self,
        name: *mut *mut w_char,
        value: *mut *mut w_char,
    ) -> HRESULT {
        unsafe { ((*self.lpVtbl).GetCurrentHeader)(self as *const _ as *mut _, name, value) }
    }

    pub(crate) unsafe fn get_HasCurrentHeader(&self, hasCurrent: *mut BOOL) -> HRESULT {
        unsafe { ((*self.lpVtbl).get_HasCurrentHeader)(self as *const _ as *mut _, hasCurrent) }
    }

    pub(crate) unsafe fn MoveNext(&self, hasNext: *mut BOOL) -> HRESULT {
        unsafe { ((*self.lpVtbl).MoveNext)(self as *const _ as *mut _, hasNext) }
    }
}

#[repr(C)]
pub(crate) struct ICoreWebView2HttpRequestHeadersIteratorVtbl {
    pub(crate) QueryInterface: unsafe extern "system" fn(
        This: *mut c_void,
        riid: *const GUID,
        ppvObject: *mut *mut c_void,
    ) -> HRESULT,
    pub(crate) AddRef: unsafe extern "system" fn(This: *mut c_void) -> u32,
    pub(crate) Release: unsafe extern "system" fn(This: *mut c_void) -> u32,
    pub(crate) GetCurrentHeader: unsafe extern "system" fn(
        This: *mut ICoreWebView2HttpRequestHeadersIterator,
        name: *mut *mut w_char,
        value: *mut *mut w_char,
    ) -> HRESULT,
    pub(crate) get_HasCurrentHeader: unsafe extern "system" fn(
        This: *mut ICoreWebView2HttpRequestHeadersIterator,
        hasCurrent: *mut BOOL,
    ) -> HRESULT,
    pub(crate) MoveNext: unsafe extern "system" fn(
        This: *mut ICoreWebView2HttpRequestHeadersIterator,
        hasNext: *mut BOOL,
    ) -> HRESULT,
}

// ICoreWebView2WebResourceResponse
#[repr(C)]
pub(crate) struct ICoreWebView2WebResourceResponse {
    pub(crate) lpVtbl: *const ICoreWebView2WebResourceResponseVtbl,
}

impl ICoreWebView2WebResourceResponse {
    pub(crate) unsafe fn Release(&self) -> u32 {
        unsafe { ((*self.lpVtbl).Release)(self as *const _ as *mut _) }
    }
}

#[repr(C)]
pub(crate) struct ICoreWebView2WebResourceResponseVtbl {
    pub(crate) QueryInterface: unsafe extern "system" fn(
        This: *mut c_void,
        riid: *const GUID,
        ppvObject: *mut *mut c_void,
    ) -> HRESULT,
    pub(crate) AddRef: unsafe extern "system" fn(This: *mut c_void) -> u32,
    pub(crate) Release: unsafe extern "system" fn(This: *mut c_void) -> u32,
}

pub(crate) const IID_ICoreWebView2CreateCoreWebView2ControllerCompletedHandler: GUID = GUID {
    data1: 0x6c4819f3,
    data2: 0xc9b7,
    data3: 0x4260,
    data4: [0x81, 0x27, 0xc9, 0xf5, 0xbd, 0xe7, 0xf6, 0x8c],
};

pub(crate) const IID_ICoreWebView2WebResourceRequestedEventHandler: GUID = GUID {
    data1: 0xab00b74c,
    data2: 0x15f1,
    data3: 0x4646,
    data4: [0x80, 0xe8, 0xe7, 0x63, 0x41, 0xd2, 0x5d, 0x71],
};

pub(crate) const IID_ICoreWebView2NavigationStartingEventHandler: GUID = GUID {
    data1: 0x9adbe429,
    data2: 0xf36d,
    data3: 0x432b,
    data4: [0x9d, 0xdc, 0xf8, 0x88, 0x1f, 0xbd, 0x76, 0xe3],
};

pub(crate) const IID_ICoreWebView2NavigationCompletedEventHandler: GUID = GUID {
    data1: 0xd33a35bf,
    data2: 0x1c49,
    data3: 0x4f98,
    data4: [0x93, 0xab, 0x00, 0x6e, 0x05, 0x33, 0xfe, 0x1c],
};

pub(crate) const IID_ICoreWebView2DocumentTitleChangedEventHandler: GUID = GUID {
    data1: 0xf5f2b923,
    data2: 0x953e,
    data3: 0x4042,
    data4: [0x9f, 0x95, 0xf3, 0xa1, 0x18, 0xe1, 0xaf, 0xd4],
};

pub(crate) const IID_ICoreWebView2NewWindowRequestedEventHandler: GUID = GUID {
    data1: 0xd4c185fe,
    data2: 0xc81c,
    data3: 0x4989,
    data4: [0x97, 0xaf, 0x2d, 0x3f, 0xa7, 0xab, 0x56, 0x51],
};

pub(crate) const IID_ICoreWebView2WebMessageReceivedEventHandler: GUID = GUID {
    data1: 0x57213f19,
    data2: 0x00e6,
    data3: 0x49fa,
    data4: [0x8e, 0x07, 0x89, 0x8e, 0xa0, 0x1e, 0xcb, 0xd2],
};
