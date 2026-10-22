/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The shared GtkDrawingArea renderers draw into, created on first use so an alternative
//! window child like a web view never gets one, and the interaction of the window. Input
//! propagates from the drawing area to the GtkWindow, which reports it. Window children
//! that handle their own input stop its propagation, so it is never delivered twice.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::ffi::{c_char, c_void};
use std::ptr::{null, null_mut};

use super::headers::*;
use super::input::gtk_input_event;
use crate::{CursorIcon, WindowEvent, WindowEventSender};

// Motion, buttons, key presses and releases, enter/leave, focus, scroll and smooth scroll.
const INPUT_EVENTS: i32 = (1 << 2)
    | (1 << 8)
    | (1 << 9)
    | (1 << 10)
    | (1 << 11)
    | (1 << 12)
    | (1 << 13)
    | (1 << 14)
    | (1 << 21)
    | (1 << 23);
const G_CONNECT_AFTER: i32 = 1;
const G_SIGNAL_MATCH_DATA: u32 = 16;
const GDK_BUTTON_PRESS: i32 = 4;
const GDK_KEY_PRESS: i32 = 8;
const GDK_KEY_RELEASE: i32 = 9;

unsafe extern "C" {
    fn gtk_drawing_area_new() -> *mut GtkWidget;
    fn g_object_ref_sink(object: *mut GObject) -> *mut GObject;
    fn gtk_widget_set_can_focus(widget: *mut GtkWidget, can_focus: i32);
    fn gtk_widget_add_events(widget: *mut GtkWidget, events: i32);
    fn gtk_widget_grab_focus(widget: *mut GtkWidget);
    fn gtk_container_check_resize(container: *mut GtkWidget);
    fn gtk_get_event_widget(event: *const GdkEvent) -> *mut GtkWidget;
    fn gdk_window_get_display(window: *mut c_void) -> *mut GdkDisplay;
    fn gdk_cursor_new_from_name(display: *mut GdkDisplay, name: *const c_char) -> *mut GObject;
    fn gdk_window_set_cursor(window: *mut c_void, cursor: *mut GObject);
}

/// The content host and interaction state of one window.
pub(super) struct Content {
    window: Cell<*mut GtkWindow>,
    area: Cell<*mut GtkWidget>,
    sender: WindowEventSender,
    keys: RefCell<HashSet<u16>>,
    cursor: Cell<CursorIcon>,
}

impl Content {
    pub(super) fn new(sender: WindowEventSender) -> Self {
        Self {
            window: Cell::new(null_mut()),
            area: Cell::new(null_mut()),
            sender,
            keys: RefCell::new(HashSet::new()),
            cursor: Cell::new(CursorIcon::Default),
        }
    }

    /// Report the interaction of `window`, this state must stay at its address until drop.
    pub(super) fn connect(&self, window: *mut GtkWindow) {
        self.window.set(window);
        let data = self as *const Self as *const c_void;
        unsafe {
            gtk_widget_add_events(window.cast(), INPUT_EVENTS);
            for signal in [
                c"button-press-event",
                c"button-release-event",
                c"motion-notify-event",
                c"scroll-event",
                c"leave-notify-event",
            ] {
                connect(
                    window.cast(),
                    signal,
                    pointer_event as *const c_void,
                    data,
                    0,
                );
            }
            // Accelerators and the focused window child handle keys first.
            for signal in [c"key-press-event", c"key-release-event"] {
                connect(
                    window.cast(),
                    signal,
                    key_event as *const c_void,
                    data,
                    G_CONNECT_AFTER,
                );
            }
            connect(
                window.cast(),
                c"focus-in-event",
                focus_in as *const c_void,
                data,
                0,
            );
            connect(
                window.cast(),
                c"focus-out-event",
                focus_out as *const c_void,
                data,
                0,
            );
            connect(window.cast(), c"realize", realize as *const c_void, data, 0);
        }
    }

    /// The shared drawing area, created on first use.
    pub(super) fn area(&self) -> *mut GtkWidget {
        if !self.area.get().is_null() {
            return self.area.get();
        }
        let data = self as *const Self as *const c_void;
        unsafe {
            let area = gtk_drawing_area_new();
            g_object_ref_sink(area.cast());
            gtk_widget_set_can_focus(area, 1);
            gtk_widget_add_events(area, INPUT_EVENTS);
            connect(
                area,
                c"leave-notify-event",
                pointer_event as *const c_void,
                data,
                0,
            );
            connect(area, c"realize", realize as *const c_void, data, 0);
            self.area.set(area);
            let window = self.window.get();
            gtk_container_add(window.cast(), area);
            gtk_widget_show(area);
            // Allocate the content area now, GTK otherwise leaves a newly attached widget
            // at its initial 1x1 allocation until the next layout pass.
            gtk_container_check_resize(window.cast());
            gtk_widget_grab_focus(area);
            area
        }
    }

    /// Show `cursor` over the content.
    pub(super) fn set_cursor(&self, cursor: CursorIcon) {
        if self.cursor.replace(cursor) != cursor {
            self.apply_cursor();
        }
    }

    fn apply_cursor(&self) {
        let widget = if self.area.get().is_null() {
            self.window.get().cast()
        } else {
            self.area.get()
        };
        unsafe {
            let window = gtk_widget_get_window(widget);
            if window.is_null() {
                return;
            }
            let name = match self.cursor.get() {
                CursorIcon::Default => c"default",
                CursorIcon::Pointer => c"pointer",
                CursorIcon::Progress => c"progress",
            };
            let cursor = gdk_cursor_new_from_name(gdk_window_get_display(window), name.as_ptr());
            gdk_window_set_cursor(window, cursor);
            if !cursor.is_null() {
                g_object_unref(cursor);
            }
        }
    }

    /// Disconnect from the window and release the drawing area, before the window is.
    pub(super) fn disconnect(&self) {
        let data = self as *const Self as *mut c_void;
        for widget in [self.window.get().cast(), self.area.get()] {
            if !widget.is_null() {
                unsafe {
                    g_signal_handlers_disconnect_matched(
                        widget.cast(),
                        G_SIGNAL_MATCH_DATA,
                        0,
                        0,
                        null_mut(),
                        null_mut(),
                        data,
                    );
                }
            }
        }
        let area = self.area.replace(null_mut());
        if !area.is_null() {
            unsafe { g_object_unref(area.cast()) };
        }
    }

    // Whether the content reports the event: over the drawing area, or the window itself
    // without one, and not passed on by another window child.
    fn targets_content(&self, event: *const GdkEvent) -> bool {
        let widget = unsafe { gtk_get_event_widget(event) };
        if self.area.get().is_null() {
            widget == self.window.get().cast()
        } else {
            widget == self.area.get()
        }
    }

    // Whether keys go to the content: the drawing area or nothing has focus.
    fn has_key_focus(&self) -> bool {
        let focus = unsafe { gtk_window_get_focus(self.window.get()) };
        focus.is_null() || focus == self.area.get()
    }
}

unsafe fn connect(
    widget: *mut GtkWidget,
    signal: &std::ffi::CStr,
    callback: *const c_void,
    data: *const c_void,
    flags: i32,
) {
    unsafe {
        g_signal_connect_data(
            widget.cast(),
            signal.as_ptr(),
            callback,
            data,
            null(),
            flags,
        );
    }
}

extern "C" fn realize(_: *mut GtkWidget, content: &Content) {
    content.apply_cursor();
}

extern "C" fn pointer_event(_: *mut GtkWidget, event: *const GdkEvent, content: &Content) -> i32 {
    if !content.targets_content(event) {
        return 0;
    }
    if unsafe { gdk_event_get_event_type(event) } == GDK_BUTTON_PRESS
        && !content.area.get().is_null()
    {
        unsafe { gtk_widget_grab_focus(content.area.get()) };
    }
    match unsafe { gtk_input_event(event.cast(), false) } {
        Some(event) => {
            content.sender.send(event);
            1
        }
        None => 0,
    }
}

extern "C" fn key_event(_: *mut GtkWidget, event: *const GdkEvent, content: &Content) -> i32 {
    if !content.has_key_focus() {
        return 0;
    }
    let kind = unsafe { gdk_event_get_event_type(event) };
    let mut code = 0;
    unsafe { gdk_event_get_keycode(event, &mut code) };
    let repeat = match kind {
        GDK_KEY_PRESS => !content.keys.borrow_mut().insert(code),
        GDK_KEY_RELEASE => {
            content.keys.borrow_mut().remove(&code);
            false
        }
        _ => false,
    };
    match unsafe { gtk_input_event(event.cast(), repeat) } {
        Some(event) => {
            content.sender.send(event);
            1
        }
        None => 0,
    }
}

extern "C" fn focus_in(_: *mut GtkWidget, _: *const GdkEvent, content: &Content) -> i32 {
    content.sender.send(WindowEvent::Focus);
    0
}

extern "C" fn focus_out(_: *mut GtkWidget, _: *const GdkEvent, content: &Content) -> i32 {
    content.keys.borrow_mut().clear();
    content.sender.send(WindowEvent::Blur);
    0
}
