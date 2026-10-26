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
pub(crate) type BOOL = i32;
pub(crate) const TRUE: BOOL = 1;
pub(crate) const FALSE: BOOL = 0;
pub(crate) type w_char = u16;
pub(crate) type HANDLE = *mut c_void;
pub(crate) type HKEY = HANDLE;
pub(crate) const HKEY_CURRENT_USER: HKEY = 0x80000001usize as HKEY;
pub(crate) const RRF_RT_REG_DWORD: u32 = 0x00000018;
pub(crate) const ERROR_SUCCESS: i32 = 0;

// MARK: kernel32.dll
pub(crate) type HMODULE = *const c_void;

pub(crate) const ERROR_ALREADY_EXISTS: u32 = 183;
pub(crate) const WAIT_TIMEOUT: u32 = 258;
pub(crate) const WAIT_ABANDONED: u32 = 128;
pub(crate) const PAGE_READWRITE: u32 = 4;
pub(crate) const FILE_MAP_READ: u32 = 4;
pub(crate) const FILE_MAP_WRITE: u32 = 2;
pub(crate) const SYNCHRONIZE: u32 = 0x0010_0000;
pub(crate) const ERROR_INSUFFICIENT_BUFFER: i32 = 122;
pub(crate) const ERROR_INVALID_PARAMETER: i32 = 87;
pub(crate) const APPMODEL_ERROR_NO_PACKAGE: i32 = 15700;

#[link(name = "kernel32")]
unsafe extern "system" {
    pub(crate) fn GetCurrentThreadId() -> u32;
    pub(crate) fn ReleaseMutex(handle: HANDLE) -> i32;
    pub(crate) fn CreateEventW(
        attributes: *const c_void,
        manual: i32,
        initial: i32,
        name: *const u16,
    ) -> HANDLE;
    pub(crate) fn SetEvent(event: HANDLE) -> i32;
    pub(crate) fn ResetEvent(event: HANDLE) -> i32;
    pub(crate) fn WaitForSingleObject(handle: HANDLE, milliseconds: u32) -> u32;
    pub(crate) fn WaitForMultipleObjects(
        count: u32,
        handles: *const HANDLE,
        all: i32,
        milliseconds: u32,
    ) -> u32;
    pub(crate) fn GetCurrentProcessId() -> u32;
    pub(crate) fn OpenProcess(access: u32, inherit: i32, pid: u32) -> HANDLE;
    pub(crate) fn CreateFileMappingW(
        file: HANDLE,
        attributes: *const c_void,
        protection: u32,
        high: u32,
        low: u32,
        name: *const u16,
    ) -> HANDLE;
    pub(crate) fn MapViewOfFile(
        mapping: HANDLE,
        access: u32,
        high: u32,
        low: u32,
        length: usize,
    ) -> *mut c_void;
    pub(crate) fn UnmapViewOfFile(address: *const c_void) -> i32;
    pub(crate) fn GetCurrentPackageFamilyName(length: *mut u32, name: *mut u16) -> i32;
    pub(crate) fn GetModuleHandleW(lpModuleName: *const w_char) -> HMODULE;
    pub(crate) fn GetProcAddress(hModule: HMODULE, lpProcName: *const c_char) -> *const c_void;
    pub(crate) fn GetLastError() -> u32;
    pub(crate) fn CreateMutexW(
        lpMutexAttributes: *mut c_void,
        bInitialOwner: BOOL,
        lpName: *const w_char,
    ) -> HANDLE;
}

// MARK: advapi32.dll
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

// MARK: gdi32.dll
pub(crate) type HBRUSH = HANDLE;
pub(crate) type HDC = HANDLE;

#[link(name = "gdi32")]
unsafe extern "system" {
    pub(crate) fn FillRect(hdc: HDC, lprc: *const RECT, hbr: HBRUSH) -> i32;
    pub(crate) fn CreateSolidBrush(color: u32) -> HBRUSH;
    pub(crate) fn DeleteObject(ho: *mut c_void) -> BOOL;
}

// MARK: user32.dll
pub(crate) type HWND = HANDLE;
pub(crate) type HCURSOR = HANDLE;
pub(crate) type HICON = HANDLE;
pub(crate) type HMENU = HANDLE;
pub(crate) type HMONITOR = HANDLE;
pub(crate) type ATOM = u16;
pub(crate) type WPARAM = usize;
pub(crate) type LPARAM = isize;
pub(crate) type LRESULT = isize;

pub(crate) const IDOK: i32 = 1;
pub(crate) const IDCANCEL: i32 = 2;
pub(crate) const IDYES: i32 = 6;
pub(crate) const IDNO: i32 = 7;

#[repr(C)]
#[derive(Default)]
pub(crate) struct WNDCLASSEXW {
    pub(crate) cbSize: u32,
    pub(crate) style: u32,
    pub(crate) lpfnWndProc: Option<
        unsafe extern "system" fn(
            window: HWND,
            message: u32,
            wparam: WPARAM,
            lparam: LPARAM,
        ) -> LRESULT,
    >,
    pub(crate) cbClsExtra: i32,
    pub(crate) cbWndExtra: i32,
    pub(crate) hInstance: HMODULE,
    pub(crate) hIcon: HICON,
    pub(crate) hCursor: HCURSOR,
    pub(crate) hbrBackground: usize,
    pub(crate) lpszMenuName: *const w_char,
    pub(crate) lpszClassName: *const w_char,
    pub(crate) hIconSm: HICON,
}

// MARK: comctl32.dll
pub(crate) const TDF_ALLOW_DIALOG_CANCELLATION: u32 = 0x0008;
pub(crate) const TDCBF_OK_BUTTON: u32 = 0x0001;
pub(crate) const TDCBF_YES_BUTTON: u32 = 0x0002;
pub(crate) const TDCBF_NO_BUTTON: u32 = 0x0004;
pub(crate) const TDCBF_CANCEL_BUTTON: u32 = 0x0008;
pub(crate) const TD_WARNING_ICON: *const w_char = u16::MAX as usize as *const w_char;
pub(crate) const TD_ERROR_ICON: *const w_char = (u16::MAX - 1) as usize as *const w_char;
pub(crate) const TD_INFORMATION_ICON: *const w_char = (u16::MAX - 2) as usize as *const w_char;

#[repr(C)]
pub(crate) struct TASKDIALOG_BUTTON {
    pub(crate) nButtonID: i32,
    pub(crate) pszButtonText: *const w_char,
}

pub(crate) type TaskDialogCallback =
    Option<unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM, isize) -> i32>;

#[repr(C)]
pub(crate) struct TASKDIALOGCONFIG {
    pub(crate) cbSize: u32,
    pub(crate) hwndParent: HWND,
    pub(crate) hInstance: HMODULE,
    pub(crate) dwFlags: u32,
    pub(crate) dwCommonButtons: u32,
    pub(crate) pszWindowTitle: *const w_char,
    pub(crate) mainIcon: *const w_char,
    pub(crate) pszMainInstruction: *const w_char,
    pub(crate) pszContent: *const w_char,
    pub(crate) cButtons: u32,
    pub(crate) pButtons: *const TASKDIALOG_BUTTON,
    pub(crate) nDefaultButton: i32,
    pub(crate) cRadioButtons: u32,
    pub(crate) pRadioButtons: *const TASKDIALOG_BUTTON,
    pub(crate) nDefaultRadioButton: i32,
    pub(crate) pszVerificationText: *const w_char,
    pub(crate) pszExpandedInformation: *const w_char,
    pub(crate) pszExpandedControlText: *const w_char,
    pub(crate) pszCollapsedControlText: *const w_char,
    pub(crate) footerIcon: *const w_char,
    pub(crate) pszFooter: *const w_char,
    pub(crate) pfCallback: TaskDialogCallback,
    pub(crate) lpCallbackData: isize,
    pub(crate) cxWidth: u32,
}

#[link(name = "comctl32")]
unsafe extern "system" {
    pub(crate) fn TaskDialogIndirect(
        pTaskConfig: *const TASKDIALOGCONFIG,
        pnButton: *mut i32,
        pnRadioButton: *mut i32,
        pfVerificationFlagChecked: *mut BOOL,
    ) -> i32;
}

#[repr(C)]
pub(crate) struct CREATESTRUCTW {
    pub(crate) lpCreateParams: *mut c_void,
    pub(crate) hInstance: HMODULE,
    pub(crate) hMenu: HMENU,
    pub(crate) hwndParent: HWND,
    pub(crate) cy: i32,
    pub(crate) cx: i32,
    pub(crate) y: i32,
    pub(crate) x: i32,
    pub(crate) style: i32,
    pub(crate) lpszName: *const w_char,
    pub(crate) lpszClass: *const w_char,
    pub(crate) dwExStyle: u32,
}

#[repr(C)]
pub(crate) struct MSG {
    pub(crate) hwnd: HWND,
    pub(crate) message: u32,
    pub(crate) wParam: WPARAM,
    pub(crate) lParam: LPARAM,
    pub(crate) time: u32,
    pub(crate) pt_x: i32,
    pub(crate) pt_y: i32,
}

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

#[repr(C)]
#[derive(Default)]
pub(crate) struct PAINTSTRUCT {
    pub(crate) hdc: HDC,
    pub(crate) fErase: BOOL,
    pub(crate) rcPaint: RECT,
    pub(crate) fRestore: BOOL,
    pub(crate) fIncUpdate: BOOL,
    pub(crate) rgbReserved: [u8; 32],
}

const _: () = assert!(size_of::<PAINTSTRUCT>() == if size_of::<HWND>() == 8 { 72 } else { 64 });

#[repr(C)]
pub(crate) struct MINMAXINFO {
    pub(crate) ptReserved: POINT,
    pub(crate) ptMaxSize: POINT,
    pub(crate) ptMaxPosition: POINT,
    pub(crate) ptMinTrackSize: POINT,
    pub(crate) ptMaxTrackSize: POINT,
}

#[repr(C)]
#[derive(Default)]
pub(crate) struct MONITORINFOEXW {
    pub(crate) cbSize: u32,
    pub(crate) rcMonitor: RECT,
    pub(crate) rcWork: RECT,
    pub(crate) dwFlags: u32,
    pub(crate) szDevice: [w_char; 32],
}

#[repr(C)]
pub(crate) struct WINDOWPLACEMENT {
    pub(crate) length: u32,
    pub(crate) flags: u32,
    pub(crate) showCmd: u32,
    pub(crate) ptMinPosition: POINT,
    pub(crate) ptMaxPosition: POINT,
    pub(crate) rcNormalPosition: RECT,
}

pub(crate) const WS_POPUP: u32 = 0x80000000;
pub(crate) const WS_THICKFRAME: u32 = 0x00040000;
pub(crate) const WS_OVERLAPPEDWINDOW: u32 = 0x00CF0000;
pub(crate) const WS_MAXIMIZEBOX: u32 = 0x00010000;

pub(crate) const CW_USEDEFAULT: i32 = 0x80000000u32 as i32;

pub(crate) const WM_CREATE: u32 = 0x0001;
pub(crate) const WM_DESTROY: u32 = 0x0002;
pub(crate) const WM_MOVE: u32 = 0x0003;
pub(crate) const WM_SIZE: u32 = 0x0005;
pub(crate) const WM_PAINT: u32 = 0x000f;
pub(crate) const WM_CLOSE: u32 = 0x0010;
pub(crate) const WM_ERASEBKGND: u32 = 0x0014;
pub(crate) const WM_GETMINMAXINFO: u32 = 0x0024;
pub(crate) const WM_NCCREATE: u32 = 0x0081;
pub(crate) const WM_DROPFILES: u32 = 0x0233;
pub(crate) const WM_DPICHANGED: u32 = 0x02E0;
pub(crate) const WM_USER: u32 = 0x0400;

pub(crate) const SW_RESTORE: i32 = 9;
pub(crate) const SW_SHOWDEFAULT: i32 = 10;

pub(crate) const MB_OK: u32 = 0x00000000;
pub(crate) const MB_OKCANCEL: u32 = 0x00000001;
pub(crate) const MB_YESNOCANCEL: u32 = 0x00000003;
pub(crate) const MB_YESNO: u32 = 0x00000004;
pub(crate) const MB_ICONERROR: u32 = 0x00000010;
pub(crate) const MB_ICONWARNING: u32 = 0x00000030;
pub(crate) const MB_ICONINFORMATION: u32 = 0x00000040;

pub(crate) const PROCESS_PER_MONITOR_DPI_AWARE: i32 = 2;
pub(crate) const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2: isize = -4isize;

pub(crate) const MONITOR_DEFAULTTOPRIMARY: u32 = 0x00000001;

pub(crate) const USER_DEFAULT_SCREEN_DPI: u32 = 96;
pub(crate) const MDT_EFFECTIVE_DPI: i32 = 0;

pub(crate) const SM_CXSCREEN: i32 = 0;
pub(crate) const SM_CYSCREEN: i32 = 1;
pub(crate) const COLOR_WINDOW: i32 = 5;

pub(crate) const GWL_STYLE: i32 = -16;
pub(crate) const GWL_USERDATA: i32 = -21;

pub(crate) const SWP_NOSIZE: u32 = 0x0001;
pub(crate) const SWP_NOMOVE: u32 = 0x0002;
pub(crate) const SWP_NOZORDER: u32 = 0x0004;
pub(crate) const SWP_NOACTIVATE: u32 = 0x0010;
pub(crate) const SWP_NOREPOSITION: u32 = 0x0200;

#[repr(C)]
pub(crate) struct ACCEL {
    pub(crate) fVirt: u8,
    pub(crate) key: u16,
    pub(crate) cmd: u16,
}
#[link(name = "user32")]
unsafe extern "system" {
    pub(crate) fn EnumThreadWindows(
        thread: u32,
        callback: unsafe extern "system" fn(HWND, LPARAM) -> BOOL,
        data: LPARAM,
    ) -> BOOL;
    pub(crate) fn IsWindowVisible(window: HWND) -> BOOL;
    pub(crate) fn IsIconic(window: HWND) -> BOOL;
    pub(crate) fn AllowSetForegroundWindow(pid: u32) -> i32;
    pub(crate) fn CreateAcceleratorTableW(accels: *const ACCEL, count: i32) -> HANDLE;
    pub(crate) fn DestroyAcceleratorTable(table: HANDLE) -> BOOL;
    pub(crate) fn TranslateAcceleratorW(window: HWND, table: HANDLE, message: *mut MSG) -> i32;
    pub(crate) fn VkKeyScanW(character: u16) -> i16;
    pub(crate) fn GetKeyState(key: i32) -> i16;
    pub(crate) fn GetKeyboardState(state: *mut u8) -> i32;
    pub(crate) fn GetKeyboardLayout(thread: u32) -> isize;
    pub(crate) fn ToUnicodeEx(
        key: u32,
        scan: u32,
        state: *const u8,
        text: *mut u16,
        count: i32,
        flags: u32,
        layout: isize,
    ) -> i32;
    pub(crate) fn GetSysColorBrush(nIndex: i32) -> HBRUSH;
    pub(crate) fn GetActiveWindow() -> HWND;
    pub(crate) fn ExtractIconExW(
        lpszFile: *const w_char,
        nIconIndex: i32,
        phiconLarge: *mut HICON,
        phiconSmall: *mut HICON,
        nIcons: u32,
    ) -> u32;
    pub(crate) fn GetClassInfoExW(
        hInstance: HMODULE,
        lpClassName: *const w_char,
        lpWndClass: *mut WNDCLASSEXW,
    ) -> BOOL;
    pub(crate) fn RegisterClassExW(lpWndClass: *const WNDCLASSEXW) -> ATOM;
    pub(crate) fn CreateWindowExW(
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
    pub(crate) fn SetWindowTextW(hWnd: HWND, lpString: *const w_char) -> i32;
    pub(crate) fn ShowWindow(hWnd: HWND, nCmdShow: i32) -> i32;
    pub(crate) fn UpdateWindow(hWnd: HWND) -> i32;
    pub(crate) fn GetWindowRect(hWnd: HWND, rect: *mut RECT) -> i32;
    pub(crate) fn GetClientRect(hWnd: HWND, rect: *mut RECT) -> i32;
    pub(crate) fn SetWindowPos(
        hWnd: HWND,
        hWndInsertAfter: HWND,
        X: i32,
        Y: i32,
        cx: i32,
        cy: i32,
        uFlags: u32,
    ) -> BOOL;
    pub(crate) fn GetMessageW(
        lpMsg: *mut MSG,
        hWnd: HWND,
        wMsgFilterMin: u32,
        wMsgFilterMax: u32,
    ) -> i32;
    pub(crate) fn TranslateMessage(lpMsg: *const MSG) -> i32;
    pub(crate) fn DispatchMessageW(lpMsg: *const MSG) -> isize;
    pub(crate) fn DefWindowProcW(hWnd: HWND, Msg: u32, wParam: WPARAM, lParam: LPARAM) -> isize;
    pub(crate) fn BeginPaint(hWnd: HWND, lpPaint: *mut PAINTSTRUCT) -> HDC;
    pub(crate) fn EndPaint(hWnd: HWND, lpPaint: *const PAINTSTRUCT) -> BOOL;
    pub(crate) fn PostQuitMessage(nExitCode: i32);
    pub(crate) fn InvalidateRect(hWnd: HWND, lpRect: *const RECT, bErase: BOOL) -> BOOL;
    pub(crate) fn MessageBoxW(
        hWnd: HWND,
        lpText: *const w_char,
        lpCaption: *const w_char,
        uType: u32,
    ) -> i32;
    pub(crate) fn GetWindowLongW(hwnd: HWND, index: i32) -> i32;
    pub(crate) fn GetWindowLongPtrW(hwnd: HWND, index: i32) -> isize;
    pub(crate) fn SetWindowLongW(hwnd: HWND, index: i32, value: i32) -> i32;
    pub(crate) fn SetWindowLongPtrW(hwnd: HWND, index: i32, value: isize) -> isize;
    pub(crate) fn MonitorFromPoint(pt: POINT, dwFlags: u32) -> *mut c_void;
    pub(crate) fn EnumDisplayMonitors(
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
    pub(crate) fn PostMessageW(hWnd: HWND, Msg: u32, wParam: WPARAM, lParam: LPARAM) -> BOOL;
    pub(crate) fn RegisterWindowMessageW(lpString: *const w_char) -> u32;
    pub(crate) fn GetMonitorInfoW(hMonitor: HMONITOR, lpmi: *mut MONITORINFOEXW) -> BOOL;
    pub(crate) fn GetDpiForSystem() -> u32;
    pub(crate) fn GetDpiForWindow(hWnd: HWND) -> u32;
    pub(crate) fn GetSystemMetrics(nIndex: i32) -> i32;
    pub(crate) fn AdjustWindowRectExForDpi(
        lpRect: *mut RECT,
        dwStyle: u32,
        bMenu: BOOL,
        dwExStyle: u32,
        dpi: u32,
    ) -> BOOL;
    pub(crate) fn DestroyWindow(hWnd: HWND) -> BOOL;
    pub(crate) fn GetWindowPlacement(hWnd: HWND, lpwndpl: *mut WINDOWPLACEMENT) -> BOOL;
    pub(crate) fn SetWindowPlacement(hWnd: HWND, lpwndpl: *const WINDOWPLACEMENT) -> BOOL;
    pub(crate) fn FindWindowW(lpClassName: *const w_char, lpWindowName: *const w_char) -> HWND;
    pub(crate) fn SetForegroundWindow(hWnd: HWND) -> BOOL;
}

/// # Safety
/// `hwnd` must be a live window handle; `index` must identify valid window data.
pub(crate) unsafe fn GetWindowLong(hwnd: HWND, index: i32) -> isize {
    cfg_select! {
        target_pointer_width = "32" => (unsafe { GetWindowLongW(hwnd, index) }) as isize,
        _ => unsafe { GetWindowLongPtrW(hwnd, index) },
    }
}

/// # Safety
/// The handle, index, and value must satisfy the Win32 SetWindowLongPtrW contract.
/// Any stored pointer or window procedure must remain valid while installed.
pub(crate) unsafe fn SetWindowLong(hwnd: HWND, index: i32, value: isize) -> isize {
    cfg_select! {
        target_pointer_width = "32" => (unsafe { SetWindowLongW(hwnd, index, value as i32) }) as isize,
        _ => unsafe { SetWindowLongPtrW(hwnd, index, value) },
    }
}

// MARK: shell32.dll
pub(crate) type HDROP = HANDLE;

#[link(name = "shell32")]
unsafe extern "system" {
    pub(crate) fn DragAcceptFiles(hwnd: HWND, accept: BOOL);
    pub(crate) fn DragFinish(drop: HDROP);
    pub(crate) fn DragQueryFileW(
        drop: HDROP,
        file: u32,
        buffer: *mut w_char,
        buffer_size: u32,
    ) -> u32;
    pub(crate) fn SHCreateItemFromParsingName(
        pszPath: *const w_char,
        pbc: *mut c_void,
        riid: *const GUID,
        ppv: *mut *mut c_void,
    ) -> HRESULT;
}

// MARK: shcore.dll
#[link(name = "shcore")]
unsafe extern "system" {
    pub(crate) fn SetProcessDpiAwareness(value: i32) -> HRESULT;
    pub(crate) fn GetDpiForMonitor(
        hmonitor: HMONITOR,
        dpiType: i32,
        dpiX: *mut u32,
        dpiY: *mut u32,
    ) -> HRESULT;
}

// MARK: dwmapi.dll
pub(crate) const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;

#[link(name = "dwmapi")]
unsafe extern "system" {
    pub(crate) fn DwmSetWindowAttribute(
        hwnd: HWND,
        dwAttribute: u32,
        pvAttribute: *const c_void,
        cbAttribute: u32,
    ) -> i32;
}

// MARK: ole32.dll
pub(crate) type HRESULT = i32;

#[repr(C)]
#[derive(PartialEq, Eq)]
pub(crate) struct GUID {
    pub(crate) data1: u32,
    pub(crate) data2: u16,
    pub(crate) data3: u16,
    pub(crate) data4: [u8; 8],
}

pub(crate) const S_OK: HRESULT = 0;

#[link(name = "ole32")]
unsafe extern "system" {
    pub(crate) fn OleInitialize(pvReserved: *mut c_void) -> HRESULT;
    pub(crate) fn OleUninitialize();
    pub(crate) fn CoTaskMemFree(pv: *mut c_void);
    pub(crate) fn CoCreateInstance(
        rclsid: *const GUID,
        pUnkOuter: *mut c_void,
        dwClsContext: u32,
        riid: *const GUID,
        ppv: *mut *mut c_void,
    ) -> HRESULT;
}

// MARK: IStream

// shlwapi.dll
#[link(name = "shlwapi")]
unsafe extern "system" {}

// MARK: Utils
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

// MARK: Common Item Dialog
pub(crate) const FOS_OVERWRITEPROMPT: u32 = 0x00000002;
pub(crate) const FOS_NOCHANGEDIR: u32 = 0x00000008;
pub(crate) const FOS_ALLOWMULTISELECT: u32 = 0x00000200;
pub(crate) const FOS_PATHMUSTEXIST: u32 = 0x00000800;
pub(crate) const FOS_FILEMUSTEXIST: u32 = 0x00001000;
pub(crate) const SIGDN_FILESYSPATH: i32 = 0x80058000u32 as i32;
pub(crate) const CLSCTX_INPROC_SERVER: u32 = 1;

pub(crate) const CLSID_FileOpenDialog: GUID = GUID {
    data1: 0xDC1C5A9C,
    data2: 0xE88A,
    data3: 0x4dde,
    data4: [0xA5, 0xA1, 0x60, 0xF8, 0x2A, 0x20, 0xAE, 0xF7],
};
pub(crate) const CLSID_FileSaveDialog: GUID = GUID {
    data1: 0xC0B4E2F3,
    data2: 0xBA21,
    data3: 0x4773,
    data4: [0x8D, 0xBA, 0x33, 0x5E, 0xC9, 0x46, 0xEB, 0x8B],
};
pub(crate) const IID_IFileOpenDialog: GUID = GUID {
    data1: 0xD57C7288,
    data2: 0xD4AD,
    data3: 0x4768,
    data4: [0xBE, 0x02, 0x9D, 0x96, 0x95, 0x32, 0xD9, 0x60],
};
pub(crate) const IID_IFileSaveDialog: GUID = GUID {
    data1: 0x84BCCD23,
    data2: 0x5FDE,
    data3: 0x4CDB,
    data4: [0xAE, 0xA4, 0xAF, 0x64, 0xB8, 0x3D, 0x78, 0xAB],
};
pub(crate) const IID_IShellItem: GUID = GUID {
    data1: 0x43826D1E,
    data2: 0xE718,
    data3: 0x42EE,
    data4: [0xBC, 0x55, 0xA1, 0xE2, 0x61, 0xC3, 0x7B, 0xFE],
};

#[repr(C)]
pub(crate) struct COMDLG_FILTERSPEC {
    pub(crate) pszName: *const u16,
    pub(crate) pszSpec: *const u16,
}

// IShellItem
#[repr(C)]
pub(crate) struct IShellItem {
    pub(crate) vtbl: *const IShellItemVtbl,
}

impl IShellItem {
    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// The caller must own a reference and must not use it after releasing it.
    pub(crate) unsafe fn Release(&self) -> u32 {
        unsafe { ((*self.vtbl).Release)(self as *const _ as *mut _) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native GetDisplayName contract.
    pub(crate) unsafe fn GetDisplayName(&self, sigdn: i32, ppszName: *mut *mut u16) -> HRESULT {
        unsafe { ((*self.vtbl).GetDisplayName)(self as *const _ as *mut _, sigdn, ppszName) }
    }
}

#[repr(C)]
pub(crate) struct IShellItemVtbl {
    _unk: [usize; 2], // QueryInterface, AddRef
    pub(crate) Release: unsafe extern "system" fn(*mut c_void) -> u32,
    _bind_parent: [usize; 2], // BindToHandler, GetParent
    pub(crate) GetDisplayName:
        unsafe extern "system" fn(*mut c_void, i32, *mut *mut u16) -> HRESULT,
    // GetAttributes, Compare
}

// IShellItemArray
#[repr(C)]
pub(crate) struct IShellItemArray {
    pub(crate) vtbl: *const IShellItemArrayVtbl,
}

impl IShellItemArray {
    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// The caller must own a reference and must not use it after releasing it.
    pub(crate) unsafe fn Release(&self) -> u32 {
        unsafe { ((*self.vtbl).Release)(self as *const _ as *mut _) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native GetCount contract.
    pub(crate) unsafe fn GetCount(&self, pdwNumItems: *mut u32) -> HRESULT {
        unsafe { ((*self.vtbl).GetCount)(self as *const _ as *mut _, pdwNumItems) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native GetItemAt contract.
    pub(crate) unsafe fn GetItemAt(&self, dwIndex: u32, ppsi: *mut *mut IShellItem) -> HRESULT {
        unsafe { ((*self.vtbl).GetItemAt)(self as *const _ as *mut _, dwIndex, ppsi) }
    }
}

#[repr(C)]
pub(crate) struct IShellItemArrayVtbl {
    _unk: [usize; 2], // QueryInterface, AddRef
    pub(crate) Release: unsafe extern "system" fn(*mut c_void) -> u32,
    _mid: [usize; 4], // BindToHandler, GetPropertyStore, GetPropertyDescriptionList, GetAttributes
    pub(crate) GetCount: unsafe extern "system" fn(*mut c_void, *mut u32) -> HRESULT,
    pub(crate) GetItemAt:
        unsafe extern "system" fn(*mut c_void, u32, *mut *mut IShellItem) -> HRESULT,
    // EnumItems
}

// IFileOpenDialog
#[repr(C)]
pub(crate) struct IFileOpenDialog {
    pub(crate) vtbl: *const IFileOpenDialogVtbl,
}

impl IFileOpenDialog {
    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// The caller must own a reference and must not use it after releasing it.
    pub(crate) unsafe fn Release(&self) -> u32 {
        unsafe { ((*self.vtbl).Release)(self as *const _ as *mut _) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native Show contract.
    pub(crate) unsafe fn Show(&self, hwnd: HWND) -> HRESULT {
        unsafe { ((*self.vtbl).Show)(self as *const _ as *mut _, hwnd) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native SetFileTypes contract.
    pub(crate) unsafe fn SetFileTypes(
        &self,
        cFileTypes: u32,
        rgFilterSpec: *const COMDLG_FILTERSPEC,
    ) -> HRESULT {
        unsafe { ((*self.vtbl).SetFileTypes)(self as *const _ as *mut _, cFileTypes, rgFilterSpec) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native SetOptions contract.
    pub(crate) unsafe fn SetOptions(&self, fos: u32) -> HRESULT {
        unsafe { ((*self.vtbl).SetOptions)(self as *const _ as *mut _, fos) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native SetFolder contract.
    pub(crate) unsafe fn SetFolder(&self, psi: *mut IShellItem) -> HRESULT {
        unsafe { ((*self.vtbl).SetFolder)(self as *const _ as *mut _, psi) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native SetTitle contract.
    pub(crate) unsafe fn SetTitle(&self, pszTitle: *const u16) -> HRESULT {
        unsafe { ((*self.vtbl).SetTitle)(self as *const _ as *mut _, pszTitle) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native GetResult contract.
    pub(crate) unsafe fn GetResult(&self, ppsi: *mut *mut IShellItem) -> HRESULT {
        unsafe { ((*self.vtbl).GetResult)(self as *const _ as *mut _, ppsi) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native GetResults contract.
    pub(crate) unsafe fn GetResults(&self, ppsia: *mut *mut IShellItemArray) -> HRESULT {
        unsafe { ((*self.vtbl).GetResults)(self as *const _ as *mut _, ppsia) }
    }
}

#[repr(C)]
pub(crate) struct IFileOpenDialogVtbl {
    _unk: [usize; 2], // QueryInterface, AddRef
    pub(crate) Release: unsafe extern "system" fn(*mut c_void) -> u32,
    // IModalWindow
    pub(crate) Show: unsafe extern "system" fn(*mut c_void, HWND) -> HRESULT,
    // IFileDialog
    pub(crate) SetFileTypes:
        unsafe extern "system" fn(*mut c_void, u32, *const COMDLG_FILTERSPEC) -> HRESULT,
    _type_idx: [usize; 4], // SetFileTypeIndex, GetFileTypeIndex, Advise, Unadvise
    pub(crate) SetOptions: unsafe extern "system" fn(*mut c_void, u32) -> HRESULT,
    _get_opts_def: [usize; 2], // GetOptions, SetDefaultFolder
    pub(crate) SetFolder: unsafe extern "system" fn(*mut c_void, *mut IShellItem) -> HRESULT,
    _get_folder_sel: [usize; 2], // GetFolder, GetCurrentSelection
    _set_filename: usize,        // SetFileName
    _get_filename: usize,        // GetFileName
    pub(crate) SetTitle: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
    _labels: [usize; 2], // SetOkButtonLabel, SetFileNameLabel
    pub(crate) GetResult: unsafe extern "system" fn(*mut c_void, *mut *mut IShellItem) -> HRESULT,
    _add_place: usize,      // AddPlace
    _def_ext: usize,        // SetDefaultExtension
    _close_etc: [usize; 4], // Close, SetClientGuid, ClearClientData, SetFilter
    // IFileOpenDialog
    pub(crate) GetResults:
        unsafe extern "system" fn(*mut c_void, *mut *mut IShellItemArray) -> HRESULT,
    // GetSelectedItems
}

// IFileSaveDialog
#[repr(C)]
pub(crate) struct IFileSaveDialog {
    pub(crate) vtbl: *const IFileSaveDialogVtbl,
}

impl IFileSaveDialog {
    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// The caller must own a reference and must not use it after releasing it.
    pub(crate) unsafe fn Release(&self) -> u32 {
        unsafe { ((*self.vtbl).Release)(self as *const _ as *mut _) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native Show contract.
    pub(crate) unsafe fn Show(&self, hwnd: HWND) -> HRESULT {
        unsafe { ((*self.vtbl).Show)(self as *const _ as *mut _, hwnd) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native SetFileTypes contract.
    pub(crate) unsafe fn SetFileTypes(
        &self,
        cFileTypes: u32,
        rgFilterSpec: *const COMDLG_FILTERSPEC,
    ) -> HRESULT {
        unsafe { ((*self.vtbl).SetFileTypes)(self as *const _ as *mut _, cFileTypes, rgFilterSpec) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native SetOptions contract.
    pub(crate) unsafe fn SetOptions(&self, fos: u32) -> HRESULT {
        unsafe { ((*self.vtbl).SetOptions)(self as *const _ as *mut _, fos) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native SetFolder contract.
    pub(crate) unsafe fn SetFolder(&self, psi: *mut IShellItem) -> HRESULT {
        unsafe { ((*self.vtbl).SetFolder)(self as *const _ as *mut _, psi) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native SetFileName contract.
    pub(crate) unsafe fn SetFileName(&self, pszName: *const u16) -> HRESULT {
        unsafe { ((*self.vtbl).SetFileName)(self as *const _ as *mut _, pszName) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native SetTitle contract.
    pub(crate) unsafe fn SetTitle(&self, pszTitle: *const u16) -> HRESULT {
        unsafe { ((*self.vtbl).SetTitle)(self as *const _ as *mut _, pszTitle) }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native SetDefaultExtension contract.
    pub(crate) unsafe fn SetDefaultExtension(&self, pszDefaultExtension: *const u16) -> HRESULT {
        unsafe {
            ((*self.vtbl).SetDefaultExtension)(self as *const _ as *mut _, pszDefaultExtension)
        }
    }

    /// # Safety
    /// The interface must be live and usable on the current COM apartment.
    /// Pointer arguments and buffer lengths must satisfy the native GetResult contract.
    pub(crate) unsafe fn GetResult(&self, ppsi: *mut *mut IShellItem) -> HRESULT {
        unsafe { ((*self.vtbl).GetResult)(self as *const _ as *mut _, ppsi) }
    }
}

#[repr(C)]
pub(crate) struct IFileSaveDialogVtbl {
    _unk: [usize; 2], // QueryInterface, AddRef
    pub(crate) Release: unsafe extern "system" fn(*mut c_void) -> u32,
    // IModalWindow
    pub(crate) Show: unsafe extern "system" fn(*mut c_void, HWND) -> HRESULT,
    // IFileDialog
    pub(crate) SetFileTypes:
        unsafe extern "system" fn(*mut c_void, u32, *const COMDLG_FILTERSPEC) -> HRESULT,
    _type_idx: [usize; 4], // SetFileTypeIndex, GetFileTypeIndex, Advise, Unadvise
    pub(crate) SetOptions: unsafe extern "system" fn(*mut c_void, u32) -> HRESULT,
    _get_opts_def: [usize; 2], // GetOptions, SetDefaultFolder
    pub(crate) SetFolder: unsafe extern "system" fn(*mut c_void, *mut IShellItem) -> HRESULT,
    _get_folder_sel: [usize; 2], // GetFolder, GetCurrentSelection
    pub(crate) SetFileName: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
    _get_filename: usize, // GetFileName
    pub(crate) SetTitle: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
    _labels: [usize; 2], // SetOkButtonLabel, SetFileNameLabel
    pub(crate) GetResult: unsafe extern "system" fn(*mut c_void, *mut *mut IShellItem) -> HRESULT,
    _add_place: usize, // AddPlace
    pub(crate) SetDefaultExtension: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
    // Close, SetClientGuid, ClearClientData, SetFilter, then IFileSaveDialog methods
}

// MARK: ITaskbarList3
pub(crate) const CLSID_TASKBAR_LIST: GUID = GUID {
    data1: 0x56FDF344,
    data2: 0xFD6D,
    data3: 0x11D0,
    data4: [0x95, 0x8A, 0x00, 0x60, 0x97, 0xC9, 0xA0, 0x90],
};
pub(crate) const IID_TASKBAR_LIST3: GUID = GUID {
    data1: 0xEA1AFB91,
    data2: 0x9E28,
    data3: 0x4B86,
    data4: [0x90, 0xE9, 0x9E, 0x9F, 0x8A, 0x5E, 0xEF, 0xAF],
};

#[repr(C)]
pub(crate) struct TaskbarList3 {
    pub(crate) vtable: *const TaskbarList3Vtable,
}

#[repr(C)]
pub(crate) struct TaskbarList3Vtable {
    query_interface: usize,
    add_ref: usize,
    pub(crate) release: unsafe extern "system" fn(*mut TaskbarList3) -> u32,
    pub(crate) hr_init: unsafe extern "system" fn(*mut TaskbarList3) -> HRESULT,
    add_tab: usize,
    delete_tab: usize,
    activate_tab: usize,
    set_active_alt: usize,
    mark_fullscreen_window: usize,
    pub(crate) set_progress_value:
        unsafe extern "system" fn(*mut TaskbarList3, HWND, u64, u64) -> HRESULT,
    pub(crate) set_progress_state:
        unsafe extern "system" fn(*mut TaskbarList3, HWND, i32) -> HRESULT,
}
