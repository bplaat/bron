/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The GTK media view: a GtkScrolledWindow with overlay scrollbars around a drawing area as large
//! as the magnified media, which draws it with bcanvas above a checkerboard.

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::ptr::null_mut;
use std::rc::{Rc, Weak};
use std::time::Instant;

use bcanvas::NativeContext;
use bwindow::{NativeViewHandle, Window, WindowAttachment};
use imageview_core::{
    Media, MediaRenderer, WHEEL_ZOOM_RATE, clamp_magnification, draw_checkerboard,
    fit_magnification,
};

use self::headers::*;
pub(crate) use self::thumbnail::thumbnail;

mod headers;
mod thumbnail;

// One scroll axis of the view.
#[derive(Clone, Copy)]
enum Axis {
    Horizontal,
    Vertical,
}

impl Axis {
    // Picks the component of a point or size along this axis.
    const fn of(self, (x, y): (f64, f64)) -> f64 {
        match self {
            Self::Horizontal => x,
            Self::Vertical => y,
        }
    }
}

struct State {
    renderer: RefCell<MediaRenderer>,
    magnification: Cell<f64>,
    // The shown animation frame, when the animation started and its timer source.
    frame: Cell<usize>,
    start: Cell<Instant>,
    timer: Cell<u32>,
    scrolled_window: *mut c_void,
    area: *mut c_void,
    gesture: *mut c_void,
    // The pointer position and scroll position where the media started being dragged.
    drag: Cell<Option<(f64, f64, f64, f64)>>,
    // The magnification where a pinch started.
    pinch: Cell<f64>,
}

/// A native scroll view that zooms and pans media.
pub(crate) struct MediaView {
    state: Rc<State>,
    _attachment: WindowAttachment,
}

impl MediaView {
    /// Fills the window with the media, zoomed to fit.
    pub(crate) fn new(window: &Window, media: Media, _theme: bwindow::Theme) -> Self {
        let attachment = window.attach_content().expect("window has no content yet");
        let size = window.size();
        let magnification = fit_magnification(
            (f64::from(size.width), f64::from(size.height)),
            media.size(),
        );
        // SAFETY: The created widgets are owned by this view until it drops them, the window
        // keeps the scrolled window it embeds alive while the attachment is open.
        let state = unsafe {
            let scrolled_window =
                g_object_ref_sink(gtk_scrolled_window_new(null_mut(), null_mut()));
            let area = g_object_ref(gtk_drawing_area_new());
            gtk_widget_add_events(
                area,
                GDK_BUTTON_PRESS_MASK
                    | GDK_BUTTON_RELEASE_MASK
                    | GDK_BUTTON1_MOTION_MASK
                    | GDK_SCROLL_MASK
                    | GDK_SMOOTH_SCROLL_MASK
                    | GDK_TOUCHPAD_GESTURE_MASK,
            );
            gtk_widget_set_can_focus(area, 1);
            gtk_container_add(scrolled_window, area);
            Rc::new(State {
                renderer: RefCell::new(MediaRenderer::new(media)),
                magnification: Cell::new(magnification),
                frame: Cell::new(0),
                start: Cell::new(Instant::now()),
                timer: Cell::new(0),
                scrolled_window,
                area,
                gesture: gtk_gesture_zoom_new(area),
                drag: Cell::new(None),
                pinch: Cell::new(1.0),
            })
        };
        state.connect();
        state.resize();
        state.start_animation();
        // SAFETY: The scrolled window is a live widget that the window lays out from now on.
        unsafe {
            attachment.set_native_view(NativeViewHandle::Gtk(state.scrolled_window));
            gtk_widget_grab_focus(state.area);
        }
        Self {
            state,
            _attachment: attachment,
        }
    }

    /// Replaces the media, keeping the zoom or fitting it to the window.
    pub(crate) fn set_media(&mut self, media: Media, zoom_to_fit: bool) {
        let state = &self.state;
        state.stop_animation();
        *state.renderer.borrow_mut() = MediaRenderer::new(media);
        state.frame.set(0);
        state.start.set(Instant::now());
        if zoom_to_fit {
            state.zoom_to_fit();
        } else {
            state.magnify(state.magnification.get(), None);
        }
        state.start_animation();
    }

    /// Magnifies by `factor`, keeping the middle of the visible area in place.
    pub(crate) fn zoom_by(&self, factor: f64) {
        self.state
            .magnify(self.state.magnification.get() * factor, None);
    }

    /// Shows the media at its natural size.
    pub(crate) fn actual_size(&self) {
        self.state.magnify(1.0, None);
        self.state.center();
    }

    /// Shows all of the media.
    pub(crate) fn zoom_to_fit(&self) {
        self.state.zoom_to_fit();
    }

    /// Redraws the checkerboard in the colors of the new theme.
    pub(crate) fn theme_changed(&self, _theme: bwindow::Theme) {
        // SAFETY: The drawing area is alive while this view is.
        unsafe { gtk_widget_queue_draw(self.state.area) };
    }
}

impl Drop for MediaView {
    fn drop(&mut self) {
        let state = &self.state;
        state.stop_animation();
        let data = Rc::as_ptr(state).cast::<c_void>();
        // SAFETY: This view owns a reference to each object, the handlers using the state are
        // disconnected before it is freed.
        unsafe {
            for object in [state.area, state.gesture] {
                g_signal_handlers_disconnect_matched(
                    object,
                    G_SIGNAL_MATCH_DATA,
                    0,
                    0,
                    null_mut(),
                    null_mut(),
                    data,
                );
            }
            g_object_unref(state.gesture);
            g_object_unref(state.area);
            g_object_unref(state.scrolled_window);
        }
    }
}

impl State {
    fn connect(&self) {
        let data = std::ptr::from_ref(self).cast::<c_void>();
        let signals: [(*mut c_void, &std::ffi::CStr, *const c_void); 7] = [
            (self.area, c"draw", draw as *const c_void),
            (self.area, c"scroll-event", scroll as *const c_void),
            (
                self.area,
                c"button-press-event",
                button_press as *const c_void,
            ),
            (
                self.area,
                c"button-release-event",
                button_release as *const c_void,
            ),
            (self.area, c"motion-notify-event", motion as *const c_void),
            (self.gesture, c"begin", pinch_begin as *const c_void),
            (
                self.gesture,
                c"scale-changed",
                pinch_changed as *const c_void,
            ),
        ];
        for (object, signal, handler) in signals {
            // SAFETY: The handlers match the signal signatures and the view disconnects them
            // before the state they receive is freed.
            unsafe { g_signal_connect_data(object, signal.as_ptr(), handler, data, null_mut(), 0) };
        }
    }

    fn adjustment(&self, axis: Axis) -> *mut c_void {
        // SAFETY: The scrolled window is alive and owns its adjustments.
        unsafe {
            match axis {
                Axis::Horizontal => gtk_scrolled_window_get_hadjustment(self.scrolled_window),
                Axis::Vertical => gtk_scrolled_window_get_vadjustment(self.scrolled_window),
            }
        }
    }

    // The visible length of an axis, the window size before the first layout.
    fn page(&self, axis: Axis) -> f64 {
        // SAFETY: The adjustment and scrolled window are alive.
        unsafe {
            let page = gtk_adjustment_get_page_size(self.adjustment(axis));
            if page > 0.0 {
                return page;
            }
            f64::from(match axis {
                Axis::Horizontal => gtk_widget_get_allocated_width(self.scrolled_window),
                Axis::Vertical => gtk_widget_get_allocated_height(self.scrolled_window),
            })
        }
    }

    // The magnified length of the media along an axis.
    fn content(&self, axis: Axis) -> f64 {
        self.magnification.get() * axis.of(self.renderer.borrow().media().size())
    }

    // Where the media starts inside the drawing area, it is centered when smaller than the view.
    fn offset(&self, axis: Axis) -> f64 {
        ((self.page(axis) - self.content(axis)) / 2.0).max(0.0)
    }

    // Sizes the drawing area to the magnified media.
    fn resize(&self) {
        let length = |axis| self.content(axis).ceil().min(f64::from(i32::MAX / 2)) as i32;
        // SAFETY: The drawing area is alive.
        unsafe {
            gtk_widget_set_size_request(
                self.area,
                length(Axis::Horizontal),
                length(Axis::Vertical),
            );
            gtk_widget_queue_draw(self.area);
        }
    }

    // Scrolls an axis, its range is updated before the next layout so the value is kept.
    fn scroll_to(&self, axis: Axis, value: f64) {
        let adjustment = self.adjustment(axis);
        let page = self.page(axis);
        let upper = self.content(axis).max(page);
        // SAFETY: The adjustment is alive.
        unsafe {
            gtk_adjustment_configure(
                adjustment,
                value.clamp(0.0, upper - page),
                0.0,
                upper,
                gtk_adjustment_get_step_increment(adjustment),
                gtk_adjustment_get_page_increment(adjustment),
                page,
            );
        }
    }

    // Magnifies the media, keeping the media under `anchor` in the drawing area in place, or
    // the middle of the visible area.
    fn magnify(&self, magnification: f64, anchor: Option<(f64, f64)>) {
        let before = self.magnification.get();
        let positions = [Axis::Horizontal, Axis::Vertical].map(|axis| {
            // SAFETY: The adjustment is alive.
            let value = unsafe { gtk_adjustment_get_value(self.adjustment(axis)) };
            let visible = anchor.map_or(self.page(axis) / 2.0, |anchor| axis.of(anchor) - value);
            (
                axis,
                visible,
                (value + visible - self.offset(axis)) / before,
            )
        });
        self.magnification.set(clamp_magnification(magnification));
        for (axis, visible, media) in positions {
            let after = self.magnification.get();
            self.scroll_to(axis, media * after + self.offset(axis) - visible);
        }
        self.resize();
    }

    // Scrolls to the middle of the media.
    fn center(&self) {
        for axis in [Axis::Horizontal, Axis::Vertical] {
            self.scroll_to(axis, (self.content(axis) - self.page(axis)) / 2.0);
        }
    }

    fn zoom_to_fit(&self) {
        let visible = (self.page(Axis::Horizontal), self.page(Axis::Vertical));
        let magnification = fit_magnification(visible, self.renderer.borrow().media().size());
        self.magnify(magnification, None);
        self.center();
    }

    // Shows the current animation frame and schedules the next one.
    fn start_animation(self: &Rc<Self>) {
        let (frame, next) = self
            .renderer
            .borrow()
            .media()
            .frame_at(self.start.get().elapsed());
        if self.frame.replace(frame) != frame {
            // SAFETY: The drawing area is alive while the state is.
            unsafe { gtk_widget_queue_draw(self.area) };
        }
        let Some(next) = next else {
            return;
        };
        let weak = Box::into_raw(Box::new(Rc::downgrade(self))).cast::<c_void>();
        // SAFETY: The timer owns the boxed weak state until GLib calls its destroy function.
        let timer = unsafe {
            g_timeout_add_full(
                0,
                next.as_millis().max(1) as u32,
                next_frame,
                weak,
                drop_weak,
            )
        };
        self.timer.set(timer);
    }

    fn stop_animation(&self) {
        let timer = self.timer.replace(0);
        if timer != 0 {
            // SAFETY: The timer is a pending source of this view.
            unsafe { g_source_remove(timer) };
        }
    }
}

extern "C" fn next_frame(data: *mut c_void) -> i32 {
    // SAFETY: GLib passes the boxed weak state of this timer, which lives until its destroy.
    let weak = unsafe { &*data.cast::<Weak<State>>() };
    if let Some(state) = weak.upgrade() {
        state.timer.set(0);
        state.start_animation();
    }
    // Every frame schedules its own timer with the delay of that frame.
    0
}

extern "C" fn drop_weak(data: *mut c_void) {
    // SAFETY: The timer owned the boxed weak state created in start_animation.
    drop(unsafe { Box::from_raw(data.cast::<Weak<State>>()) });
}

// The viewport caches what the drawing area draws beyond the visible area and reuses it while
// scrolling, so everything is drawn in drawing area coordinates, independent of the scroll
// position. The viewport sizes the area to at least the visible area, so media smaller than
// the view is centered in it.
extern "C" fn draw(area: *mut c_void, cr: *mut c_void, state: &State) -> i32 {
    let content = [Axis::Horizontal, Axis::Vertical].map(|axis| state.content(axis) as f32);
    // SAFETY: GTK passes the live drawing area and its cairo context while drawing.
    unsafe {
        let width = gtk_widget_get_allocated_width(area) as f32;
        let height = gtk_widget_get_allocated_height(area) as f32;
        let dark = is_dark(area);
        bcanvas::draw_native(
            NativeContext::Cairo(cr),
            width,
            height,
            gtk_widget_get_scale_factor(area) as f32,
            |canvas| {
                draw_checkerboard(canvas, width, height, dark);
                state.renderer.borrow().draw(
                    canvas,
                    state.frame.get(),
                    (
                        ((width - content[0]) / 2.0).max(0.0),
                        ((height - content[1]) / 2.0).max(0.0),
                        content[0],
                        content[1],
                    ),
                );
            },
        );
    }
    1
}

// Whether the theme is dark, which draws light text.
unsafe fn is_dark(widget: *mut c_void) -> bool {
    let mut color = GdkRgba::default();
    // SAFETY: The caller supplies a live widget, which owns its style context.
    unsafe {
        let context = gtk_widget_get_style_context(widget);
        gtk_style_context_get_color(context, gtk_style_context_get_state(context), &mut color);
    }
    color.red * 0.2126 + color.green * 0.7152 + color.blue * 0.0722 > 0.5
}

// Control + scroll magnifies around the pointer, other scrolls are left to the scrolled window.
extern "C" fn scroll(_: *mut c_void, event: *const c_void, state: &State) -> i32 {
    let (mut modifiers, mut direction, mut x, mut y, mut dx, mut dy) = (0, 0, 0.0, 0.0, 0.0, 0.0);
    // SAFETY: GTK passes a live scroll event.
    let lines = unsafe {
        gdk_event_get_state(event, &mut modifiers);
        gdk_event_get_coords(event, &mut x, &mut y);
        if gdk_event_get_scroll_deltas(event, &mut dx, &mut dy) != 0 {
            -dy
        } else if gdk_event_get_scroll_direction(event, &mut direction) != 0 {
            match direction {
                GDK_SCROLL_UP => 1.0,
                GDK_SCROLL_DOWN => -1.0,
                _ => 0.0,
            }
        } else {
            0.0
        }
    };
    if modifiers & GDK_CONTROL_MASK == 0 || lines == 0.0 {
        return 0;
    }
    state.magnify(
        state.magnification.get() * (lines * WHEEL_ZOOM_RATE).exp(),
        Some((x, y)),
    );
    1
}

// Dragging with the primary button pans the media.
extern "C" fn button_press(area: *mut c_void, event: *const c_void, state: &State) -> i32 {
    let (mut button, mut x, mut y) = (0, 0.0, 0.0);
    // SAFETY: GTK passes the live drawing area and a button event.
    unsafe {
        gtk_widget_grab_focus(area);
        if gdk_event_get_button(event, &mut button) == 0 || button != 1 {
            return 0;
        }
        gdk_event_get_root_coords(event, &mut x, &mut y);
        state.drag.set(Some((
            x,
            y,
            gtk_adjustment_get_value(state.adjustment(Axis::Horizontal)),
            gtk_adjustment_get_value(state.adjustment(Axis::Vertical)),
        )));
        let cursor = gdk_cursor_new_from_name(gtk_widget_get_display(area), c"grabbing".as_ptr());
        gdk_window_set_cursor(gtk_widget_get_window(area), cursor);
        if !cursor.is_null() {
            g_object_unref(cursor);
        }
    }
    1
}

extern "C" fn button_release(area: *mut c_void, _: *const c_void, state: &State) -> i32 {
    if state.drag.take().is_none() {
        return 0;
    }
    // SAFETY: GTK passes the live realized drawing area.
    unsafe { gdk_window_set_cursor(gtk_widget_get_window(area), null_mut()) };
    1
}

extern "C" fn motion(_: *mut c_void, event: *const c_void, state: &State) -> i32 {
    let Some((start_x, start_y, value_x, value_y)) = state.drag.get() else {
        return 0;
    };
    let (mut x, mut y) = (0.0, 0.0);
    // SAFETY: GTK passes a live motion event, the adjustments clamp their values.
    unsafe {
        gdk_event_get_root_coords(event, &mut x, &mut y);
        gtk_adjustment_set_value(state.adjustment(Axis::Horizontal), value_x - (x - start_x));
        gtk_adjustment_set_value(state.adjustment(Axis::Vertical), value_y - (y - start_y));
    }
    1
}

// Pinching magnifies around the middle of the fingers.
extern "C" fn pinch_begin(_: *mut c_void, _: *mut c_void, state: &State) {
    state.pinch.set(state.magnification.get());
}

extern "C" fn pinch_changed(gesture: *mut c_void, scale: f64, state: &State) {
    let (mut x, mut y) = (0.0, 0.0);
    // SAFETY: GTK passes the live zoom gesture.
    let anchor = unsafe { gtk_gesture_get_bounding_box_center(gesture, &mut x, &mut y) != 0 };
    state.magnify(state.pinch.get() * scale, anchor.then_some((x, y)));
}
