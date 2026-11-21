/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The parts of the Win32 API the programs use that std doesn't cover, with a few safe helpers
//! they share. Structure layouts are checked against the mingw-w64 headers.

#![allow(non_snake_case, missing_docs, clippy::upper_case_acronyms)]

use core::ffi::c_void;
use core::mem::{offset_of, size_of};
use core::ptr;
use std::string::String;
use std::vec::Vec;

pub type BOOL = i32;
pub type HANDLE = *mut c_void;
pub type HWND = *mut c_void;
pub type HKEY = *mut c_void;
pub type HDC = *mut c_void;
pub type LRESULT = isize;
pub type WPARAM = usize;
pub type LPARAM = isize;
pub type WNDPROC = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

pub const ERROR_SUCCESS: i32 = 0;
pub const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
pub const ERROR_ALREADY_EXISTS: u32 = 183;

// Windows and messages
pub const WS_POPUP: u32 = 0x8000_0000;
pub const WM_DESTROY: u32 = 0x0002;
pub const WM_SETTINGCHANGE: u32 = 0x001A;
pub const WM_TIMER: u32 = 0x0113;
pub const WM_DRAWCLIPBOARD: u32 = 0x0308;
pub const WM_CHANGECBCHAIN: u32 = 0x030D;
pub const WM_APP: u32 = 0x8000;
pub const LVM_ARRANGE: u32 = 0x1016;
pub const LVA_DEFAULT: usize = 0;
pub const HWND_BROADCAST: HWND = 0xFFFF as HWND;
pub const SMTO_ABORTIFHUNG: u32 = 2;
pub const MB_ICONWARNING: u32 = 0x30;
pub const MB_SYSTEMMODAL: u32 = 0x1000;
pub const EWX_REBOOT: u32 = 0x2;
pub const EWX_FORCE: u32 = 0x4;
pub const EWX_POWEROFF: u32 = 0x8;

#[repr(C)]
pub struct WNDCLASSW {
    pub style: u32,
    pub lpfnWndProc: Option<WNDPROC>,
    pub cbClsExtra: i32,
    pub cbWndExtra: i32,
    pub hInstance: HANDLE,
    pub hIcon: HANDLE,
    pub hCursor: HANDLE,
    pub hbrBackground: HANDLE,
    pub lpszMenuName: *const u16,
    pub lpszClassName: *const u16,
}
const _: () = assert!(size_of::<WNDCLASSW>() == 40);

#[repr(C)]
pub struct MSG {
    pub hwnd: HWND,
    pub message: u32,
    pub wParam: WPARAM,
    pub lParam: LPARAM,
    pub time: u32,
    pub pt: [i32; 2],
}
const _: () = assert!(size_of::<MSG>() == 28);

// Display
pub const BITSPIXEL: i32 = 12;
pub const SM_CXSCREEN: i32 = 0;
pub const SM_CYSCREEN: i32 = 1;
pub const DM_BITSPERPEL: u32 = 0x0004_0000;
pub const DM_PELSWIDTH: u32 = 0x0008_0000;
pub const DM_PELSHEIGHT: u32 = 0x0010_0000;
pub const DM_DISPLAYFREQUENCY: u32 = 0x0040_0000;
pub const CDS_UPDATEREGISTRY: u32 = 1;
pub const DISP_CHANGE_SUCCESSFUL: i32 = 0;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DEVMODEW {
    _head: [u16; 34],
    pub dmSize: u16,
    pub dmDriverExtra: u16,
    pub dmFields: u32,
    _middle: [u8; 92],
    pub dmBitsPerPel: u32,
    pub dmPelsWidth: u32,
    pub dmPelsHeight: u32,
    pub dmDisplayFlags: u32,
    pub dmDisplayFrequency: u32,
    _tail: [u32; 8],
}
const _: () = assert!(size_of::<DEVMODEW>() == 220);
const _: () = assert!(offset_of!(DEVMODEW, dmSize) == 68);
const _: () = assert!(offset_of!(DEVMODEW, dmBitsPerPel) == 168);
const _: () = assert!(offset_of!(DEVMODEW, dmDisplayFrequency) == 184);

impl Default for DEVMODEW {
    fn default() -> Self {
        // SAFETY: All zeroes is a valid DEVMODEW
        let mut mode: Self = unsafe { core::mem::zeroed() };
        mode.dmSize = size_of::<Self>() as u16;
        mode
    }
}

// Clipboard and GDI
pub const CF_DIB: u32 = 8;
pub const CF_UNICODETEXT: u32 = 13;
pub const GMEM_MOVEABLE: u32 = 2;
pub const BI_RGB: u32 = 0;
pub const BI_RLE8: u32 = 1;
pub const BI_RLE4: u32 = 2;
pub const BI_BITFIELDS: u32 = 3;
pub const DIB_RGB_COLORS: u32 = 0;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct BITMAPINFOHEADER {
    pub biSize: u32,
    pub biWidth: i32,
    pub biHeight: i32,
    pub biPlanes: u16,
    pub biBitCount: u16,
    pub biCompression: u32,
    pub biSizeImage: u32,
    pub biXPelsPerMeter: i32,
    pub biYPelsPerMeter: i32,
    pub biClrUsed: u32,
    pub biClrImportant: u32,
}
const _: () = assert!(size_of::<BITMAPINFOHEADER>() == 40);

// Clock
pub const TIME_ZONE_ID_INVALID: u32 = u32::MAX;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TIME_ZONE_INFORMATION {
    pub Bias: i32,
    pub StandardName: [u16; 32],
    pub StandardDate: [u16; 8],
    pub StandardBias: i32,
    pub DaylightName: [u16; 32],
    pub DaylightDate: [u16; 8],
    pub DaylightBias: i32,
}
const _: () = assert!(size_of::<TIME_ZONE_INFORMATION>() == 172);
const _: () = assert!(offset_of!(TIME_ZONE_INFORMATION, DaylightBias) == 168);

// Security, registry and services
pub const TOKEN_ADJUST_PRIVILEGES: u32 = 0x20;
pub const SE_PRIVILEGE_ENABLED: u32 = 2;
const HKEY_USERS: HKEY = 0x8000_0003_u32 as HKEY;
const HKEY_CURRENT_USER: HKEY = 0x8000_0001_u32 as HKEY;
const HKEY_LOCAL_MACHINE: HKEY = 0x8000_0002_u32 as HKEY;
pub const KEY_QUERY_VALUE: u32 = 0x1;
pub const KEY_SET_VALUE: u32 = 0x2;
pub const KEY_READ: u32 = 0x20019;
pub const KEY_ALL_ACCESS: u32 = 0xF003F;
pub const REG_SZ: u32 = 1;
pub const REG_EXPAND_SZ: u32 = 2;
pub const REG_BINARY: u32 = 3;
pub const REG_DWORD: u32 = 4;
pub const REG_MULTI_SZ: u32 = 7;
pub const SC_MANAGER_CREATE_SERVICE: u32 = 2;
pub const SERVICE_ALL_ACCESS: u32 = 0xF01FF;
pub const SERVICE_KERNEL_DRIVER: u32 = 1;
pub const SERVICE_SYSTEM_START: u32 = 1;
pub const SERVICE_ERROR_NORMAL: u32 = 1;
pub const SERVICE_DISABLED: u32 = 4;

#[repr(C)]
struct TOKEN_PRIVILEGES {
    count: u32,
    luid: [u32; 2],
    attributes: u32,
}
const _: () = assert!(size_of::<TOKEN_PRIVILEGES>() == 16);

// Network
#[repr(C)]
pub struct IP_ADAPTER_INDEX_MAP {
    pub Index: u32,
    pub Name: [u16; 128],
}
const _: () = assert!(size_of::<IP_ADAPTER_INDEX_MAP>() == 260);

// Device setup
pub const DICD_GENERATE_ID: u32 = 1;
pub const SPDRP_HARDWAREID: u32 = 1;
pub const DIF_REGISTERDEVICE: u32 = 25;

#[repr(C)]
pub struct GUID(pub u32, pub u16, pub u16, pub [u8; 8]);

#[repr(C)]
pub struct SP_DEVINFO_DATA {
    pub cbSize: u32,
    pub ClassGuid: [u8; 16],
    pub DevInst: u32,
    pub Reserved: usize,
}
const _: () = assert!(size_of::<SP_DEVINFO_DATA>() == 28);

// Accounts
pub const USER_PRIV_USER: u32 = 1;
pub const UF_SCRIPT: u32 = 0x1;
pub const UF_DONT_EXPIRE_PASSWD: u32 = 0x10000;

#[repr(C)]
pub struct USER_INFO_1 {
    pub name: *const u16,
    pub password: *const u16,
    pub password_age: u32,
    pub privilege: u32,
    pub home_dir: *const u16,
    pub comment: *const u16,
    pub flags: u32,
    pub script_path: *const u16,
}
const _: () = assert!(size_of::<USER_INFO_1>() == 32);

// Shell
pub const CSIDL_DESKTOPDIRECTORY: i32 = 0x10;
pub const SHCNE_CREATE: i32 = 2;
pub const SHCNF_PATHW: u32 = 5;

#[link(name = "kernel32")]
unsafe extern "system" {
    pub fn CloseHandle(handle: HANDLE) -> BOOL;
    pub fn CreateMutexW(attributes: *mut c_void, owner: BOOL, name: *const u16) -> HANDLE;
    pub fn GetLastError() -> u32;
    fn ExpandEnvironmentStringsW(source: *const u16, destination: *mut u16, size: u32) -> u32;
    pub fn GetModuleHandleW(name: *const u16) -> HANDLE;
    pub fn LoadLibraryW(name: *const u16) -> HANDLE;
    pub fn GetProcAddress(module: HANDLE, name: *const u8) -> *mut c_void;
    pub fn DeviceIoControl(
        device: HANDLE,
        code: u32,
        input: *const c_void,
        input_size: u32,
        output: *mut c_void,
        output_size: u32,
        returned: *mut u32,
        overlapped: *mut c_void,
    ) -> BOOL;
    pub fn GlobalAlloc(flags: u32, size: usize) -> HANDLE;
    pub fn GlobalLock(memory: HANDLE) -> *mut c_void;
    pub fn GlobalUnlock(memory: HANDLE) -> BOOL;
    pub fn GlobalFree(memory: HANDLE) -> HANDLE;
    pub fn GlobalSize(memory: HANDLE) -> usize;
    pub fn GetTimeZoneInformation(info: *mut TIME_ZONE_INFORMATION) -> u32;
    pub fn SetTimeZoneInformation(info: *const TIME_ZONE_INFORMATION) -> BOOL;
    pub fn GetSystemTimeAsFileTime(time: *mut u64);
    pub fn FileTimeToSystemTime(file_time: *const u64, system_time: *mut [u16; 8]) -> BOOL;
    pub fn SetSystemTime(system_time: *const [u16; 8]) -> BOOL;
}

#[link(name = "user32")]
unsafe extern "system" {
    pub fn RegisterClassW(class: *const WNDCLASSW) -> u16;
    pub fn CreateWindowExW(
        ex_style: u32,
        class: *const u16,
        name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: HWND,
        menu: HANDLE,
        instance: HANDLE,
        param: *mut c_void,
    ) -> HWND;
    pub fn DefWindowProcW(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT;
    pub fn GetMessageW(message: *mut MSG, hwnd: HWND, min: u32, max: u32) -> BOOL;
    pub fn DispatchMessageW(message: *const MSG) -> LRESULT;
    pub fn PostQuitMessage(code: i32);
    pub fn SetTimer(hwnd: HWND, id: usize, elapse: u32, function: *mut c_void) -> usize;
    pub fn SendMessageW(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT;
    pub fn PostMessageW(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> BOOL;
    pub fn SendMessageTimeoutW(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        flags: u32,
        timeout: u32,
        result: *mut usize,
    ) -> LRESULT;
    pub fn FindWindowW(class: *const u16, name: *const u16) -> HWND;
    pub fn FindWindowExW(parent: HWND, after: HWND, class: *const u16, name: *const u16) -> HWND;
    pub fn MessageBoxW(hwnd: HWND, text: *const u16, caption: *const u16, kind: u32) -> i32;
    pub fn ExitWindowsEx(flags: u32, reason: u32) -> BOOL;
    pub fn SetClipboardViewer(hwnd: HWND) -> HWND;
    pub fn ChangeClipboardChain(remove: HWND, next: HWND) -> BOOL;
    pub fn OpenClipboard(hwnd: HWND) -> BOOL;
    pub fn CloseClipboard() -> BOOL;
    pub fn EmptyClipboard() -> BOOL;
    pub fn GetClipboardData(format: u32) -> HANDLE;
    pub fn SetClipboardData(format: u32, memory: HANDLE) -> HANDLE;
    pub fn RegisterClipboardFormatW(name: *const u16) -> u32;
    pub fn EnumDisplaySettingsW(device: *const u16, index: u32, mode: *mut DEVMODEW) -> BOOL;
    pub fn ChangeDisplaySettingsW(mode: *const DEVMODEW, flags: u32) -> i32;
    pub fn GetSystemMetrics(index: i32) -> i32;
    pub fn GetDC(hwnd: HWND) -> HDC;
    pub fn ReleaseDC(hwnd: HWND, dc: HDC) -> i32;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    pub fn GetDeviceCaps(dc: HDC, index: i32) -> i32;
    pub fn CreateDIBSection(
        dc: HDC,
        info: *const BITMAPINFOHEADER,
        usage: u32,
        bits: *mut *mut c_void,
        section: HANDLE,
        offset: u32,
    ) -> HANDLE;
    pub fn CreateCompatibleDC(dc: HDC) -> HDC;
    pub fn SelectObject(dc: HDC, object: HANDLE) -> HANDLE;
    pub fn SetDIBitsToDevice(
        dc: HDC,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        source_x: i32,
        source_y: i32,
        start_scan: u32,
        lines: u32,
        bits: *const c_void,
        info: *const BITMAPINFOHEADER,
        usage: u32,
    ) -> i32;
    pub fn GdiFlush() -> BOOL;
    pub fn DeleteDC(dc: HDC) -> BOOL;
    pub fn DeleteObject(object: HANDLE) -> BOOL;
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn OpenProcessToken(process: HANDLE, access: u32, token: *mut HANDLE) -> BOOL;
    fn LookupPrivilegeValueW(system: *const u16, name: *const u16, luid: *mut [u32; 2]) -> BOOL;
    fn AdjustTokenPrivileges(
        token: HANDLE,
        disable_all: BOOL,
        new: *const TOKEN_PRIVILEGES,
        length: u32,
        previous: *mut c_void,
        returned: *mut u32,
    ) -> BOOL;
    fn RegOpenKeyExW(
        key: HKEY,
        subkey: *const u16,
        options: u32,
        access: u32,
        result: *mut HKEY,
    ) -> i32;
    fn RegCreateKeyExW(
        key: HKEY,
        subkey: *const u16,
        reserved: u32,
        class: *const u16,
        options: u32,
        access: u32,
        attributes: *mut c_void,
        result: *mut HKEY,
        disposition: *mut u32,
    ) -> i32;
    fn RegSetValueExW(
        key: HKEY,
        name: *const u16,
        reserved: u32,
        kind: u32,
        data: *const u8,
        size: u32,
    ) -> i32;
    fn RegQueryValueExW(
        key: HKEY,
        name: *const u16,
        reserved: *mut u32,
        kind: *mut u32,
        data: *mut u8,
        size: *mut u32,
    ) -> i32;
    fn RegEnumKeyExW(
        key: HKEY,
        index: u32,
        name: *mut u16,
        name_size: *mut u32,
        reserved: *mut u32,
        class: *mut u16,
        class_size: *mut u32,
        time: *mut u64,
    ) -> i32;
    fn RegDeleteKeyW(key: HKEY, subkey: *const u16) -> i32;
    fn RegDeleteValueW(key: HKEY, name: *const u16) -> i32;
    fn RegLoadKeyW(key: HKEY, subkey: *const u16, file: *const u16) -> i32;
    fn RegUnLoadKeyW(key: HKEY, subkey: *const u16) -> i32;
    fn RegCloseKey(key: HKEY) -> i32;
    pub fn OpenSCManagerW(machine: *const u16, database: *const u16, access: u32) -> HANDLE;
    pub fn CreateServiceW(
        manager: HANDLE,
        name: *const u16,
        display_name: *const u16,
        access: u32,
        kind: u32,
        start: u32,
        error_control: u32,
        binary: *const u16,
        group: *const u16,
        tag: *mut u32,
        dependencies: *const u16,
        account: *const u16,
        password: *const u16,
    ) -> HANDLE;
    pub fn CloseServiceHandle(service: HANDLE) -> BOOL;
}

#[link(name = "iphlpapi")]
unsafe extern "system" {
    pub fn GetInterfaceInfo(info: *mut c_void, size: *mut u32) -> u32;
    pub fn IpRenewAddress(adapter: *const IP_ADAPTER_INDEX_MAP) -> u32;
}

#[link(name = "shell32")]
unsafe extern "system" {
    pub fn SHGetSpecialFolderPathW(hwnd: HWND, path: *mut u16, folder: i32, create: BOOL) -> BOOL;
    pub fn SHChangeNotify(event: i32, flags: u32, item1: *const c_void, item2: *const c_void);
}

#[link(name = "ole32")]
unsafe extern "system" {
    pub fn CoInitialize(reserved: *mut c_void) -> i32;
}

#[link(name = "setupapi")]
unsafe extern "system" {
    pub fn SetupDiCreateDeviceInfoList(class: *const GUID, parent: HWND) -> HANDLE;
    pub fn SetupDiCreateDeviceInfoW(
        set: HANDLE,
        name: *const u16,
        class: *const GUID,
        description: *const u16,
        parent: HWND,
        flags: u32,
        data: *mut SP_DEVINFO_DATA,
    ) -> BOOL;
    pub fn SetupDiSetDeviceRegistryPropertyW(
        set: HANDLE,
        data: *mut SP_DEVINFO_DATA,
        property: u32,
        buffer: *const u8,
        size: u32,
    ) -> BOOL;
    pub fn SetupDiCallClassInstaller(
        function: u32,
        set: HANDLE,
        data: *mut SP_DEVINFO_DATA,
    ) -> BOOL;
    pub fn SetupDiDestroyDeviceInfoList(set: HANDLE) -> BOOL;
}

#[link(name = "netapi32")]
unsafe extern "system" {
    pub fn NetUserAdd(
        server: *const u16,
        level: u32,
        buffer: *const c_void,
        error: *mut u32,
    ) -> u32;
    pub fn NetUserSetInfo(
        server: *const u16,
        user: *const u16,
        level: u32,
        buffer: *const c_void,
        error: *mut u32,
    ) -> u32;
    pub fn NetLocalGroupAddMembers(
        server: *const u16,
        group: *const u16,
        level: u32,
        buffer: *const c_void,
        count: u32,
    ) -> u32;
}

/// A NUL terminated UTF-16 string
pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

/// UTF-16 strings, each NUL terminated, with the NUL that ends a REG_MULTI_SZ
pub fn wide_multi(texts: &[&str]) -> Vec<u16> {
    texts
        .iter()
        .flat_map(|text| wide(text))
        .chain([0])
        .collect()
}

/// The bytes of UTF-16 text for the registry
pub fn utf16_bytes(text: &[u16]) -> Vec<u8> {
    text.iter().flat_map(|unit| unit.to_le_bytes()).collect()
}

/// Expands environment variables like `%SystemRoot%`
pub fn expand(text: &str) -> String {
    let source = wide(text);
    let mut buffer = [0_u16; 1024];
    // SAFETY: The source is NUL terminated and the buffer holds its length
    let length = unsafe {
        ExpandEnvironmentStringsW(source.as_ptr(), buffer.as_mut_ptr(), buffer.len() as u32)
    };
    match length as usize {
        1..=1024 => String::from_utf16_lossy(&buffer[..length as usize - 1]),
        _ => text.into(),
    }
}

/// Enables a privilege of the process token, like `SeShutdownPrivilege`
pub fn enable_privilege(name: &str) {
    let name = wide(name);
    // SAFETY: The handles and buffers are valid for the calls, the token is closed after
    unsafe {
        let mut token = ptr::null_mut();
        // The pseudo handle of the current process
        if OpenProcessToken(-1_isize as HANDLE, TOKEN_ADJUST_PRIVILEGES, &mut token) == 0 {
            return;
        }
        let mut privileges = TOKEN_PRIVILEGES {
            count: 1,
            luid: [0; 2],
            attributes: SE_PRIVILEGE_ENABLED,
        };
        if LookupPrivilegeValueW(ptr::null(), name.as_ptr(), &mut privileges.luid) != 0 {
            AdjustTokenPrivileges(token, 0, &privileges, 0, ptr::null_mut(), ptr::null_mut());
        }
        CloseHandle(token);
    }
}

/// A registry key, the opened ones are closed on drop
pub struct Key {
    raw: HKEY,
    owned: bool,
}

impl Key {
    pub const LOCAL_MACHINE: Self = Self::predefined(HKEY_LOCAL_MACHINE);
    pub const CURRENT_USER: Self = Self::predefined(HKEY_CURRENT_USER);
    pub const USERS: Self = Self::predefined(HKEY_USERS);

    const fn predefined(raw: HKEY) -> Self {
        Self { raw, owned: false }
    }

    pub fn open(&self, path: &str, access: u32) -> Option<Self> {
        let path = wide(path);
        let mut raw = ptr::null_mut();
        // SAFETY: The key is open, the path is NUL terminated and the result is valid for writes
        (unsafe { RegOpenKeyExW(self.raw, path.as_ptr(), 0, access, &mut raw) } == ERROR_SUCCESS)
            .then_some(Self { raw, owned: true })
    }

    pub fn create(&self, path: &str, access: u32) -> Option<Self> {
        let path = wide(path);
        let mut raw = ptr::null_mut();
        // SAFETY: The key is open, the path is NUL terminated and the result is valid for writes
        let status = unsafe {
            RegCreateKeyExW(
                self.raw,
                path.as_ptr(),
                0,
                ptr::null(),
                0,
                access,
                ptr::null_mut(),
                &mut raw,
                ptr::null_mut(),
            )
        };
        (status == ERROR_SUCCESS).then_some(Self { raw, owned: true })
    }

    /// Deletes a subkey without subkeys of its own
    pub fn delete(&self, path: &str) {
        let path = wide(path);
        // SAFETY: The key is open and the path is NUL terminated
        unsafe { RegDeleteKeyW(self.raw, path.as_ptr()) };
    }

    /// Loads a hive file as a subkey, which needs the backup and restore privileges
    pub fn load(&self, name: &str, file: &str) -> bool {
        let (name, file) = (wide(name), wide(file));
        // SAFETY: The key is open and the strings are NUL terminated
        unsafe { RegLoadKeyW(self.raw, name.as_ptr(), file.as_ptr()) == ERROR_SUCCESS }
    }

    /// Unloads a hive, its keys must be closed
    pub fn unload(&self, name: &str) {
        let name = wide(name);
        // SAFETY: The key is open and the name is NUL terminated
        unsafe { RegUnLoadKeyW(self.raw, name.as_ptr()) };
    }

    /// Sets a value, the default value without a name
    pub fn set(&self, name: Option<&str>, kind: u32, data: &[u8]) {
        let name = name.map(wide);
        let name = name.as_ref().map_or(ptr::null(), |name| name.as_ptr());
        // SAFETY: The name is NUL terminated or null and the data has its length
        unsafe { RegSetValueExW(self.raw, name, 0, kind, data.as_ptr(), data.len() as u32) };
    }

    pub fn set_dword(&self, name: &str, value: u32) {
        self.set(Some(name), REG_DWORD, &value.to_le_bytes());
    }

    pub fn delete_value(&self, name: &str) {
        let name = wide(name);
        // SAFETY: The name is NUL terminated
        unsafe { RegDeleteValueW(self.raw, name.as_ptr()) };
    }

    /// The value as a string, without its NUL
    pub fn string(&self, name: &str) -> Option<String> {
        let name = wide(name);
        let mut buffer = [0_u16; 260];
        let mut size = (buffer.len() * 2) as u32;
        // SAFETY: The name is NUL terminated and the buffer holds size bytes
        let status = unsafe {
            RegQueryValueExW(
                self.raw,
                name.as_ptr(),
                ptr::null_mut(),
                ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        };
        (status == ERROR_SUCCESS).then(|| {
            let text = &buffer[..(size as usize / 2).min(buffer.len())];
            String::from_utf16_lossy(text.split(|&unit| unit == 0).next().unwrap_or(text))
        })
    }

    /// The names of the subkeys
    pub fn subkeys(&self) -> Vec<String> {
        let mut names = Vec::new();
        let mut name = [0_u16; 260];
        for index in 0.. {
            let mut length = name.len() as u32;
            // SAFETY: The name buffer holds length characters
            let status = unsafe {
                RegEnumKeyExW(
                    self.raw,
                    index,
                    name.as_mut_ptr(),
                    &mut length,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            };
            if status != ERROR_SUCCESS {
                break;
            }
            names.push(String::from_utf16_lossy(&name[..length as usize]));
        }
        names
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        if self.owned {
            // SAFETY: The key is open and owned
            unsafe { RegCloseKey(self.raw) };
        }
    }
}
