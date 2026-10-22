/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::rc::Rc;

use bwindow::{NativeContentHandle, WindowAttachment, WindowEventSender};

use crate::CanvasRenderingContext2d;
use crate::platforms::gtk::context::PlatformCanvasContext;

#[repr(C)]
struct GObject([u8; 0]);
#[repr(C)]
struct GtkWidget([u8; 0]);

const G_CONNECT_DEFAULT: i32 = 0;

unsafe extern "C" {
    fn g_signal_connect_data(
        instance: *mut GObject,
        detailed_signal: *const std::ffi::c_char,
        c_handler: *const c_void,
        data: *const c_void,
        destroy_data: *const c_void,
        connect_flags: i32,
    );
    fn g_signal_handlers_disconnect_matched(
        instance: *mut GObject,
        mask: u32,
        signal_id: u32,
        detail: u32,
        closure: *mut c_void,
        func: *mut c_void,
        data: *mut c_void,
    ) -> u32;
    fn g_object_ref(object: *mut GObject) -> *mut GObject;
    fn g_object_unref(object: *mut GObject);
    fn gtk_widget_get_allocated_width(widget: *mut GtkWidget) -> i32;
    fn gtk_widget_get_allocated_height(widget: *mut GtkWidget) -> i32;
    fn gtk_widget_get_scale_factor(widget: *mut GtkWidget) -> i32;
    fn gtk_widget_queue_draw(widget: *mut GtkWidget);
    fn gtk_widget_add_tick_callback(
        widget: *mut GtkWidget,
        callback: extern "C" fn(*mut GtkWidget, *mut c_void, *mut c_void) -> i32,
        data: *mut c_void,
        notify: extern "C" fn(*mut c_void),
    ) -> u32;
    fn gtk_widget_remove_tick_callback(widget: *mut GtkWidget, id: u32);
}

// Draws the shared drawing area of the window with Cairo in its draw signal, the window
// delivers the interaction. Animation frames follow the GTK frame clock.
struct CanvasData {
    widget: *mut GtkWidget,
    sender: WindowEventSender,
    closed: Cell<bool>,
    painting: Cell<bool>,
    frame: RefCell<Option<CanvasRenderingContext2d<'static>>>,
    source: Cell<Option<u32>>,
}

impl CanvasData {
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
        if self.closed.get() || self.source.get().is_some() {
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
            // The window paints its background again.
            gtk_widget_queue_draw(self.widget);
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
        let Some(NativeContentHandle::Gtk(widget)) = (unsafe { attachment.content_handle() })
        else {
            panic!("expected an open GTK window");
        };
        unsafe {
            let widget = widget.cast::<GtkWidget>();
            g_object_ref(widget.cast());
            let data = Rc::new(CanvasData {
                widget,
                sender: attachment.event_sender(),
                closed: Cell::new(false),
                painting: Cell::new(false),
                frame: RefCell::new(None),
                source: Cell::new(None),
            });
            g_signal_connect_data(
                widget.cast(),
                c"draw".as_ptr(),
                canvas_draw as *const c_void,
                Rc::as_ptr(&data).cast(),
                null(),
                G_CONNECT_DEFAULT,
            );
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
