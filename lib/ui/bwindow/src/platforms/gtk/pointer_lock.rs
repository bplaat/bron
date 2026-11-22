/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The relative pointer motion of a pointer lock. X11 keeps the hidden pointer in the middle
//! of the content and reports how far it moves from there. Wayland cannot move the pointer,
//! so it locks it in place with the pointer constraints protocol and reports the relative
//! pointer motion. Without those protocols the motion of the grabbed pointer is reported.

use std::cell::RefCell;
use std::ffi::c_void;

use super::headers::*;
use super::input::gtk_input_event;
use super::wayland;
use crate::{LogicalPoint, WindowEvent, WindowEventSender};

const GDK_MOTION_NOTIFY: i32 = 3;

unsafe extern "C" {
    fn gdk_seat_get_pointer(seat: *mut c_void) -> *mut c_void;
    fn gdk_window_get_screen(window: *mut c_void) -> *mut c_void;
    fn gdk_window_get_root_coords(window: *mut c_void, x: i32, y: i32, rx: *mut i32, ry: *mut i32);
    fn gdk_event_get_root_coords(event: *const GdkEvent, x: *mut f64, y: *mut f64) -> i32;
    fn gdk_device_warp(device: *mut c_void, screen: *mut c_void, x: i32, y: i32);
}

enum Lock {
    // The pointer is moved back to `center` in root coordinates after every motion.
    Warp {
        device: *mut c_void,
        screen: *mut c_void,
        center: (i32, i32),
    },
    // Destroying the lock releases the pointer.
    Wayland {
        _lock: wayland::Lock,
    },
    // The pointer moves freely, from `last` in window coordinates.
    Grab {
        last: Option<(f64, f64)>,
    },
}

thread_local! {
    static LOCK: RefCell<Option<Lock>> = const { RefCell::new(None) };
}

/// Start reporting relative motion over `widget` for the grabbed pointer of `seat`.
pub(super) unsafe fn start(
    display: *mut GdkDisplay,
    seat: *mut c_void,
    widget: *mut GtkWidget,
    sender: &WindowEventSender,
) {
    stop();
    let window = unsafe { gtk_widget_get_window(widget) };
    let device = unsafe { gdk_seat_get_pointer(seat) };
    if unsafe { gdk_display_is_wayland(display) } {
        let lock = unsafe { wayland::Lock::new(display, device, window, sender) };
        LOCK.set(Some(lock.map_or(Lock::Grab { last: None }, |lock| {
            Lock::Wayland { _lock: lock }
        })));
        return;
    }
    let (mut x, mut y) = (0, 0);
    unsafe {
        gdk_window_get_root_coords(
            window,
            gtk_widget_get_allocated_width(widget) / 2,
            gtk_widget_get_allocated_height(widget) / 2,
            &mut x,
            &mut y,
        );
    }
    let screen = unsafe { gdk_window_get_screen(window) };
    unsafe { gdk_device_warp(device, screen, x, y) };
    LOCK.set(Some(Lock::Warp {
        device,
        screen,
        center: (x, y),
    }));
}

/// Stop reporting relative motion.
pub(super) fn stop() {
    LOCK.set(None);
}

/// Report the motion of a locked pointer, returns whether `event` was handled.
pub(super) unsafe fn motion(event: *const GdkEvent, sender: &WindowEventSender) -> bool {
    if unsafe { gdk_event_get_event_type(event) } != GDK_MOTION_NOTIFY {
        return false;
    }
    LOCK.with_borrow_mut(|lock| {
        let movement = match lock {
            None => return false,
            Some(Lock::Warp {
                device,
                screen,
                center,
            }) => {
                let (mut x, mut y) = (0.0, 0.0);
                unsafe { gdk_event_get_root_coords(event, &mut x, &mut y) };
                let movement = (x - f64::from(center.0), y - f64::from(center.1));
                if movement == (0.0, 0.0) {
                    // The motion of moving the pointer back.
                    return true;
                }
                unsafe { gdk_device_warp(*device, *screen, center.0, center.1) };
                movement
            }
            // The relative pointer reports the motion of the locked pointer.
            Some(Lock::Wayland { .. }) => return true,
            Some(Lock::Grab { last }) => {
                let (mut x, mut y) = (0.0, 0.0);
                unsafe { gdk_event_get_coords(event, &mut x, &mut y) };
                let movement = last.map_or((0.0, 0.0), |(lx, ly)| (x - lx, y - ly));
                *last = Some((x, y));
                movement
            }
        };
        if let Some(WindowEvent::MouseMove {
            position,
            modifiers,
            ..
        }) = unsafe { gtk_input_event(event.cast(), false) }
        {
            sender.send(WindowEvent::MouseMove {
                position,
                movement: LogicalPoint::new(movement.0 as f32, movement.1 as f32),
                modifiers,
            });
        }
        true
    })
}
