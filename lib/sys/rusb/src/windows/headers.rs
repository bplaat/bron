/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Minimal Win32 and WinUSB declarations used by this backend.

use std::ffi::c_void;

pub(super) type Bool = i32;
pub(super) type Dword = u32;
pub(super) type HandleValue = *mut c_void;
pub(super) type Hdevinfo = HandleValue;
pub(super) type Hkey = HandleValue;
type WinusbInterfaceHandle = HandleValue;

pub(super) const INVALID_HANDLE_VALUE: HandleValue = -1isize as HandleValue;
pub(super) const DIGCF_PRESENT: Dword = 0x0000_0002;
pub(super) const DIGCF_ALLCLASSES: Dword = 0x0000_0004;
pub(super) const DIGCF_DEVICEINTERFACE: Dword = 0x0000_0010;
pub(super) const SPDRP_HARDWAREID: Dword = 0x0000_0001;
pub(super) const DICS_FLAG_GLOBAL: Dword = 0x0000_0001;
pub(super) const DIREG_DEV: Dword = 0x0000_0001;
pub(super) const KEY_READ: Dword = 0x0002_0019;
pub(super) const REG_SZ: Dword = 1;
pub(super) const REG_MULTI_SZ: Dword = 7;
pub(super) const ERROR_FILE_NOT_FOUND: Dword = 2;
pub(super) const ERROR_ACCESS_DENIED: Dword = 5;
pub(super) const ERROR_NOT_ENOUGH_MEMORY: Dword = 8;
pub(super) const ERROR_GEN_FAILURE: Dword = 31;
pub(super) const ERROR_NOT_SUPPORTED: Dword = 50;
pub(super) const ERROR_INVALID_PARAMETER: Dword = 87;
pub(super) const ERROR_INSUFFICIENT_BUFFER: Dword = 122;
pub(super) const ERROR_BUSY: Dword = 170;
pub(super) const ERROR_NO_MORE_ITEMS: Dword = 259;
pub(super) const ERROR_OPERATION_ABORTED: Dword = 995;
pub(super) const ERROR_IO_PENDING: Dword = 997;
pub(super) const ERROR_DEVICE_NOT_CONNECTED: Dword = 1167;
pub(super) const ERROR_SEM_TIMEOUT: Dword = 121;
pub(super) const ERROR_TIMEOUT: Dword = 1460;
pub(super) const GENERIC_READ: Dword = 0x8000_0000;
pub(super) const GENERIC_WRITE: Dword = 0x4000_0000;
pub(super) const FILE_SHARE_READ: Dword = 0x0000_0001;
pub(super) const FILE_SHARE_WRITE: Dword = 0x0000_0002;
pub(super) const OPEN_EXISTING: Dword = 3;
pub(super) const FILE_ATTRIBUTE_NORMAL: Dword = 0x0000_0080;
pub(super) const FILE_FLAG_OVERLAPPED: Dword = 0x4000_0000;
pub(super) const WAIT_OBJECT_0: Dword = 0;
pub(super) const WAIT_TIMEOUT: Dword = 258;
pub(super) const INFINITE: Dword = u32::MAX;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub(super) struct Guid {
    pub(super) data1: u32,
    pub(super) data2: u16,
    pub(super) data3: u16,
    pub(super) data4: [u8; 8],
}

#[repr(C)]
pub(super) struct SpDevinfoData {
    pub(super) size: Dword,
    pub(super) class_guid: Guid,
    pub(super) dev_inst: Dword,
    pub(super) reserved: usize,
}

#[repr(C)]
pub(super) struct SpDeviceInterfaceData {
    pub(super) size: Dword,
    pub(super) interface_class_guid: Guid,
    pub(super) flags: Dword,
    pub(super) reserved: usize,
}

#[derive(Clone, Copy)]
#[repr(C, packed)]
pub(super) struct WinusbSetupPacket {
    pub(super) request_type: u8,
    pub(super) request: u8,
    pub(super) value: u16,
    pub(super) index: u16,
    pub(super) length: u16,
}

#[repr(C)]
pub(super) struct Overlapped {
    pub(super) internal: usize,
    pub(super) internal_high: usize,
    pub(super) offset: Dword,
    pub(super) offset_high: Dword,
    pub(super) event: HandleValue,
}

#[link(name = "setupapi")]
unsafe extern "system" {
    pub(super) fn SetupDiGetClassDevsW(
        class_guid: *const Guid,
        enumerator: *const u16,
        hwnd_parent: HandleValue,
        flags: Dword,
    ) -> Hdevinfo;
    pub(super) fn SetupDiEnumDeviceInfo(
        device_info_set: Hdevinfo,
        member_index: Dword,
        device_info_data: *mut SpDevinfoData,
    ) -> Bool;
    pub(super) fn SetupDiGetDeviceRegistryPropertyW(
        device_info_set: Hdevinfo,
        device_info_data: *mut SpDevinfoData,
        property: Dword,
        property_reg_data_type: *mut Dword,
        property_buffer: *mut u8,
        property_buffer_size: Dword,
        required_size: *mut Dword,
    ) -> Bool;
    pub(super) fn SetupDiOpenDevRegKey(
        device_info_set: Hdevinfo,
        device_info_data: *mut SpDevinfoData,
        scope: Dword,
        hw_profile: Dword,
        key_type: Dword,
        sam_desired: Dword,
    ) -> Hkey;
    pub(super) fn SetupDiEnumDeviceInterfaces(
        device_info_set: Hdevinfo,
        device_info_data: *mut SpDevinfoData,
        interface_class_guid: *const Guid,
        member_index: Dword,
        device_interface_data: *mut SpDeviceInterfaceData,
    ) -> Bool;
    pub(super) fn SetupDiGetDeviceInterfaceDetailW(
        device_info_set: Hdevinfo,
        device_interface_data: *mut SpDeviceInterfaceData,
        device_interface_detail_data: *mut c_void,
        device_interface_detail_data_size: Dword,
        required_size: *mut Dword,
        device_info_data: *mut SpDevinfoData,
    ) -> Bool;
    pub(super) fn SetupDiDestroyDeviceInfoList(device_info_set: Hdevinfo) -> Bool;
}

#[link(name = "advapi32")]
unsafe extern "system" {
    pub(super) fn RegQueryValueExW(
        key: Hkey,
        value_name: *const u16,
        reserved: *mut Dword,
        value_type: *mut Dword,
        data: *mut u8,
        data_size: *mut Dword,
    ) -> i32;
    pub(super) fn RegCloseKey(key: Hkey) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    pub(super) fn CreateFileW(
        file_name: *const u16,
        desired_access: Dword,
        share_mode: Dword,
        security_attributes: *mut c_void,
        creation_disposition: Dword,
        flags_and_attributes: Dword,
        template_file: HandleValue,
    ) -> HandleValue;
    pub(super) fn CloseHandle(object: HandleValue) -> Bool;
    pub(super) fn CreateEventW(
        event_attributes: *mut c_void,
        manual_reset: Bool,
        initial_state: Bool,
        name: *const u16,
    ) -> HandleValue;
    pub(super) fn WaitForSingleObject(handle: HandleValue, milliseconds: Dword) -> Dword;
    pub(super) fn CancelIoEx(file: HandleValue, overlapped: *mut Overlapped) -> Bool;
    pub(super) fn GetLastError() -> Dword;
}

#[link(name = "winusb")]
unsafe extern "system" {
    pub(super) fn WinUsb_Initialize(
        device_handle: HandleValue,
        interface_handle: *mut WinusbInterfaceHandle,
    ) -> Bool;
    pub(super) fn WinUsb_Free(interface_handle: WinusbInterfaceHandle) -> Bool;
    pub(super) fn WinUsb_GetAssociatedInterface(
        interface_handle: WinusbInterfaceHandle,
        associated_interface_index: u8,
        associated_interface_handle: *mut WinusbInterfaceHandle,
    ) -> Bool;
    pub(super) fn WinUsb_ControlTransfer(
        interface_handle: WinusbInterfaceHandle,
        setup_packet: WinusbSetupPacket,
        buffer: *mut u8,
        buffer_length: Dword,
        length_transferred: *mut Dword,
        overlapped: *mut Overlapped,
    ) -> Bool;
    pub(super) fn WinUsb_GetOverlappedResult(
        interface_handle: WinusbInterfaceHandle,
        overlapped: *mut Overlapped,
        length_transferred: *mut Dword,
        wait: Bool,
    ) -> Bool;
}

#[cfg(test)]
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
