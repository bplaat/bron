/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::ffi::{c_char, c_void};
use std::ptr::{null, null_mut};
use std::rc::Rc;

use bwindow::ffi::*;
use bwindow::{NativeWindowHandle, WindowAttachment, WindowEvent, WindowEventSender};

use crate::CanvasRenderingContext2d;
use crate::platforms::gtk::context::PlatformCanvasContext;

unsafe extern "C" {
    fn gdk_window_get_display(window: *mut c_void) -> *mut c_void;
    fn gdk_cursor_new_from_name(display: *mut c_void, name: *const c_char) -> *mut GObject;
    fn gdk_window_set_cursor(window: *mut c_void, cursor: *mut GObject);
    fn gtk_drawing_area_new() -> *mut GtkWidget;
    fn gtk_widget_set_can_focus(widget: *mut GtkWidget, can_focus: i32);
    fn gtk_widget_grab_focus(widget: *mut GtkWidget);
    fn gtk_widget_add_events(widget: *mut GtkWidget, events: i32);
    fn gtk_widget_queue_draw(widget: *mut GtkWidget);
    fn gtk_widget_add_tick_callback(
        widget: *mut GtkWidget,
        callback: extern "C" fn(*mut GtkWidget, *mut c_void, *mut c_void) -> i32,
        data: *mut c_void,
        notify: extern "C" fn(*mut c_void),
    ) -> u32;
    fn gtk_widget_remove_tick_callback(widget: *mut GtkWidget, id: u32);
    fn g_object_ref_sink(object: *mut GObject) -> *mut GObject;
}

struct CanvasData {
    cursor: Cell<crate::CursorIcon>,
    widget: *mut GtkWidget,
    sender: WindowEventSender,
    closed: Cell<bool>,
    painting: Cell<bool>,
    frame: RefCell<Option<CanvasRenderingContext2d<'static>>>,
    source: Cell<Option<u32>>,
    keys: RefCell<HashSet<u16>>,
}

impl CanvasData {
    fn apply_cursor(&self) {
        if self.closed.get() {
            return;
        }
        unsafe {
            let window = gtk_widget_get_window(self.widget);
            if window.is_null() {
                return;
            }
            let name = match self.cursor.get() {
                crate::CursorIcon::Default => c"default",
                crate::CursorIcon::Pointer => c"pointer",
                crate::CursorIcon::Progress => c"progress",
            };
            let native = gdk_cursor_new_from_name(gdk_window_get_display(window), name.as_ptr());
            gdk_window_set_cursor(window, native);
            if !native.is_null() {
                g_object_unref(native);
            }
        }
    }

    fn request_redraw(self: &Rc<Self>) {
        if self.closed.get() {
            return;
        }
        if !self.painting.get() {
            if let Some(source) = self.source.take() {
                unsafe { gtk_widget_remove_tick_callback(self.widget, source) };
            }
            unsafe { gtk_widget_queue_draw(self.widget) };
            return;
        }
        self.request_animation_frame();
    }

    fn request_animation_frame(self: &Rc<Self>) {
        if self.closed.get() {
            return;
        }
        if self.source.get().is_some() {
            return;
        }
        let pointer = Rc::into_raw(self.clone()) as *mut c_void;
        let source =
            unsafe { gtk_widget_add_tick_callback(self.widget, redraw, pointer, release_source) };
        self.source.set(Some(source));
    }

    fn close(&self) {
        if self.closed.replace(true) {
            return;
        }
        if let Some(source) = self.source.take() {
            unsafe { gtk_widget_remove_tick_callback(self.widget, source) };
        }
        self.frame.borrow_mut().take();
        unsafe {
            g_signal_handlers_disconnect_matched(
                self.widget.cast(),
                16,
                0,
                0,
                null_mut(),
                null_mut(),
                self as *const Self as *mut c_void,
            );
            gtk_widget_destroy(self.widget);
        }
    }

    unsafe fn retain(data: *const Self) -> Rc<Self> {
        unsafe {
            Rc::increment_strong_count(data);
            Rc::from_raw(data)
        }
    }
}

impl Drop for CanvasData {
    fn drop(&mut self) {
        unsafe { g_object_unref(self.widget.cast()) };
    }
}

extern "C" fn redraw(_: *mut GtkWidget, _: *mut c_void, pointer: *mut c_void) -> i32 {
    let data = unsafe { &*pointer.cast::<CanvasData>() };
    data.source.set(None);
    if !data.closed.get() {
        unsafe { gtk_widget_queue_draw(data.widget) };
    }
    0
}

extern "C" fn release_source(pointer: *mut c_void) {
    drop(unsafe { Rc::from_raw(pointer.cast::<CanvasData>()) });
}

pub(crate) struct PlatformCanvas {
    data: Rc<CanvasData>,
    attachment: WindowAttachment,
}

impl PlatformCanvas {
    pub(crate) fn new(attachment: WindowAttachment) -> Self {
        let Some(NativeWindowHandle::Gtk(window)) = (unsafe { attachment.native_handle() }) else {
            panic!("expected an open GTK window");
        };
        unsafe {
            let widget = gtk_drawing_area_new();
            g_object_ref_sink(widget.cast());
            gtk_widget_set_can_focus(widget, 1);
            // Motion, buttons, enter/leave, keys, focus, discrete and smooth scroll.
            gtk_widget_add_events(
                widget,
                (1 << 2)
                    | (1 << 8)
                    | (1 << 9)
                    | (1 << 10)
                    | (1 << 11)
                    | (1 << 12)
                    | (1 << 13)
                    | (1 << 14)
                    | (1 << 21)
                    | (1 << 23),
            );
            let data = Rc::new(CanvasData {
                cursor: Cell::new(crate::CursorIcon::Default),
                widget,
                sender: attachment.event_sender(),
                closed: Cell::new(false),
                painting: Cell::new(false),
                frame: RefCell::new(None),
                source: Cell::new(None),
                keys: RefCell::new(HashSet::new()),
            });
            let pointer = Rc::as_ptr(&data).cast::<c_void>();
            for (name, callback) in [
                (c"realize", canvas_realize as *const c_void),
                (c"draw", canvas_draw as *const c_void),
                (c"event", input_event as *const c_void),
                (c"focus-in-event", focus_in as *const c_void),
                (c"focus-out-event", focus_out as *const c_void),
            ] {
                g_signal_connect_data(
                    widget.cast(),
                    name.as_ptr(),
                    callback,
                    pointer,
                    null(),
                    G_CONNECT_DEFAULT,
                );
            }
            #[cfg(feature = "file_drop")]
            if attachment.allow_file_drop() {
                gtk_connect_file_drop(widget, attachment.event_sender());
            }
            gtk_container_add(window.cast(), widget);
            let redraw = Rc::downgrade(&data);
            attachment.on_redraw(move || {
                if let Some(data) = redraw.upgrade() {
                    data.request_redraw();
                }
            });
            let animation_frame = Rc::downgrade(&data);
            attachment.on_animation_frame(move || {
                if let Some(data) = animation_frame.upgrade() {
                    data.request_animation_frame();
                }
            });
            let close = data.clone();
            attachment.on_close(move || close.close());
            gtk_widget_show(widget);
            gtk_widget_grab_focus(widget);
            data.request_redraw();
            Self { data, attachment }
        }
    }

    pub(crate) fn request_redraw(&self) {
        self.data.request_redraw();
    }

    pub(crate) fn request_animation_frame(&self) {
        self.data.request_animation_frame();
    }

    pub(crate) fn set_cursor(&mut self, cursor: crate::CursorIcon) {
        if !self.data.closed.get() && self.data.cursor.replace(cursor) != cursor {
            self.data.apply_cursor();
        }
    }

    pub(crate) fn draw(
        &mut self,
        draw: impl for<'frame> FnOnce(&mut CanvasRenderingContext2d<'frame>),
    ) -> bool {
        if self.attachment.is_closed() {
            return false;
        }
        let frame = self.data.frame.borrow_mut().take();
        let Some(mut frame) = frame else { return false };
        draw(&mut frame);
        true
    }
}

impl Drop for PlatformCanvas {
    fn drop(&mut self) {
        self.attachment.disconnect();
        self.data.close();
    }
}

extern "C" fn canvas_realize(_: *mut GtkWidget, data: &CanvasData) {
    data.apply_cursor();
}

extern "C" fn canvas_draw(
    widget: *mut GtkWidget,
    cr: *mut c_void,
    pointer: *const CanvasData,
) -> i32 {
    let data = unsafe { CanvasData::retain(pointer) };
    if data.closed.get() {
        return 0;
    }
    if data.painting.replace(true) {
        data.request_redraw();
        return 0;
    }
    *data.frame.borrow_mut() = Some(CanvasRenderingContext2d::new(
        unsafe { PlatformCanvasContext::new(cr) },
        unsafe { gtk_widget_get_allocated_width(widget) } as f32,
        unsafe { gtk_widget_get_allocated_height(widget) } as f32,
        unsafe { gtk_widget_get_scale_factor(widget) } as f32,
    ));
    if !data.sender.redraw(|| {
        data.frame.borrow_mut().take();
    }) {
        data.frame.borrow_mut().take();
        data.request_redraw();
    }
    data.painting.set(false);
    1
}

extern "C" fn input_event(
    widget: *mut GtkWidget,
    event: *const GdkEvent,
    pointer: *const CanvasData,
) -> i32 {
    let data = unsafe { CanvasData::retain(pointer) };
    if data.closed.get() {
        return 0;
    }
    let kind = unsafe { gdk_event_get_event_type(event) };
    let repeat = if kind == 8 || kind == 9 {
        let mut code = 0;
        unsafe { gdk_event_get_keycode(event, &mut code) };
        if kind == 8 {
            !data.keys.borrow_mut().insert(code)
        } else {
            data.keys.borrow_mut().remove(&code);
            false
        }
    } else {
        false
    };
    if kind == 4 {
        unsafe { gtk_widget_grab_focus(widget) };
    }
    if let Some(event) = unsafe { gtk_input_event(event, repeat) } {
        data.sender.send(event);
        return 1;
    }
    0
}

extern "C" fn focus_in(_: *mut GtkWidget, _: *const GdkEvent, pointer: *const CanvasData) -> i32 {
    let data = unsafe { CanvasData::retain(pointer) };
    data.sender.send(WindowEvent::Focus);
    0
}

extern "C" fn focus_out(_: *mut GtkWidget, _: *const GdkEvent, pointer: *const CanvasData) -> i32 {
    let data = unsafe { CanvasData::retain(pointer) };
    data.keys.borrow_mut().clear();
    data.sender.send(WindowEvent::Blur);
    0
}
