/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(missing_docs, reason = "Raw Win32 declarations")]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(clippy::upper_case_acronyms)]
#![allow(unused)]

use std::ffi::{c_char, c_void};

// MARK: Base types
pub type BOOL = i32;
pub const TRUE: BOOL = 1;
pub const FALSE: BOOL = 0;
pub type w_char = u16;
pub type HANDLE = *mut c_void;
pub type HKEY = HANDLE;
pub const HKEY_CURRENT_USER: HKEY = 0x80000001usize as HKEY;
pub const HKEY_LOCAL_MACHINE: HKEY = 0x80000002usize as HKEY;
pub const RRF_RT_REG_DWORD: u32 = 0x00000018;
pub const RRF_RT_REG_SZ: u32 = 0x00000002;
pub const RRF_SUBKEY_WOW6432KEY: u32 = 0x00020000;
pub const ERROR_SUCCESS: i32 = 0;

#[repr(C)]
pub struct FILETIME {
    pub dwLowDateTime: u32,
    pub dwHighDateTime: u32,
}

// MARK: kernel32.dll
pub type HMODULE = *const c_void;

pub const ERROR_ALREADY_EXISTS: u32 = 183;

#[link(name = "kernel32")]
unsafe extern "system" {
    pub fn GetModuleHandleW(lpModuleName: *const w_char) -> HMODULE;
    pub fn LoadLibraryW(lpLibFileName: *const u16) -> HMODULE;
    pub fn GetProcAddress(hModule: HMODULE, lpProcName: *const c_char) -> *const c_void;
    pub fn FreeLibrary(hLibModule: HMODULE) -> BOOL;
    pub fn GetLastError() -> u32;
    pub fn CreateMutexW(
        lpMutexAttributes: *mut c_void,
        bInitialOwner: BOOL,
        lpName: *const w_char,
    ) -> HANDLE;
}

// MARK: advapi32.dll
#[link(name = "advapi32")]
unsafe extern "system" {
    pub fn RegGetValueW(
        hkey: HKEY,
        lpSubKey: *const w_char,
        lpValue: *const w_char,
        dwFlags: u32,
        pdwType: *mut u32,
        pvData: *mut c_void,
        pcbData: *mut u32,
    ) -> i32;
}

// MARK: gdi32.dll
pub type HBRUSH = HANDLE;
pub type HDC = HANDLE;

#[link(name = "gdi32")]
unsafe extern "system" {
    pub fn FillRect(hdc: HDC, lprc: *const RECT, hbr: HBRUSH) -> i32;
    pub fn CreateSolidBrush(color: u32) -> HBRUSH;
    pub fn DeleteObject(ho: *mut c_void) -> BOOL;
}

// MARK: user32.dll
pub type HWND = HANDLE;
pub type HCURSOR = HANDLE;
pub type HICON = HANDLE;
pub type HMENU = HANDLE;
pub type HMONITOR = HANDLE;
pub type ATOM = u16;
pub type WPARAM = usize;
pub type LPARAM = isize;
pub type LRESULT = isize;

pub const IDOK: i32 = 1;
pub const IDCANCEL: i32 = 2;
pub const IDYES: i32 = 6;
pub const IDNO: i32 = 7;

#[repr(C)]
#[derive(Default)]
pub struct WNDCLASSEXW {
    pub cbSize: u32,
    pub style: u32,
    pub lpfnWndProc: Option<
        unsafe extern "system" fn(
            window: HWND,
            message: u32,
            wparam: WPARAM,
            lparam: LPARAM,
        ) -> LRESULT,
    >,
    pub cbClsExtra: i32,
    pub cbWndExtra: i32,
    pub hInstance: HMODULE,
    pub hIcon: HICON,
    pub hCursor: HCURSOR,
    pub hbrBackground: usize,
    pub lpszMenuName: *const w_char,
    pub lpszClassName: *const w_char,
    pub hIconSm: HICON,
}

// MARK: comctl32.dll
pub const TDF_ALLOW_DIALOG_CANCELLATION: u32 = 0x0008;
pub const TDCBF_OK_BUTTON: u32 = 0x0001;
pub const TDCBF_YES_BUTTON: u32 = 0x0002;
pub const TDCBF_NO_BUTTON: u32 = 0x0004;
pub const TDCBF_CANCEL_BUTTON: u32 = 0x0008;
pub const TD_WARNING_ICON: *const w_char = u16::MAX as usize as *const w_char;
pub const TD_ERROR_ICON: *const w_char = (u16::MAX - 1) as usize as *const w_char;
pub const TD_INFORMATION_ICON: *const w_char = (u16::MAX - 2) as usize as *const w_char;

#[repr(C)]
pub struct TASKDIALOG_BUTTON {
    pub nButtonID: i32,
    pub pszButtonText: *const w_char,
}

pub type TaskDialogCallback =
    Option<unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM, isize) -> i32>;

#[repr(C)]
pub struct TASKDIALOGCONFIG {
    pub cbSize: u32,
    pub hwndParent: HWND,
    pub hInstance: HMODULE,
    pub dwFlags: u32,
    pub dwCommonButtons: u32,
    pub pszWindowTitle: *const w_char,
    pub mainIcon: *const w_char,
    pub pszMainInstruction: *const w_char,
    pub pszContent: *const w_char,
    pub cButtons: u32,
    pub pButtons: *const TASKDIALOG_BUTTON,
    pub nDefaultButton: i32,
    pub cRadioButtons: u32,
    pub pRadioButtons: *const TASKDIALOG_BUTTON,
    pub nDefaultRadioButton: i32,
    pub pszVerificationText: *const w_char,
    pub pszExpandedInformation: *const w_char,
    pub pszExpandedControlText: *const w_char,
    pub pszCollapsedControlText: *const w_char,
    pub footerIcon: *const w_char,
    pub pszFooter: *const w_char,
    pub pfCallback: TaskDialogCallback,
    pub lpCallbackData: isize,
    pub cxWidth: u32,
}

#[link(name = "comctl32")]
unsafe extern "system" {
    pub fn TaskDialogIndirect(
        pTaskConfig: *const TASKDIALOGCONFIG,
        pnButton: *mut i32,
        pnRadioButton: *mut i32,
        pfVerificationFlagChecked: *mut BOOL,
    ) -> i32;
}

#[repr(C)]
pub struct CREATESTRUCTW {
    pub lpCreateParams: *mut c_void,
    pub hInstance: HMODULE,
    pub hMenu: HMENU,
    pub hwndParent: HWND,
    pub cy: i32,
    pub cx: i32,
    pub y: i32,
    pub x: i32,
    pub style: i32,
    pub lpszName: *const w_char,
    pub lpszClass: *const w_char,
    pub dwExStyle: u32,
}

#[repr(C)]
pub struct MSG {
    pub hwnd: HWND,
    pub message: u32,
    pub wParam: WPARAM,
    pub lParam: LPARAM,
    pub time: u32,
    pub pt_x: i32,
    pub pt_y: i32,
}

#[repr(C)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
#[derive(Default, Clone)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
#[derive(Default)]
pub struct PAINTSTRUCT {
    pub hdc: HDC,
    pub fErase: BOOL,
    pub rcPaint: RECT,
    pub fRestore: BOOL,
    pub fIncUpdate: BOOL,
    pub rgbReserved: [u8; 32],
}

const _: () = assert!(size_of::<PAINTSTRUCT>() == if size_of::<HWND>() == 8 { 72 } else { 64 });

#[repr(C)]
pub struct MINMAXINFO {
    pub ptReserved: POINT,
    pub ptMaxSize: POINT,
    pub ptMaxPosition: POINT,
    pub ptMinTrackSize: POINT,
    pub ptMaxTrackSize: POINT,
}

#[repr(C)]
#[derive(Default)]
pub struct MONITORINFOEXW {
    pub cbSize: u32,
    pub rcMonitor: RECT,
    pub rcWork: RECT,
    pub dwFlags: u32,
    pub szDevice: [w_char; 32],
}

#[repr(C)]
pub struct WINDOWPLACEMENT {
    pub length: u32,
    pub flags: u32,
    pub showCmd: u32,
    pub ptMinPosition: POINT,
    pub ptMaxPosition: POINT,
    pub rcNormalPosition: RECT,
}

pub const WS_POPUP: u32 = 0x80000000;
pub const WS_THICKFRAME: u32 = 0x00040000;
pub const WS_OVERLAPPEDWINDOW: u32 = 0x00CF0000;
pub const WS_MAXIMIZEBOX: u32 = 0x00010000;

pub const CW_USEDEFAULT: i32 = 0x80000000u32 as i32;

pub const WM_CREATE: u32 = 0x0001;
pub const WM_DESTROY: u32 = 0x0002;
pub const WM_MOVE: u32 = 0x0003;
pub const WM_SIZE: u32 = 0x0005;
pub const WM_PAINT: u32 = 0x000f;
pub const WM_CLOSE: u32 = 0x0010;
pub const WM_ERASEBKGND: u32 = 0x0014;
pub const WM_GETMINMAXINFO: u32 = 0x0024;
pub const WM_NCCREATE: u32 = 0x0081;
pub const WM_DROPFILES: u32 = 0x0233;
pub const WM_DPICHANGED: u32 = 0x02E0;
pub const WM_USER: u32 = 0x0400;
pub const WM_APP: u32 = 0x8000;

pub const SW_SHOWNORMAL: i32 = 1;
pub const SW_RESTORE: i32 = 9;
pub const SW_SHOWDEFAULT: i32 = 10;

pub const MB_OK: u32 = 0x00000000;
pub const MB_OKCANCEL: u32 = 0x00000001;
pub const MB_YESNOCANCEL: u32 = 0x00000003;
pub const MB_YESNO: u32 = 0x00000004;
pub const MB_ICONERROR: u32 = 0x00000010;
pub const MB_ICONWARNING: u32 = 0x00000030;
pub const MB_ICONINFORMATION: u32 = 0x00000040;

pub const PROCESS_PER_MONITOR_DPI_AWARE: i32 = 2;
pub const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2: isize = -4isize;

pub const MONITOR_DEFAULTTOPRIMARY: u32 = 0x00000001;

pub const USER_DEFAULT_SCREEN_DPI: u32 = 96;
pub const MDT_EFFECTIVE_DPI: i32 = 0;

pub const SM_CXSCREEN: i32 = 0;
pub const SM_CYSCREEN: i32 = 1;
pub const COLOR_WINDOW: i32 = 5;

pub const GWL_STYLE: i32 = -16;
pub const GWL_USERDATA: i32 = -21;

pub const SWP_NOSIZE: u32 = 0x0001;
pub const SWP_NOMOVE: u32 = 0x0002;
pub const SWP_NOZORDER: u32 = 0x0004;
pub const SWP_NOACTIVATE: u32 = 0x0010;
pub const SWP_NOREPOSITION: u32 = 0x0200;

#[repr(C)]
pub struct ACCEL {
    pub fVirt: u8,
    pub key: u16,
    pub cmd: u16,
}
#[link(name = "user32")]
unsafe extern "system" {
    pub fn CreateAcceleratorTableW(accels: *const ACCEL, count: i32) -> HANDLE;
    pub fn DestroyAcceleratorTable(table: HANDLE) -> BOOL;
    pub fn TranslateAcceleratorW(window: HWND, table: HANDLE, message: *mut MSG) -> i32;
    pub fn VkKeyScanW(character: u16) -> i16;
    pub fn GetKeyState(key: i32) -> i16;
    pub fn GetKeyboardState(state: *mut u8) -> i32;
    pub fn GetKeyboardLayout(thread: u32) -> isize;
    pub fn ToUnicodeEx(
        key: u32,
        scan: u32,
        state: *const u8,
        text: *mut u16,
        count: i32,
        flags: u32,
        layout: isize,
    ) -> i32;
    pub fn EnumChildWindows(
        hwndParent: HWND,
        lpEnumFunc: unsafe extern "system" fn(HWND, LPARAM) -> BOOL,
        lParam: LPARAM,
    ) -> BOOL;
    pub fn GetSysColorBrush(nIndex: i32) -> HBRUSH;
    pub fn GetActiveWindow() -> HWND;
    pub fn ExtractIconExW(
        lpszFile: *const w_char,
        nIconIndex: i32,
        phiconLarge: *mut HICON,
        phiconSmall: *mut HICON,
        nIcons: u32,
    ) -> u32;
    pub fn GetClassInfoExW(
        hInstance: HMODULE,
        lpClassName: *const w_char,
        lpWndClass: *mut WNDCLASSEXW,
    ) -> BOOL;
    pub fn RegisterClassExW(lpWndClass: *const WNDCLASSEXW) -> ATOM;
    pub fn CreateWindowExW(
        dwExStyle: u32,
        lpClassName: *const w_char,
        lpWindowName: *const w_char,
        dwStyle: u32,
        X: i32,
        Y: i32,
        nWidth: i32,
        nHeight: i32,
        hWndParent: HWND,
        hMenu: HMENU,
        hInstance: HMODULE,
        lpParam: LPARAM,
    ) -> HWND;
    pub fn SetWindowTextW(hWnd: HWND, lpString: *const w_char) -> i32;
    pub fn ShowWindow(hWnd: HWND, nCmdShow: i32) -> i32;
    pub fn UpdateWindow(hWnd: HWND) -> i32;
    pub fn GetWindowRect(hWnd: HWND, rect: *mut RECT) -> i32;
    pub fn GetClientRect(hWnd: HWND, rect: *mut RECT) -> i32;
    pub fn SetWindowPos(
        hWnd: HWND,
        hWndInsertAfter: HWND,
        X: i32,
        Y: i32,
        cx: i32,
        cy: i32,
        uFlags: u32,
    ) -> BOOL;
    pub fn GetMessageW(lpMsg: *mut MSG, hWnd: HWND, wMsgFilterMin: u32, wMsgFilterMax: u32) -> i32;
    pub fn TranslateMessage(lpMsg: *const MSG) -> i32;
    pub fn DispatchMessageW(lpMsg: *const MSG) -> isize;
    pub fn DefWindowProcW(hWnd: HWND, Msg: u32, wParam: WPARAM, lParam: LPARAM) -> isize;
    pub fn BeginPaint(hWnd: HWND, lpPaint: *mut PAINTSTRUCT) -> HDC;
    pub fn EndPaint(hWnd: HWND, lpPaint: *const PAINTSTRUCT) -> BOOL;
    pub fn PostQuitMessage(nExitCode: i32);
    pub fn InvalidateRect(hWnd: HWND, lpRect: *const RECT, bErase: BOOL) -> BOOL;
    pub fn MessageBoxW(
        hWnd: HWND,
        lpText: *const w_char,
        lpCaption: *const w_char,
        uType: u32,
    ) -> i32;
    pub fn GetWindowLongW(hwnd: HWND, index: i32) -> i32;
    pub fn GetWindowLongPtrW(hwnd: HWND, index: i32) -> isize;
    pub fn SetWindowLongW(hwnd: HWND, index: i32, value: i32) -> i32;
    pub fn SetWindowLongPtrW(hwnd: HWND, index: i32, value: isize) -> isize;
    pub fn MonitorFromPoint(pt: POINT, dwFlags: u32) -> *mut c_void;
    pub fn EnumDisplayMonitors(
        hdc: HDC,
        lprcClip: *const RECT,
        lpfnEnum: unsafe extern "system" fn(
            hMonitor: HMONITOR,
            hdcMonitor: HDC,
            lprcMonitor: *const RECT,
            dwData: LPARAM,
        ) -> BOOL,
        dwData: LPARAM,
    ) -> BOOL;
    pub fn PostMessageW(hWnd: HWND, Msg: u32, wParam: WPARAM, lParam: LPARAM) -> BOOL;
    pub fn RegisterWindowMessageW(lpString: *const w_char) -> u32;
    pub fn GetMonitorInfoW(hMonitor: HMONITOR, lpmi: *mut MONITORINFOEXW) -> BOOL;
    pub fn GetDpiForSystem() -> u32;
    pub fn GetDpiForWindow(hWnd: HWND) -> u32;
    pub fn GetSystemMetrics(nIndex: i32) -> i32;
    pub fn AdjustWindowRectExForDpi(
        lpRect: *mut RECT,
        dwStyle: u32,
        bMenu: BOOL,
        dwExStyle: u32,
        dpi: u32,
    ) -> BOOL;
    pub fn DestroyWindow(hWnd: HWND) -> BOOL;
    pub fn GetWindowPlacement(hWnd: HWND, lpwndpl: *mut WINDOWPLACEMENT) -> BOOL;
    pub fn SetWindowPlacement(hWnd: HWND, lpwndpl: *const WINDOWPLACEMENT) -> BOOL;
    pub fn FindWindowW(lpClassName: *const w_char, lpWindowName: *const w_char) -> HWND;
    pub fn SetForegroundWindow(hWnd: HWND) -> BOOL;
}

/// # Safety
/// `hwnd` must be a live window handle; `index` must identify valid window data.
pub unsafe fn GetWindowLong(hwnd: HWND, index: i32) -> isize {
    cfg_select! {
        target_pointer_width = "32" => (unsafe { GetWindowLongW(hwnd, index) }) as isize,
        _ => unsafe { GetWindowLongPtrW(hwnd, index) },
    }
}

/// # Safety
/// The handle, index, and value must satisfy the Win32 SetWindowLongPtrW contract.
/// Any stored pointer or window procedure must remain valid while installed.
pub unsafe fn SetWindowLong(hwnd: HWND, index: i32, value: isize) -> isize {
    cfg_select! {
        target_pointer_width = "32" => (unsafe { SetWindowLongW(hwnd, index, value as i32) }) as isize,
        _ => unsafe { SetWindowLongPtrW(hwnd, index, value) },
    }
}

// MARK: shell32.dll
pub type HDROP = HANDLE;

#[link(name = "shell32")]
unsafe extern "system" {
    pub fn DragAcceptFiles(hwnd: HWND, accept: BOOL);
    pub fn DragFinish(drop: HDROP);
    pub fn DragQueryFileW(drop: HDROP, file: u32, buffer: *mut w_char, buffer_size: u32) -> u32;
    pub fn ShellExecuteW(
        hwnd: HWND,
        lpOperation: *const w_char,
        lpFile: *const w_char,
        lpParameters: *const w_char,
        lpDirectory: *const w_char,
        nShowCmd: i32,
    ) -> isize;
    pub fn SHCreateItemFromParsingName(
        pszPath: *const w_char,
        pbc: *mut c_void,
        riid: *const GUID,
        ppv: *mut *mut c_void,
    ) -> HRESULT;
}

// MARK: shcore.dll
#[link(name = "shcore")]
unsafe extern "system" {
    pub fn SetProcessDpiAwareness(value: i32) -> HRESULT;
    pub fn GetDpiForMonitor(
        hmonitor: HMONITOR,
        dpiType: i32,
        dpiX: *mut u32,
        dpiY: *mut u32,
    ) -> HRESULT;
}

// MARK: dwmapi.dll
pub const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;

#[link(name = "dwmapi")]
unsafe extern "system" {
    pub fn DwmSetWindowAttribute(
        hwnd: HWND,
        dwAttribute: u32,
        pvAttribute: *const c_void,
        cbAttribute: u32,
    ) -> i32;
}

// MARK: ole32.dll
pub type HRESULT = i32;

#[repr(C)]
#[derive(PartialEq, Eq)]
pub struct GUID {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

#[repr(C)]
pub struct STATSTG {
    pub pwcsName: *mut w_char,
    pub type_: u32,
    pub cbSize: u64,
    pub mtime: FILETIME,
    pub ctime: FILETIME,
    pub atime: FILETIME,
    pub grfMode: u32,
    pub grfLocksSupported: u32,
    pub clsid: GUID,
    pub grfStateBits: u32,
    pub reserved: u32,
}

pub const S_OK: HRESULT = 0;
pub const E_NOINTERFACE: HRESULT = 0x80004002u32 as HRESULT;
pub const E_NOTIMPL: HRESULT = 0x80004001u32 as HRESULT;
pub const E_POINTER: HRESULT = 0x80004003u32 as HRESULT;
pub const DRAGDROP_E_INVALIDHWND: HRESULT = 0x80040102u32 as HRESULT;
pub const CF_HDROP: u16 = 15;
pub const DVASPECT_CONTENT: u32 = 1;
pub const TYMED_HGLOBAL: u32 = 1;
pub const DROPEFFECT_NONE: u32 = 0;
pub const DROPEFFECT_COPY: u32 = 1;

#[repr(C)]
pub struct FORMATETC {
    pub cfFormat: u16,
    pub ptd: *mut c_void,
    pub dwAspect: u32,
    pub lindex: i32,
    pub tymed: u32,
}

#[repr(C)]
pub struct STGMEDIUM {
    pub tymed: u32,
    pub data: *mut c_void,
    pub pUnkForRelease: *mut c_void,
}

#[repr(C)]
pub struct IDataObject {
    pub lpVtbl: *const IDataObjectVtbl,
}

#[repr(C)]
pub struct IDataObjectVtbl {
    pub QueryInterface:
        unsafe extern "system" fn(*mut IDataObject, *const GUID, *mut *mut c_void) -> HRESULT,
    pub AddRef: unsafe extern "system" fn(*mut IDataObject) -> u32,
    pub Release: unsafe extern "system" fn(*mut IDataObject) -> u32,
    pub GetData:
        unsafe extern "system" fn(*mut IDataObject, *mut FORMATETC, *mut STGMEDIUM) -> HRESULT,
}

#[repr(C)]
pub struct IDropTarget {
    pub lpVtbl: *const IDropTargetVtbl,
}

#[repr(C)]
pub struct IDropTargetVtbl {
    pub QueryInterface:
        unsafe extern "system" fn(*mut IDropTarget, *const GUID, *mut *mut c_void) -> HRESULT,
    pub AddRef: unsafe extern "system" fn(*mut IDropTarget) -> u32,
    pub Release: unsafe extern "system" fn(*mut IDropTarget) -> u32,
    pub DragEnter: unsafe extern "system" fn(
        *mut IDropTarget,
        *mut IDataObject,
        u32,
        POINT,
        *mut u32,
    ) -> HRESULT,
    pub DragOver: unsafe extern "system" fn(*mut IDropTarget, u32, POINT, *mut u32) -> HRESULT,
    pub DragLeave: unsafe extern "system" fn(*mut IDropTarget) -> HRESULT,
    pub Drop: unsafe extern "system" fn(
        *mut IDropTarget,
        *mut IDataObject,
        u32,
        POINT,
        *mut u32,
    ) -> HRESULT,
}

#[link(name = "ole32")]
unsafe extern "system" {
    pub fn OleInitialize(pvReserved: *mut c_void) -> HRESULT;
    pub fn OleUninitialize();
    pub fn RegisterDragDrop(hwnd: HWND, drop_target: *mut IDropTarget) -> HRESULT;
    pub fn RevokeDragDrop(hwnd: HWND) -> HRESULT;
    pub fn ReleaseStgMedium(medium: *mut STGMEDIUM);
    pub fn CoTaskMemFree(pv: *mut c_void);
    pub fn CoCreateInstance(
        rclsid: *const GUID,
        pUnkOuter: *mut c_void,
        dwClsContext: u32,
        riid: *const GUID,
        ppv: *mut *mut c_void,
    ) -> HRESULT;
}

// MARK: IStream
pub const STATFLAG_NONAME: u32 = 1;

#[repr(C)]
pub struct IStream {
    pub lpVtbl: *const IStream_Vtbl,
}

impl IStream {
    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// The caller must own a reference and must not use it after releasing it.
    pub unsafe fn Release(&self) -> u32 {
        unsafe { ((*self.lpVtbl).Release)(self as *const _ as *mut _) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native Read contract.
    pub unsafe fn Read(&self, pv: *mut c_void, cb: u32, pcbRead: *mut u32) -> HRESULT {
        unsafe { ((*self.lpVtbl).Read)(self as *const _ as *mut _, pv, cb, pcbRead) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native Stat contract.
    pub unsafe fn Stat(&self, pstatstg: *mut STATSTG, grfStatFlag: u32) -> HRESULT {
        unsafe { ((*self.lpVtbl).Stat)(self as *const _ as *mut _, pstatstg, grfStatFlag) }
    }
}

#[repr(C)]
pub struct IStream_Vtbl {
    pub QueryInterface: unsafe extern "system" fn(
        This: *mut IStream,
        riid: *const GUID,
        ppvObject: *mut *mut c_void,
    ) -> HRESULT,
    pub AddRef: unsafe extern "system" fn(This: *mut IStream) -> u32,
    pub Release: unsafe extern "system" fn(This: *mut IStream) -> u32,
    pub Read: unsafe extern "system" fn(
        This: *mut IStream,
        pv: *mut c_void,
        cb: u32,
        pcbRead: *mut u32,
    ) -> HRESULT,
    padding1: [usize; 8],
    pub Stat: unsafe extern "system" fn(
        This: *mut IStream,
        pstatstg: *mut STATSTG,
        grfStatFlag: u32,
    ) -> HRESULT,
}

// shlwapi.dll
#[link(name = "shlwapi")]
unsafe extern "system" {
    pub fn SHCreateMemStream(pInit: *const u8, cbInit: u32) -> *mut IStream;
}

// MARK: Utils
pub const fn wide_ascii<const N: usize>(value: &str) -> [w_char; N] {
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
#[doc(hidden)]
#[macro_export]
macro_rules! wide {
    ($value:literal) => {{
        const WIDE: &[w_char] = &$crate::ffi::wide_ascii::<{ $value.len() + 1 }>($value);
        WIDE
    }};
}
pub use wide;

pub trait ToWideString {
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

pub struct LPWSTR(*mut w_char);
impl Default for LPWSTR {
    fn default() -> Self {
        Self(std::ptr::null_mut())
    }
}
impl LPWSTR {
    pub const fn as_ptr(&self) -> *const w_char {
        self.0
    }

    pub const fn as_mut_ptr(&mut self) -> *mut *mut w_char {
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

// MARK: Common Item Dialog
pub const FOS_OVERWRITEPROMPT: u32 = 0x00000002;
pub const FOS_NOCHANGEDIR: u32 = 0x00000008;
pub const FOS_ALLOWMULTISELECT: u32 = 0x00000200;
pub const FOS_PATHMUSTEXIST: u32 = 0x00000800;
pub const FOS_FILEMUSTEXIST: u32 = 0x00001000;
pub const SIGDN_FILESYSPATH: i32 = 0x80058000u32 as i32;
pub const CLSCTX_INPROC_SERVER: u32 = 1;

pub const CLSID_FileOpenDialog: GUID = GUID {
    data1: 0xDC1C5A9C,
    data2: 0xE88A,
    data3: 0x4dde,
    data4: [0xA5, 0xA1, 0x60, 0xF8, 0x2A, 0x20, 0xAE, 0xF7],
};
pub const CLSID_FileSaveDialog: GUID = GUID {
    data1: 0xC0B4E2F3,
    data2: 0xBA21,
    data3: 0x4773,
    data4: [0x8D, 0xBA, 0x33, 0x5E, 0xC9, 0x46, 0xEB, 0x8B],
};
pub const IID_IFileOpenDialog: GUID = GUID {
    data1: 0xD57C7288,
    data2: 0xD4AD,
    data3: 0x4768,
    data4: [0xBE, 0x02, 0x9D, 0x96, 0x95, 0x32, 0xD9, 0x60],
};
pub const IID_IFileSaveDialog: GUID = GUID {
    data1: 0x84BCCD23,
    data2: 0x5FDE,
    data3: 0x4CDB,
    data4: [0xAE, 0xA4, 0xAF, 0x64, 0xB8, 0x3D, 0x78, 0xAB],
};
pub const IID_IShellItem: GUID = GUID {
    data1: 0x43826D1E,
    data2: 0xE718,
    data3: 0x42EE,
    data4: [0xBC, 0x55, 0xA1, 0xE2, 0x61, 0xC3, 0x7B, 0xFE],
};

#[repr(C)]
pub struct COMDLG_FILTERSPEC {
    pub pszName: *const u16,
    pub pszSpec: *const u16,
}

// IShellItem
#[repr(C)]
pub struct IShellItem {
    pub vtbl: *const IShellItemVtbl,
}

impl IShellItem {
    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// The caller must own a reference and must not use it after releasing it.
    pub unsafe fn Release(&self) -> u32 {
        unsafe { ((*self.vtbl).Release)(self as *const _ as *mut _) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native GetDisplayName contract.
    pub unsafe fn GetDisplayName(&self, sigdn: i32, ppszName: *mut *mut u16) -> HRESULT {
        unsafe { ((*self.vtbl).GetDisplayName)(self as *const _ as *mut _, sigdn, ppszName) }
    }
}

#[repr(C)]
pub struct IShellItemVtbl {
    _unk: [usize; 2], // QueryInterface, AddRef
    pub Release: unsafe extern "system" fn(*mut c_void) -> u32,
    _bind_parent: [usize; 2], // BindToHandler, GetParent
    pub GetDisplayName: unsafe extern "system" fn(*mut c_void, i32, *mut *mut u16) -> HRESULT,
    // GetAttributes, Compare
}

// IShellItemArray
#[repr(C)]
pub struct IShellItemArray {
    pub vtbl: *const IShellItemArrayVtbl,
}

impl IShellItemArray {
    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// The caller must own a reference and must not use it after releasing it.
    pub unsafe fn Release(&self) -> u32 {
        unsafe { ((*self.vtbl).Release)(self as *const _ as *mut _) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native GetCount contract.
    pub unsafe fn GetCount(&self, pdwNumItems: *mut u32) -> HRESULT {
        unsafe { ((*self.vtbl).GetCount)(self as *const _ as *mut _, pdwNumItems) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native GetItemAt contract.
    pub unsafe fn GetItemAt(&self, dwIndex: u32, ppsi: *mut *mut IShellItem) -> HRESULT {
        unsafe { ((*self.vtbl).GetItemAt)(self as *const _ as *mut _, dwIndex, ppsi) }
    }
}

#[repr(C)]
pub struct IShellItemArrayVtbl {
    _unk: [usize; 2], // QueryInterface, AddRef
    pub Release: unsafe extern "system" fn(*mut c_void) -> u32,
    _mid: [usize; 4], // BindToHandler, GetPropertyStore, GetPropertyDescriptionList, GetAttributes
    pub GetCount: unsafe extern "system" fn(*mut c_void, *mut u32) -> HRESULT,
    pub GetItemAt: unsafe extern "system" fn(*mut c_void, u32, *mut *mut IShellItem) -> HRESULT,
    // EnumItems
}

// IFileOpenDialog
#[repr(C)]
pub struct IFileOpenDialog {
    pub vtbl: *const IFileOpenDialogVtbl,
}

impl IFileOpenDialog {
    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// The caller must own a reference and must not use it after releasing it.
    pub unsafe fn Release(&self) -> u32 {
        unsafe { ((*self.vtbl).Release)(self as *const _ as *mut _) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native Show contract.
    pub unsafe fn Show(&self, hwnd: HWND) -> HRESULT {
        unsafe { ((*self.vtbl).Show)(self as *const _ as *mut _, hwnd) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native SetFileTypes contract.
    pub unsafe fn SetFileTypes(
        &self,
        cFileTypes: u32,
        rgFilterSpec: *const COMDLG_FILTERSPEC,
    ) -> HRESULT {
        unsafe { ((*self.vtbl).SetFileTypes)(self as *const _ as *mut _, cFileTypes, rgFilterSpec) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native SetOptions contract.
    pub unsafe fn SetOptions(&self, fos: u32) -> HRESULT {
        unsafe { ((*self.vtbl).SetOptions)(self as *const _ as *mut _, fos) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native SetFolder contract.
    pub unsafe fn SetFolder(&self, psi: *mut IShellItem) -> HRESULT {
        unsafe { ((*self.vtbl).SetFolder)(self as *const _ as *mut _, psi) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native SetTitle contract.
    pub unsafe fn SetTitle(&self, pszTitle: *const u16) -> HRESULT {
        unsafe { ((*self.vtbl).SetTitle)(self as *const _ as *mut _, pszTitle) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native GetResult contract.
    pub unsafe fn GetResult(&self, ppsi: *mut *mut IShellItem) -> HRESULT {
        unsafe { ((*self.vtbl).GetResult)(self as *const _ as *mut _, ppsi) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native GetResults contract.
    pub unsafe fn GetResults(&self, ppsia: *mut *mut IShellItemArray) -> HRESULT {
        unsafe { ((*self.vtbl).GetResults)(self as *const _ as *mut _, ppsia) }
    }
}

#[repr(C)]
pub struct IFileOpenDialogVtbl {
    _unk: [usize; 2], // QueryInterface, AddRef
    pub Release: unsafe extern "system" fn(*mut c_void) -> u32,
    // IModalWindow
    pub Show: unsafe extern "system" fn(*mut c_void, HWND) -> HRESULT,
    // IFileDialog
    pub SetFileTypes:
        unsafe extern "system" fn(*mut c_void, u32, *const COMDLG_FILTERSPEC) -> HRESULT,
    _type_idx: [usize; 4], // SetFileTypeIndex, GetFileTypeIndex, Advise, Unadvise
    pub SetOptions: unsafe extern "system" fn(*mut c_void, u32) -> HRESULT,
    _get_opts_def: [usize; 2], // GetOptions, SetDefaultFolder
    pub SetFolder: unsafe extern "system" fn(*mut c_void, *mut IShellItem) -> HRESULT,
    _get_folder_sel: [usize; 2], // GetFolder, GetCurrentSelection
    _set_filename: usize,        // SetFileName
    _get_filename: usize,        // GetFileName
    pub SetTitle: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
    _labels: [usize; 2], // SetOkButtonLabel, SetFileNameLabel
    pub GetResult: unsafe extern "system" fn(*mut c_void, *mut *mut IShellItem) -> HRESULT,
    _add_place: usize,      // AddPlace
    _def_ext: usize,        // SetDefaultExtension
    _close_etc: [usize; 4], // Close, SetClientGuid, ClearClientData, SetFilter
    // IFileOpenDialog
    pub GetResults: unsafe extern "system" fn(*mut c_void, *mut *mut IShellItemArray) -> HRESULT,
    // GetSelectedItems
}

// IFileSaveDialog
#[repr(C)]
pub struct IFileSaveDialog {
    pub vtbl: *const IFileSaveDialogVtbl,
}

impl IFileSaveDialog {
    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// The caller must own a reference and must not use it after releasing it.
    pub unsafe fn Release(&self) -> u32 {
        unsafe { ((*self.vtbl).Release)(self as *const _ as *mut _) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native Show contract.
    pub unsafe fn Show(&self, hwnd: HWND) -> HRESULT {
        unsafe { ((*self.vtbl).Show)(self as *const _ as *mut _, hwnd) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native SetFileTypes contract.
    pub unsafe fn SetFileTypes(
        &self,
        cFileTypes: u32,
        rgFilterSpec: *const COMDLG_FILTERSPEC,
    ) -> HRESULT {
        unsafe { ((*self.vtbl).SetFileTypes)(self as *const _ as *mut _, cFileTypes, rgFilterSpec) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native SetOptions contract.
    pub unsafe fn SetOptions(&self, fos: u32) -> HRESULT {
        unsafe { ((*self.vtbl).SetOptions)(self as *const _ as *mut _, fos) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native SetFolder contract.
    pub unsafe fn SetFolder(&self, psi: *mut IShellItem) -> HRESULT {
        unsafe { ((*self.vtbl).SetFolder)(self as *const _ as *mut _, psi) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native SetFileName contract.
    pub unsafe fn SetFileName(&self, pszName: *const u16) -> HRESULT {
        unsafe { ((*self.vtbl).SetFileName)(self as *const _ as *mut _, pszName) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native SetTitle contract.
    pub unsafe fn SetTitle(&self, pszTitle: *const u16) -> HRESULT {
        unsafe { ((*self.vtbl).SetTitle)(self as *const _ as *mut _, pszTitle) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native SetDefaultExtension contract.
    pub unsafe fn SetDefaultExtension(&self, pszDefaultExtension: *const u16) -> HRESULT {
        unsafe {
            ((*self.vtbl).SetDefaultExtension)(self as *const _ as *mut _, pszDefaultExtension)
        }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native GetResult contract.
    pub unsafe fn GetResult(&self, ppsi: *mut *mut IShellItem) -> HRESULT {
        unsafe { ((*self.vtbl).GetResult)(self as *const _ as *mut _, ppsi) }
    }
}

#[repr(C)]
pub struct IFileSaveDialogVtbl {
    _unk: [usize; 2], // QueryInterface, AddRef
    pub Release: unsafe extern "system" fn(*mut c_void) -> u32,
    // IModalWindow
    pub Show: unsafe extern "system" fn(*mut c_void, HWND) -> HRESULT,
    // IFileDialog
    pub SetFileTypes:
        unsafe extern "system" fn(*mut c_void, u32, *const COMDLG_FILTERSPEC) -> HRESULT,
    _type_idx: [usize; 4], // SetFileTypeIndex, GetFileTypeIndex, Advise, Unadvise
    pub SetOptions: unsafe extern "system" fn(*mut c_void, u32) -> HRESULT,
    _get_opts_def: [usize; 2], // GetOptions, SetDefaultFolder
    pub SetFolder: unsafe extern "system" fn(*mut c_void, *mut IShellItem) -> HRESULT,
    _get_folder_sel: [usize; 2], // GetFolder, GetCurrentSelection
    pub SetFileName: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
    _get_filename: usize, // GetFileName
    pub SetTitle: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
    _labels: [usize; 2], // SetOkButtonLabel, SetFileNameLabel
    pub GetResult: unsafe extern "system" fn(*mut c_void, *mut *mut IShellItem) -> HRESULT,
    _add_place: usize, // AddPlace
    pub SetDefaultExtension: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
    // Close, SetClientGuid, ClearClientData, SetFilter, then IFileSaveDialog methods
}

// MARK: ITaskbarList3
pub const CLSID_TASKBAR_LIST: GUID = GUID {
    data1: 0x56FDF344,
    data2: 0xFD6D,
    data3: 0x11D0,
    data4: [0x95, 0x8A, 0x00, 0x60, 0x97, 0xC9, 0xA0, 0x90],
};
pub const IID_TASKBAR_LIST3: GUID = GUID {
    data1: 0xEA1AFB91,
    data2: 0x9E28,
    data3: 0x4B86,
    data4: [0x90, 0xE9, 0x9E, 0x9F, 0x8A, 0x5E, 0xEF, 0xAF],
};

#[repr(C)]
pub struct TaskbarList3 {
    pub vtable: *const TaskbarList3Vtable,
}

#[repr(C)]
pub struct TaskbarList3Vtable {
    query_interface: usize,
    add_ref: usize,
    pub release: unsafe extern "system" fn(*mut TaskbarList3) -> u32,
    pub hr_init: unsafe extern "system" fn(*mut TaskbarList3) -> HRESULT,
    add_tab: usize,
    delete_tab: usize,
    activate_tab: usize,
    set_active_alt: usize,
    mark_fullscreen_window: usize,
    pub set_progress_value: unsafe extern "system" fn(*mut TaskbarList3, HWND, u64, u64) -> HRESULT,
    pub set_progress_state: unsafe extern "system" fn(*mut TaskbarList3, HWND, i32) -> HRESULT,
}
