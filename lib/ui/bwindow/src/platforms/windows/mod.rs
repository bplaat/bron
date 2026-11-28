/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#[cfg(feature = "dialog")]
pub(crate) use crate::platforms::windows::dialog::{PlatformFileDialog, PlatformMessageDialog};
pub(crate) use crate::platforms::windows::event_loop::{
    PlatformEventLoop, PlatformEventLoopProxy, PlatformMonitor, open_url,
};
pub(crate) use crate::platforms::windows::frame::FrameSource;
pub(crate) use crate::platforms::windows::interaction::release_pointer_lock;
pub(crate) use crate::platforms::windows::window::{
    PlatformWindow, config_dir as windows_config_dir, embed_native_view, resize_native_view,
};

/// Imports an API available since the given baseline (7 or 10). Targets below that baseline
/// load it once at runtime and return `None` when it is unavailable.
macro_rules! optional_proc {
    (7, $($function:tt)*) => {
        optional_proc!(@resolve target_vendor = "winxp"; $($function)*)
    };
    (10, $($function:tt)*) => {
        optional_proc!(@resolve any(target_vendor = "winxp", target_vendor = "win7"); $($function)*)
    };
    (@resolve $dynamic:meta; $module:literal, $name:ident($($arg:ty),* $(,)?) -> $ret:ty) => {{
        type Function = unsafe extern "system" fn($($arg),*) -> $ret;
        #[cfg(not($dynamic))]
        let function: Option<Function> = {
            #[link(name = $module)]
            unsafe extern "system" {
                fn $name($(_: $arg),*) -> $ret;
            }
            Some($name)
        };
        #[cfg($dynamic)]
        let function: Option<Function> = {
            static FUNCTION: std::sync::OnceLock<Option<Function>> = std::sync::OnceLock::new();
            *FUNCTION.get_or_init(|| {
                let address = $crate::platforms::windows::load_system_proc(
                    $crate::platforms::windows::headers::w!(concat!($module, ".dll")),
                    concat!(stringify!($name), "\0"),
                );
                (!address.is_null()).then(|| unsafe {
                    std::mem::transmute::<*const std::ffi::c_void, Function>(address)
                })
            })
        };
        function
    }};
}

mod com;
#[cfg(feature = "dialog")]
mod dialog;
mod event_loop;
#[cfg(feature = "file_drop")]
mod file_drop;
mod frame;
pub(crate) mod headers;
pub(crate) mod input;
mod instance;
mod interaction;
#[cfg(feature = "menu")]
mod menu;
#[cfg(feature = "progress_bar")]
mod progress_bar;
mod window;
#[cfg(feature = "remember_window_state")]
mod window_state;

use std::ffi::c_void;

use self::headers::*;

/// Loads a function of a DLL in the system directory, the DLL stays loaded
#[cfg(any(target_vendor = "winxp", target_vendor = "win7"))]
fn load_system_proc(module: &[u16], symbol: &str) -> *const c_void {
    // Version 6 of comctl32 lives in WinSxS: only a load by bare name goes through the manifest's
    // activation context, a full path gets the old version 5.82 in the system directory
    let mut path;
    let name = if module == w!("comctl32.dll") {
        module
    } else {
        const MAX_PATH: usize = 260;
        path = vec![0; MAX_PATH];
        let length = unsafe { GetSystemDirectoryW(path.as_mut_ptr(), path.len() as u32) } as usize;
        if length == 0 || length >= path.len() {
            return std::ptr::null();
        }
        path.truncate(length);
        path.push(u16::from(b'\\'));
        path.extend_from_slice(module);
        &path
    };
    let module = unsafe { LoadLibraryW(name.as_ptr()) };
    if module.is_null() {
        return std::ptr::null();
    }
    unsafe { GetProcAddress(module, symbol.as_ptr().cast()) }
}

pub(super) unsafe fn package_family_name(length: *mut u32, name: *mut u16) -> i32 {
    match optional_proc!(
        10,
        "kernel32",
        GetCurrentPackageFamilyName(*mut u32, *mut u16) -> i32
    ) {
        Some(function) => unsafe { function(length, name) },
        None => APPMODEL_ERROR_NO_PACKAGE,
    }
}

/// Reads a registry value of exactly the given type and size into `data`, with RegGetValueW and
/// on Windows XP the open key API, whose type and size checks replace the RRF_RT flags
pub(super) fn registry_value(
    key: HKEY,
    sub_key: &[u16],
    value: &[u16],
    kind: u32,
    data: &mut [u8],
) -> bool {
    let (mut actual_kind, mut size) = (0, data.len() as u32);
    let status = if let Some(get_value) = optional_proc!(
        7,
        "advapi32",
        RegGetValueW(HKEY, *const u16, *const u16, u32, *mut u32, *mut c_void, *mut u32) -> i32
    ) {
        let flags = match kind {
            REG_BINARY => RRF_RT_REG_BINARY,
            REG_DWORD => RRF_RT_REG_DWORD,
            _ => unreachable!("unsupported registry value type"),
        };
        unsafe {
            get_value(
                key,
                sub_key.as_ptr(),
                value.as_ptr(),
                flags,
                &mut actual_kind,
                data.as_mut_ptr().cast(),
                &mut size,
            )
        }
    } else {
        let mut opened = std::ptr::null_mut();
        if unsafe { RegOpenKeyExW(key, sub_key.as_ptr(), 0, KEY_QUERY_VALUE, &mut opened) }
            != ERROR_SUCCESS
        {
            return false;
        }
        let status = unsafe {
            RegQueryValueExW(
                opened,
                value.as_ptr(),
                std::ptr::null_mut(),
                &mut actual_kind,
                data.as_mut_ptr().cast(),
                &mut size,
            )
        };
        unsafe { RegCloseKey(opened) };
        status
    };
    status == ERROR_SUCCESS && actual_kind == kind && size as usize == data.len()
}

/// Writes a registry value, creating its key when needed, with RegSetKeyValueW and on Windows XP
/// the open key API
#[cfg(feature = "remember_window_state")]
pub(super) fn set_registry_value(
    key: HKEY,
    sub_key: &[u16],
    value: &[u16],
    kind: u32,
    data: &[u8],
) -> bool {
    if let Some(set_value) = optional_proc!(
        7,
        "advapi32",
        RegSetKeyValueW(HKEY, *const u16, *const u16, u32, *const c_void, u32) -> i32
    ) {
        return unsafe {
            set_value(
                key,
                sub_key.as_ptr(),
                value.as_ptr(),
                kind,
                data.as_ptr().cast(),
                data.len() as u32,
            )
        } == ERROR_SUCCESS;
    }
    let mut opened = std::ptr::null_mut();
    if unsafe {
        RegCreateKeyExW(
            key,
            sub_key.as_ptr(),
            0,
            std::ptr::null(),
            0,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut opened,
            std::ptr::null_mut(),
        )
    } != ERROR_SUCCESS
    {
        return false;
    }
    let status = unsafe {
        RegSetValueExW(
            opened,
            value.as_ptr(),
            0,
            kind,
            data.as_ptr().cast(),
            data.len() as u32,
        )
    };
    unsafe { RegCloseKey(opened) };
    status == ERROR_SUCCESS
}

// Windows XP has no DPI awareness, it never scales windows
pub(super) unsafe fn set_process_dpi_aware() {
    if let Some(function) = optional_proc!(7, "user32", SetProcessDPIAware() -> BOOL) {
        unsafe { function() };
    }
}

// Windows XP has no desktop composition, so no cloaking and no dark title bar
pub(super) unsafe fn dwm_set_window_attribute(
    hwnd: HWND,
    attribute: u32,
    value: *const c_void,
    size: u32,
) -> HRESULT {
    match optional_proc!(
        7,
        "dwmapi",
        DwmSetWindowAttribute(HWND, u32, *const c_void, u32) -> HRESULT
    ) {
        Some(function) => unsafe { function(hwnd, attribute, value, size) },
        None => 0x8000_4001u32 as i32, // E_NOTIMPL
    }
}

pub(super) unsafe fn system_dpi() -> u32 {
    if let Some(function) = optional_proc!(10, "user32", GetDpiForSystem() -> u32) {
        return unsafe { function() };
    }
    unsafe { legacy_system_dpi() }
}

unsafe fn legacy_system_dpi() -> u32 {
    let dc = unsafe { GetDC(std::ptr::null_mut()) };
    if dc.is_null() {
        return USER_DEFAULT_SCREEN_DPI;
    }
    let dpi = unsafe { GetDeviceCaps(dc, 88) }; // LOGPIXELSX
    unsafe { ReleaseDC(std::ptr::null_mut(), dc) };
    dpi.max(1) as u32
}

pub(super) unsafe fn window_dpi(hwnd: HWND) -> u32 {
    match optional_proc!(10, "user32", GetDpiForWindow(HWND) -> u32) {
        Some(function) => unsafe { function(hwnd) },
        None => unsafe { system_dpi() },
    }
}

pub(super) unsafe fn adjust_window_rect_for_dpi(
    rect: *mut RECT,
    style: u32,
    menu: BOOL,
    ex_style: u32,
    dpi: u32,
) -> BOOL {
    match optional_proc!(
        10,
        "user32",
        AdjustWindowRectExForDpi(*mut RECT, u32, BOOL, u32, u32) -> BOOL
    ) {
        Some(function) => unsafe { function(rect, style, menu, ex_style, dpi) },
        // Windows 7 is system-DPI-aware: the native frame metrics already use system DPI.
        None => unsafe { AdjustWindowRectEx(rect, style, menu, ex_style) },
    }
}

pub(super) unsafe fn set_process_dpi_awareness(value: i32) -> HRESULT {
    match optional_proc!(10, "shcore", SetProcessDpiAwareness(i32) -> HRESULT) {
        Some(function) => unsafe { function(value) },
        None => 0x8000_4001u32 as i32, // E_NOTIMPL
    }
}

pub(super) unsafe fn monitor_dpi(
    monitor: HMONITOR,
    kind: i32,
    x: *mut u32,
    y: *mut u32,
) -> HRESULT {
    match optional_proc!(
        10,
        "shcore",
        GetDpiForMonitor(HMONITOR, i32, *mut u32, *mut u32) -> HRESULT
    ) {
        Some(function) => unsafe { function(monitor, kind, x, y) },
        None => {
            let dpi = unsafe { system_dpi() };
            unsafe {
                *x = dpi;
                *y = dpi;
            }
            S_OK
        }
    }
}
