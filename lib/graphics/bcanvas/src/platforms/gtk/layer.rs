/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// Draws a native window layer: the GDK window of the layer, a Wayland subsurface or an X11
// ARGB child window, painted with Cairo through GDK's draw frames and blended by the
// compositor. The window delivers the interaction and paces the animation frames.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::rc::{Rc, Weak};

use bwindow::{NativeContentHandle, WindowAttachment, WindowEventSender};

use super::headers::{cairo_paint, cairo_restore, cairo_save, cairo_set_operator};
use crate::CanvasRenderingContext2d;
use crate::platforms::gtk::context::PlatformCanvasContext;

const CAIRO_OPERATOR_CLEAR: i32 = 0;
const G_PRIORITY_HIGH_IDLE: i32 = 100;
const GDK_FRAME_CLOCK_PHASE_AFTER_PAINT: u32 = 1 << 6;

#[repr(C)]
struct CairoRectangleInt {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

unsafe extern "C" {
    fn gdk_window_get_width(window: *mut c_void) -> i32;
    fn gdk_window_get_height(window: *mut c_void) -> i32;
    fn gdk_window_get_scale_factor(window: *mut c_void) -> i32;
    fn gdk_window_begin_draw_frame(window: *mut c_void, region: *const c_void) -> *mut c_void;
    fn gdk_window_end_draw_frame(window: *mut c_void, context: *mut c_void);
    fn gdk_drawing_context_get_cairo_context(context: *mut c_void) -> *mut c_void;
    fn gdk_window_get_frame_clock(window: *mut c_void) -> *mut c_void;
    fn gdk_frame_clock_request_phase(clock: *mut c_void, phase: u32);
    fn cairo_region_create_rectangle(rectangle: *const CairoRectangleInt) -> *mut c_void;
    fn cairo_region_destroy(region: *mut c_void);
    fn g_idle_add_full(
        priority: i32,
        function: extern "C" fn(*mut c_void) -> i32,
        data: *mut c_void,
        notify: extern "C" fn(*mut c_void),
    ) -> u32;
}

struct LayerData {
    window: *mut c_void,
    sender: WindowEventSender,
    closed: Cell<bool>,
    painting: Cell<bool>,
    scheduled: Cell<bool>,
    frame: RefCell<Option<CanvasRenderingContext2d<'static>>>,
}

impl LayerData {
    fn request_redraw(self: &Rc<Self>) {
        if self.closed.get() {
            return;
        }
        // Redraws requested while painting wait for the next display frame.
        if self.painting.get() {
            self.sender.request_animation_frame();
        } else if !self.scheduled.replace(true) {
            let data = Box::into_raw(Box::new(Rc::downgrade(self))).cast();
            unsafe { g_idle_add_full(G_PRIORITY_HIGH_IDLE, paint_idle, data, drop_idle) };
        }
    }

    fn paint(&self) {
        self.scheduled.set(false);
        if self.closed.get() || self.painting.replace(true) {
            return;
        }
        unsafe {
            let (width, height) = (
                gdk_window_get_width(self.window),
                gdk_window_get_height(self.window),
            );
            let scale = gdk_window_get_scale_factor(self.window).max(1);
            let region = cairo_region_create_rectangle(&CairoRectangleInt {
                x: 0,
                y: 0,
                width,
                height,
            });
            let drawing = gdk_window_begin_draw_frame(self.window, region);
            cairo_region_destroy(region);
            let cr = gdk_drawing_context_get_cairo_context(drawing);
            // Every frame starts transparent, the layers below show through.
            cairo_save(cr);
            cairo_set_operator(cr, CAIRO_OPERATOR_CLEAR);
            cairo_paint(cr);
            cairo_restore(cr);
            *self.frame.borrow_mut() = Some(CanvasRenderingContext2d::new(
                PlatformCanvasContext::new(cr),
                width as f32,
                height as f32,
                scale as f32,
            ));
            let end = || {
                self.frame.borrow_mut().take();
                gdk_window_end_draw_frame(self.window, drawing);
                // GDK commits the buffer of a window in the after paint phase of its clock.
                gdk_frame_clock_request_phase(
                    gdk_window_get_frame_clock(self.window),
                    GDK_FRAME_CLOCK_PHASE_AFTER_PAINT,
                );
            };
            if !self.sender.redraw(end) {
                self.frame.borrow_mut().take();
                gdk_window_end_draw_frame(self.window, drawing);
                self.sender.request_animation_frame();
            }
        }
        self.painting.set(false);
    }
}

extern "C" fn paint_idle(data: *mut c_void) -> i32 {
    let data = unsafe { &*data.cast::<Weak<LayerData>>() };
    if let Some(data) = data.upgrade() {
        data.paint();
    }
    0
}

extern "C" fn drop_idle(data: *mut c_void) {
    drop(unsafe { Box::from_raw(data.cast::<Weak<LayerData>>()) });
}

pub(crate) struct PlatformLayerCanvas {
    data: Rc<LayerData>,
    attachment: WindowAttachment,
}

impl PlatformLayerCanvas {
    pub(crate) fn new(attachment: WindowAttachment) -> Self {
        let Some(NativeContentHandle::GdkLayer(window)) = (unsafe { attachment.content_handle() })
        else {
            panic!("expected a GDK layer window");
        };
        let data = Rc::new(LayerData {
            window,
            sender: attachment.event_sender(),
            closed: Cell::new(false),
            painting: Cell::new(false),
            scheduled: Cell::new(false),
            frame: RefCell::new(None),
        });
        let redraw = Rc::downgrade(&data);
        attachment.on_redraw(move || {
            if let Some(data) = redraw.upgrade() {
                data.request_redraw();
            }
        });
        let resize = Rc::downgrade(&data);
        attachment.on_resize(move |_, _| {
            if let Some(data) = resize.upgrade() {
                data.request_redraw();
            }
        });
        let close = Rc::downgrade(&data);
        attachment.on_close(move || {
            if let Some(data) = close.upgrade() {
                data.closed.set(true);
                data.frame.borrow_mut().take();
            }
        });
        data.request_redraw();
        Self { data, attachment }
    }

    pub(crate) fn request_redraw(&self) {
        self.data.request_redraw();
    }

    pub(crate) fn request_animation_frame(&self) {
        self.attachment.request_animation_frame();
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

impl Drop for PlatformLayerCanvas {
    fn drop(&mut self) {
        self.attachment.disconnect();
        self.data.closed.set(true);
        self.data.frame.borrow_mut().take();
    }
}
