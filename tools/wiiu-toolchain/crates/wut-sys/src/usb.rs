/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Native nsyskbd keyboard and nsyshid USB HID interfaces from WUT.

use core::ffi::c_void;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct KBDAttachEvent {
    pub channel: u8,
    pub padding: [u8; 3],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct KBDKeyEvent {
    pub channel: u8,
    pub hid_code: u8,
    pub padding: [u8; 2],
    pub pressed: i32,
    pub unknown: [u8; 4],
    pub character: u16,
    pub trailing: [u8; 2],
}

pub type KBDAttachCallback = Option<unsafe extern "C" fn(*mut KBDAttachEvent)>;
pub type KBDKeyCallback = Option<unsafe extern "C" fn(*mut KBDKeyEvent)>;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct HIDDevice {
    pub handle: u32,
    pub physical_device: u32,
    pub vid: u16,
    pub pid: u16,
    pub interface: u8,
    pub sub_class: u8,
    pub protocol: u8,
    pub padding: u8,
    pub max_packet_rx: u16,
    pub max_packet_tx: u16,
}

#[repr(C)]
pub struct HIDClient {
    pub next: *mut HIDClient,
    pub attach: HIDAttachCallback,
}

pub type HIDAttachCallback =
    Option<unsafe extern "C" fn(*mut HIDClient, *mut HIDDevice, u32) -> i32>;
pub type HIDCallback = Option<unsafe extern "C" fn(u32, i32, *mut u8, u32, *mut c_void)>;

unsafe extern "C" {
    pub fn KBDSetup(
        attach: KBDAttachCallback,
        detach: KBDAttachCallback,
        key: KBDKeyCallback,
    ) -> i32;
    pub fn KBDTeardown() -> i32;
    pub fn HIDSetup() -> i32;
    pub fn HIDTeardown() -> i32;
    pub fn HIDAddClient(client: *mut HIDClient, callback: HIDAttachCallback) -> i32;
    pub fn HIDDelClient(client: *mut HIDClient) -> i32;
    pub fn HIDSetProtocol(
        handle: u32,
        interface: u8,
        protocol: u8,
        callback: HIDCallback,
        context: *mut c_void,
    ) -> i32;
    pub fn HIDRead(
        handle: u32,
        buffer: *mut u8,
        length: u32,
        callback: HIDCallback,
        context: *mut c_void,
    ) -> i32;
    pub fn HIDWrite(
        handle: u32,
        buffer: *mut u8,
        length: u32,
        callback: HIDCallback,
        context: *mut c_void,
    ) -> i32;
    pub fn HIDGetReport(
        handle: u32,
        kind: u8,
        id: u8,
        buffer: *mut u8,
        length: u32,
        callback: HIDCallback,
        context: *mut c_void,
    ) -> i32;
    pub fn HIDSetReport(
        handle: u32,
        kind: u8,
        id: u8,
        buffer: *mut u8,
        length: u32,
        callback: HIDCallback,
        context: *mut c_void,
    ) -> i32;
    pub fn HIDGetDescriptor(
        handle: u32,
        kind: u8,
        index: u8,
        language: u16,
        buffer: *mut u8,
        length: u32,
        callback: HIDCallback,
        context: *mut c_void,
    ) -> i32;
}
