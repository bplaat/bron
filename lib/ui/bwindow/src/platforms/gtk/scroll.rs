/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The native scroll bars of a window: a vertical and a horizontal GtkScrollbar beside the
//! content area in its grid, so the content area with its layers narrows instead of being
//! covered. The area never moves, which would stall the frames of its layer subsurfaces. Each shows when the content overflows that direction. The wheel and touchpad scroll
//! them like a GtkScrolledWindow, with its step size and kinetic scrolling after a touchpad
//! swipe.

use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::null_mut;

use super::headers::*;
use crate::{LogicalPoint, LogicalSize, WindowEvent, WindowEventSender};

const GTK_ORIENTATION_HORIZONTAL: i32 = 0;
const GTK_ORIENTATION_VERTICAL: i32 = 1;
const GDK_SCROLL_UP: i32 = 0;
const GDK_SCROLL_DOWN: i32 = 1;
const GDK_SCROLL_LEFT: i32 = 2;
const GDK_SCROLL_RIGHT: i32 = 3;
// The distance of a scroll bar arrow in logical pixels.
const LINE: f64 = 40.0;
// Kinetic scrolling slows down with this time constant, and stops below this speed.
const KINETIC_TIME: f64 = 0.325;
const KINETIC_STOP: f64 = 30.0;

unsafe extern "C" {
    fn gtk_grid_attach(grid: *mut GtkWidget, child: *mut GtkWidget, x: i32, y: i32, w: i32, h: i32);
    fn gtk_scrollbar_new(orientation: i32, adjustment: *mut c_void) -> *mut GtkWidget;
    fn gtk_adjustment_new(
        value: f64,
        lower: f64,
        upper: f64,
        step: f64,
        page_increment: f64,
        page_size: f64,
    ) -> *mut c_void;
    fn gtk_adjustment_configure(
        adjustment: *mut c_void,
        value: f64,
        lower: f64,
        upper: f64,
        step: f64,
        page_increment: f64,
        page_size: f64,
    );
    fn gtk_adjustment_get_value(adjustment: *mut c_void) -> f64;
    fn gtk_adjustment_set_value(adjustment: *mut c_void, value: f64);
    fn gtk_adjustment_get_page_size(adjustment: *mut c_void) -> f64;
    fn gtk_adjustment_get_upper(adjustment: *mut c_void) -> f64;
    fn gtk_widget_get_visible(widget: *mut GtkWidget) -> i32;
    fn gtk_widget_set_visible(widget: *mut GtkWidget, visible: i32);
    fn gtk_widget_destroy(widget: *mut GtkWidget);
    fn gtk_widget_add_tick_callback(
        widget: *mut GtkWidget,
        callback: extern "C" fn(*mut GtkWidget, *mut c_void, *const c_void) -> i32,
        data: *const c_void,
        notify: extern "C" fn(*const c_void),
    ) -> u32;
    fn gtk_widget_remove_tick_callback(widget: *mut GtkWidget, id: u32);
    fn gdk_frame_clock_get_frame_time(clock: *mut c_void) -> i64;
    fn gdk_event_get_scroll_direction(event: *const GdkEvent, direction: *mut i32) -> i32;
    fn gdk_event_get_scroll_deltas(event: *const GdkEvent, x: *mut f64, y: *mut f64) -> i32;
    fn gdk_event_is_scroll_stop_event(event: *const GdkEvent) -> i32;
    fn gdk_event_get_time(event: *const GdkEvent) -> u32;
}

/// The scroll bars beside the content area of a window.
pub(super) struct Scroll {
    area: *mut GtkWidget,
    bars: [*mut GtkWidget; 2],
    adjustments: [*mut c_void; 2],
    sender: WindowEventSender,
    size: Cell<(f64, f64)>,
    // The touchpad speed in pixels per second and the time of its last delta, and the
    // kinetic scrolling with its last frame time.
    velocity: Cell<(f64, f64)>,
    last_delta: Cell<u32>,
    kinetic: Cell<Option<(u32, i64)>>,
}

impl Scroll {
    /// Add scroll bars beside the content area in its grid, it stays at its address.
    pub(super) fn new(
        grid: *mut GtkWidget,
        area: *mut GtkWidget,
        sender: WindowEventSender,
    ) -> Box<Self> {
        unsafe {
            let adjustments = [0, 1].map(|_| gtk_adjustment_new(0.0, 0.0, 0.0, LINE, 0.0, 0.0));
            let bars = [
                gtk_scrollbar_new(GTK_ORIENTATION_HORIZONTAL, adjustments[0]),
                gtk_scrollbar_new(GTK_ORIENTATION_VERTICAL, adjustments[1]),
            ];
            gtk_grid_attach(grid, bars[0], 0, 1, 1, 1);
            gtk_grid_attach(grid, bars[1], 1, 0, 1, 1);
            let scroll = Box::new(Self {
                area,
                bars,
                adjustments,
                sender,
                size: Cell::new((0.0, 0.0)),
                velocity: Cell::new((0.0, 0.0)),
                last_delta: Cell::new(0),
                kinetic: Cell::new(None),
            });
            let data = &*scroll as *const Self as *const c_void;
            for adjustment in adjustments {
                g_signal_connect_data(
                    adjustment.cast(),
                    c"value-changed".as_ptr(),
                    value_changed as *const c_void,
                    data,
                    null_mut(),
                    0,
                );
            }
            g_signal_connect_data(
                area.cast(),
                c"size-allocate".as_ptr(),
                allocated as *const c_void,
                data,
                null_mut(),
                1, // G_CONNECT_AFTER
            );
            scroll
        }
    }

    pub(super) fn offset(&self) -> LogicalPoint {
        let [x, y] = self
            .adjustments
            .map(|adjustment| unsafe { gtk_adjustment_get_value(adjustment) });
        LogicalPoint::new(x as f32, y as f32)
    }

    pub(super) fn set_size(&self, size: LogicalSize) {
        let size = (f64::from(size.width), f64::from(size.height));
        if self.size.replace(size) != size {
            self.update();
        }
    }

    /// Scroll to an offset, which the adjustments clamp to the content.
    pub(super) fn scroll_to(&self, offset: LogicalPoint) {
        self.stop_kinetic();
        for (adjustment, value) in self.adjustments.into_iter().zip([offset.x, offset.y]) {
            unsafe { gtk_adjustment_set_value(adjustment, f64::from(value)) };
        }
    }

    // Fit the adjustments and scroll bars to the content area.
    fn update(&self) {
        let (width, height) = self.size.get();
        let pages = unsafe {
            [
                f64::from(gtk_widget_get_allocated_width(self.area)),
                f64::from(gtk_widget_get_allocated_height(self.area)),
            ]
        };
        for ((adjustment, bar), (content, page)) in self
            .adjustments
            .into_iter()
            .zip(self.bars)
            .zip([width, height].into_iter().zip(pages))
        {
            // Changes lay out the window again, so only real ones.
            let upper = content.max(page);
            unsafe {
                if gtk_adjustment_get_upper(adjustment) != upper
                    || gtk_adjustment_get_page_size(adjustment) != page
                {
                    let value = gtk_adjustment_get_value(adjustment);
                    gtk_adjustment_configure(adjustment, value, 0.0, upper, LINE, page * 0.9, page);
                }
                let visible = content > page;
                if (gtk_widget_get_visible(bar) != 0) != visible {
                    gtk_widget_set_visible(bar, i32::from(visible));
                }
            }
        }
    }

    /// Scroll with a wheel or touchpad event, like a GtkScrolledWindow.
    pub(super) fn wheel(&self, event: *const GdkEvent) {
        let (mut direction, mut x, mut y) = (0, 0.0, 0.0);
        unsafe { gdk_event_get_scroll_direction(event, &mut direction) };
        let (dx, dy) = match direction {
            GDK_SCROLL_UP => (0.0, -1.0),
            GDK_SCROLL_DOWN => (0.0, 1.0),
            GDK_SCROLL_LEFT => (-1.0, 0.0),
            GDK_SCROLL_RIGHT => (1.0, 0.0),
            _ => {
                unsafe { gdk_event_get_scroll_deltas(event, &mut x, &mut y) };
                (x, y)
            }
        };
        self.stop_kinetic();
        // GtkScrolledWindow scrolls a unit of the page size to the power of two thirds.
        let [ux, uy] = self
            .adjustments
            .map(|adjustment| unsafe { gtk_adjustment_get_page_size(adjustment).powf(2.0 / 3.0) });
        let (px, py) = (dx * ux, dy * uy);
        self.scroll_by(px, py);
        // A touchpad swipe keeps scrolling with its speed when the fingers lift.
        let time = unsafe { gdk_event_get_time(event) };
        if unsafe { gdk_event_is_scroll_stop_event(event) } != 0 {
            let (vx, vy) = self.velocity.replace((0.0, 0.0));
            if vx.hypot(vy) > KINETIC_STOP {
                let data = self as *const Self as *const c_void;
                let id =
                    unsafe { gtk_widget_add_tick_callback(self.area, kinetic, data, no_notify) };
                self.velocity.set((vx, vy));
                self.kinetic.set(Some((id, 0)));
            }
        } else if direction > GDK_SCROLL_RIGHT {
            let elapsed =
                f64::from(time.wrapping_sub(self.last_delta.replace(time))).max(1.0) / 1000.0;
            if elapsed < 0.1 {
                // Smooth the speed over the last events.
                let (vx, vy) = self.velocity.get();
                self.velocity
                    .set(((vx + px / elapsed) / 2.0, (vy + py / elapsed) / 2.0));
            } else {
                self.velocity.set((0.0, 0.0));
            }
        }
    }

    fn scroll_by(&self, dx: f64, dy: f64) {
        for (adjustment, delta) in self.adjustments.into_iter().zip([dx, dy]) {
            unsafe {
                let value = gtk_adjustment_get_value(adjustment);
                gtk_adjustment_set_value(adjustment, value + delta);
            }
        }
    }

    fn stop_kinetic(&self) {
        if let Some((id, _)) = self.kinetic.take() {
            unsafe { gtk_widget_remove_tick_callback(self.area, id) };
        }
    }

    /// Remove the scroll bars, the content area fills the grid again.
    pub(super) fn remove(&self) {
        self.stop_kinetic();
        unsafe {
            let data = self as *const Self as *mut c_void;
            for instance in [self.adjustments[0], self.adjustments[1], self.area.cast()] {
                g_signal_handlers_disconnect_matched(
                    instance.cast(),
                    16,
                    0,
                    0,
                    null_mut(),
                    null_mut(),
                    data,
                );
            }
            for bar in self.bars {
                gtk_widget_destroy(bar);
            }
        }
    }
}

extern "C" fn value_changed(_: *mut c_void, scroll: &Scroll) {
    scroll.sender.send(WindowEvent::Scroll(scroll.offset()));
}

extern "C" fn allocated(_: *mut GtkWidget, _: *mut c_void, scroll: &Scroll) {
    scroll.update();
}

// Glide on after a touchpad swipe, slowing down every frame.
const extern "C" fn no_notify(_: *const c_void) {}

extern "C" fn kinetic(_: *mut GtkWidget, clock: *mut c_void, scroll: *const c_void) -> i32 {
    let scroll = unsafe { &*scroll.cast::<Scroll>() };
    let Some((id, last)) = scroll.kinetic.get() else {
        return 0;
    };
    let now = unsafe { gdk_frame_clock_get_frame_time(clock) };
    if last == 0 {
        scroll.kinetic.set(Some((id, now)));
        return 1;
    }
    let elapsed = (now - last) as f64 / 1_000_000.0;
    let (vx, vy) = scroll.velocity.get();
    scroll.scroll_by(vx * elapsed, vy * elapsed);
    let decay = (-elapsed / KINETIC_TIME).exp();
    let (vx, vy) = (vx * decay, vy * decay);
    scroll.velocity.set((vx, vy));
    if vx.hypot(vy) < KINETIC_STOP {
        scroll.kinetic.set(None);
        return 0;
    }
    scroll.kinetic.set(Some((id, now)));
    1
}
