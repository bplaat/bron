/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Official libogc USB keyboard and mouse drivers, assigned to P1.

use alloc::boxed::Box;
use core::sync::atomic::{AtomicBool, Ordering};

use libogc_sys::*;
use plaatcraft_engine::client::Thread;
use plaatcraft_engine::client::desktop::DesktopEvents;

pub static EVENTS: DesktopEvents = DesktopEvents::new();
static MOUSE_READY: AtomicBool = AtomicBool::new(false);

extern "C" fn key(event: USBKeyboardEvent) {
    match event.kind {
        0 => EVENTS.key(event.key_code, true, None),
        1 => EVENTS.key(event.key_code, false, None),
        2 => EVENTS.disconnect_keyboard(),
        _ => {}
    }
}

pub fn start() {
    // The official drivers own their USB handles and hotplug notifications.
    unsafe {
        if USB_Initialize() < 0 {
            return;
        }
        MOUSE_READY.store(MOUSE_Init() >= 0, Ordering::Relaxed);
        if USBKeyboard_Initialize() < 0 {
            return;
        }
    }
    crate::system::spawn(
        Thread::Input,
        Box::new(|| {
            loop {
                // Interrupt reads may block until a key changes, so keyboard scans stay off the render thread.
                unsafe {
                    if USBKeyboard_IsConnected() {
                        if USBKeyboard_Scan() < 0 {
                            USBKeyboard_Close();
                            EVENTS.disconnect_keyboard();
                        }
                        usleep(2_000);
                    } else {
                        EVENTS.disconnect_keyboard();
                        let opened = USBKeyboard_Open(Some(key));
                        usleep(if opened >= 0 { 2_000 } else { 500_000 });
                    }
                }
            }
        }),
    );
}

pub fn poll() {
    if !MOUSE_READY.load(Ordering::Relaxed) {
        return;
    }
    let mut event = MouseEvent::default();
    for _ in 0..128 {
        // MOUSE_GetEvent is a nonblocking dequeue from libogc's mouse thread.
        if unsafe { MOUSE_GetEvent(&mut event) } <= 0 {
            break;
        }
        EVENTS.mouse(event.buttons, event.dx, event.dy, event.wheel);
    }
    if !unsafe { MOUSE_IsConnected() } {
        EVENTS.disconnect_mouse();
    }
}
