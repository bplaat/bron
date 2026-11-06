/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Official libogc USB boot-keyboard and mouse interfaces.

use core::ffi::c_void;

pub type USBCallback = Option<unsafe extern "C" fn(i32, *mut c_void) -> i32>;

#[repr(C)]
pub struct USBDeviceEntry {
    pub device_id: i32,
    pub vid: u16,
    pub pid: u16,
    pub token: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct USBKeyboardEvent {
    pub kind: u32,
    pub key_code: u8,
}

#[repr(C)]
#[derive(Default)]
pub struct MouseEvent {
    pub buttons: u8,
    pub dx: i32,
    pub dy: i32,
    pub wheel: i32,
}

unsafe extern "C" {
    pub fn USB_Initialize() -> i32;
    pub fn USB_GetDeviceList(
        devices: *mut USBDeviceEntry,
        capacity: u8,
        class: u8,
        count: *mut u8,
    ) -> i32;
    pub fn USB_OpenDevice(device_id: i32, vid: u16, pid: u16, fd: *mut i32) -> i32;
    pub fn USB_CloseDevice(fd: *mut i32) -> i32;
    pub fn USB_GetReportDescriptorSize(fd: i32, interface: u8) -> i32;
    pub fn USB_GetReportDescriptor(fd: i32, interface: u8, data: *mut c_void, size: u16) -> i32;
    pub fn USB_ReadIntrMsg(fd: i32, endpoint: u8, length: u16, data: *mut c_void) -> i32;
    pub fn USB_WriteIntrMsg(fd: i32, endpoint: u8, length: u16, data: *mut c_void) -> i32;
    pub fn USB_ReadIntrMsgAsync(
        fd: i32,
        endpoint: u8,
        length: u16,
        data: *mut c_void,
        callback: USBCallback,
        context: *mut c_void,
    ) -> i32;
    pub fn USB_DeviceRemovalNotifyAsync(
        fd: i32,
        callback: USBCallback,
        context: *mut c_void,
    ) -> i32;
    pub fn USB_DeviceChangeNotifyAsync(
        class: u8,
        callback: USBCallback,
        context: *mut c_void,
    ) -> i32;
    pub fn USB_ReadCtrlMsg(
        fd: i32,
        kind: u8,
        request: u8,
        value: u16,
        index: u16,
        length: u16,
        data: *mut c_void,
    ) -> i32;
    pub fn USB_WriteCtrlMsg(
        fd: i32,
        kind: u8,
        request: u8,
        value: u16,
        index: u16,
        length: u16,
        data: *mut c_void,
    ) -> i32;
    pub fn USBKeyboard_Initialize() -> i32;
    pub fn USBKeyboard_Open(callback: Option<extern "C" fn(USBKeyboardEvent)>) -> i32;
    pub fn USBKeyboard_IsConnected() -> bool;
    pub fn USBKeyboard_Scan() -> i32;
    pub fn USBKeyboard_Close();
    pub fn MOUSE_Init() -> i32;
    pub fn MOUSE_GetEvent(event: *mut MouseEvent) -> i32;
    pub fn MOUSE_IsConnected() -> bool;
    pub fn MOUSE_Deinit() -> i32;
    pub fn usleep(microseconds: u32) -> i32;
}
