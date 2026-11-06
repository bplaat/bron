/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Native nsyskbd keyboard input and nsyshid boot mice, assigned to P1.

use alloc::boxed::Box;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use plaatcraft_engine::client::Thread;
use plaatcraft_engine::client::desktop::DesktopEvents;
use wut_sys::*;

pub static EVENTS: DesktopEvents = DesktopEvents::new();
static KEYBOARD: AtomicU32 = AtomicU32::new(u32::MAX);
static MOUSE: AtomicU32 = AtomicU32::new(u32::MAX);
static INTERFACE: AtomicU32 = AtomicU32::new(0);
static PACKET_SIZE: AtomicU32 = AtomicU32::new(0);
static PENDING: AtomicBool = AtomicBool::new(false);

unsafe extern "C" fn keyboard_attach(event: *mut KBDAttachEvent) {
    if !event.is_null() {
        // The native callback supplies this packed event for the duration of the call.
        let event = unsafe { ptr::read_unaligned(event) };
        let _ = KEYBOARD.compare_exchange(
            u32::MAX,
            u32::from(event.channel),
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
    }
}

unsafe extern "C" fn keyboard_detach(event: *mut KBDAttachEvent) {
    if !event.is_null() {
        let event = unsafe { ptr::read_unaligned(event) };
        if KEYBOARD
            .compare_exchange(
                u32::from(event.channel),
                u32::MAX,
                Ordering::Relaxed,
                Ordering::Relaxed,
            )
            .is_ok()
        {
            EVENTS.disconnect_keyboard();
        }
    }
}

unsafe extern "C" fn key(event: *mut KBDKeyEvent) {
    if event.is_null() {
        return;
    }
    let event = unsafe { ptr::read_unaligned(event) };
    if u32::from(event.channel) == KEYBOARD.load(Ordering::Relaxed) {
        EVENTS.key(
            event.hid_code,
            event.pressed != 0,
            Some(char::from_u32(u32::from(event.character)).unwrap_or('\0')),
        );
    }
}

unsafe extern "C" fn attach(_client: *mut HIDClient, device: *mut HIDDevice, attached: u32) -> i32 {
    if device.is_null() {
        return 0;
    }
    let device = unsafe { ptr::read(device) };
    if attached == 0 {
        if MOUSE
            .compare_exchange(device.handle, u32::MAX, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            EVENTS.disconnect_mouse();
        }
        return 0;
    }
    // Claim only standard boot mice. nsyskbd retains ownership of USB keyboards.
    if device.sub_class != 1
        || device.protocol != 2
        || device.max_packet_rx < 3
        || device.max_packet_rx > 64
    {
        return 0;
    }
    if MOUSE.load(Ordering::Acquire) != u32::MAX {
        return 0;
    }
    INTERFACE.store(u32::from(device.interface), Ordering::Relaxed);
    PACKET_SIZE.store(u32::from(device.max_packet_rx), Ordering::Relaxed);
    MOUSE.store(device.handle, Ordering::Release);
    1
}

unsafe extern "C" fn read(
    handle: u32,
    error: i32,
    buffer: *mut u8,
    length: u32,
    _context: *mut c_void,
) {
    if handle == MOUSE.load(Ordering::Acquire) {
        if error == 0 && !buffer.is_null() && (3..=64).contains(&length) {
            // nsyshid owns this aligned buffer until the callback completes.
            let report = unsafe { core::slice::from_raw_parts(buffer, length as usize) };
            EVENTS.boot_mouse(report);
        } else {
            EVENTS.disconnect_mouse();
        }
    }
    if handle != MOUSE.load(Ordering::Acquire) {
        EVENTS.disconnect_mouse();
    }
    // No buffer access follows this release; the worker may now submit another read.
    PENDING.store(false, Ordering::Release);
}

pub fn start() {
    unsafe {
        KBDSetup(Some(keyboard_attach), Some(keyboard_detach), Some(key));
    }
    if unsafe { HIDSetup() } < 0 {
        return;
    }
    // The native service retains the client until process exit.
    let client = Box::into_raw(Box::new(HIDClient {
        next: ptr::null_mut(),
        attach: None,
    }));
    if unsafe { HIDAddClient(client, Some(attach)) } < 0 {
        unsafe {
            drop(Box::from_raw(client));
        }
        return;
    }
    crate::system::spawn(
        Thread::Input,
        Box::new(|| {
            #[repr(align(64))]
            struct Report([u8; 64]);
            let buffer = Box::leak(Box::new(Report([0; 64])));
            let mut configured = None;
            loop {
                let handle = MOUSE.load(Ordering::Acquire);
                if handle != u32::MAX && !PENDING.load(Ordering::Acquire) {
                    if Some(handle) != configured {
                        // Boot protocol gives stable button/x/y fields, independent of a device's report IDs.
                        if unsafe {
                            HIDSetProtocol(
                                handle,
                                INTERFACE.load(Ordering::Relaxed) as u8,
                                0,
                                None,
                                ptr::null_mut(),
                            )
                        } < 0
                        {
                            let _ = MOUSE.compare_exchange(
                                handle,
                                u32::MAX,
                                Ordering::AcqRel,
                                Ordering::Relaxed,
                            );
                            EVENTS.disconnect_mouse();
                            continue;
                        }
                        configured = Some(handle);
                    }
                    PENDING.store(true, Ordering::Release);
                    // The buffer is stable and aligned, and reused only after the previous callback released it.
                    if unsafe {
                        HIDRead(
                            handle,
                            buffer.0.as_mut_ptr(),
                            PACKET_SIZE.load(Ordering::Relaxed),
                            Some(read),
                            ptr::null_mut(),
                        )
                    } < 0
                    {
                        PENDING.store(false, Ordering::Release);
                        EVENTS.disconnect_mouse();
                    }
                } else if handle == u32::MAX {
                    configured = None;
                }
                unsafe {
                    OSSleepTicks(i64::from(OSTimerClockSpeed() / 1000));
                }
            }
        }),
    );
}
