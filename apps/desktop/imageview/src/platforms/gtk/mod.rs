/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The GTK media view: a native GtkScrolledWindow around a drawing area as large as the
//! magnified media, which draws the viewport with bcanvas, in an overlay that shows a spinner
//! while the media loads. The scrolled window scrolls the
//! drawing area, its scroll position is followed by the viewport and the other way around.

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::ptr::null_mut;
use std::rc::{Rc, Weak};

use bcanvas::NativeContext;
use bwindow::{NativeViewHandle, Theme, Window, WindowAttachment};
use imageview_core::{Media, WHEEL_ZOOM_RATE};

use self::headers::*;
pub(crate) use self::thumbnail::thumbnail;
use crate::viewport::{Axis, LINE, Step, Viewport};

mod headers;
mod thumbnail;
mod toolbar;

pub(crate) use toolbar::{init, uses_header_bar};

/// The smallest window.
pub(crate) const MIN_SIZE: (f32, f32) = (240.0, 180.0);

struct State {
    viewport: RefCell<Viewport>,
    dark: Cell<bool>,
    // The overlay of the scrolled window and the loading spinner, which the window embeds.
    overlay: *mut c_void,
    header_bar: *mut c_void,
    integrated_toolbar: bool,
    scrolled_window: *mut c_void,
    // The spinner shown until the media is decoded, or null.
    spinner: Cell<*mut c_void>,
    area: *mut c_void,
    gesture: *mut c_void,
    // Whether the adjustments are being set from the viewport.
    updating: Cell<bool>,
    // The scale of the current pinch so far and the timer of the next animation frame.
    pinch: Cell<f64>,
    timer: Cell<u32>,
}

/// A native view that zooms and pans media.
pub(crate) struct MediaView {
    state: Rc<State>,
    _attachment: WindowAttachment,
}

impl MediaView {
    /// Fills the window with the media zoomed to fit, or a loading spinner until the media is
    /// set.
    pub(crate) fn new(window: &Window, media: Option<Media>, theme: Theme) -> Self {
        let loading = media.is_none();
        let attachment = window.attach_content().expect("window has no content yet");
        let size = window.size();
        let integrated_toolbar = uses_header_bar();
        // SAFETY: The attachment keeps the GTK window alive on this thread.
        let header_bar = unsafe {
            let Some(bwindow::NativeWindowHandle::Gtk(window)) = attachment.window_handle() else {
                unreachable!("GTK view requires a GTK window");
            };
            let bar = toolbar::install(window, integrated_toolbar);
            if integrated_toolbar && let Some(media) = &media {
                toolbar::show_size(bar, media);
            }
            bar
        };
        // SAFETY: The created widgets are owned by this view until it drops them, the window
        // keeps the overlay it embeds alive while the attachment is open.
        let state = unsafe {
            let overlay = g_object_ref_sink(gtk_overlay_new());
            let scrolled_window =
                g_object_ref_sink(gtk_scrolled_window_new(null_mut(), null_mut()));
            gtk_container_add(overlay, scrolled_window);
            let spinner = if loading {
                let spinner = gtk_spinner_new();
                gtk_widget_set_halign(spinner, GTK_ALIGN_CENTER);
                gtk_widget_set_valign(spinner, GTK_ALIGN_CENTER);
                gtk_widget_set_size_request(spinner, 32, 32);
                gtk_overlay_add_overlay(overlay, spinner);
                gtk_overlay_set_overlay_pass_through(overlay, spinner, 1);
                gtk_spinner_start(spinner);
                spinner
            } else {
                null_mut()
            };
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
                viewport: RefCell::new(Viewport::new(
                    media,
                    (f64::from(size.width), f64::from(size.height)),
                )),
                dark: Cell::new(theme == Theme::Dark),
                overlay,
                header_bar,
                integrated_toolbar,
                scrolled_window,
                spinner: Cell::new(spinner),
                area,
                gesture: gtk_gesture_zoom_new(area),
                updating: Cell::new(false),
                pinch: Cell::new(1.0),
                timer: Cell::new(0),
            })
        };
        state.connect();
        // SAFETY: The overlay is a live widget that the window lays out from now on.
        unsafe {
            let content = if integrated_toolbar {
                state.overlay
            } else {
                let content = gtk_box_new(GTK_ORIENTATION_VERTICAL, 0);
                gtk_box_pack_start(content, header_bar, 0, 0, 0);
                gtk_box_pack_start(content, state.overlay, 1, 1, 0);
                content
            };
            attachment.set_native_view(NativeViewHandle::Gtk(content));
            // As on macOS and Windows, preserve the media area requested by the builder.
            // An inline toolbar needs extra client height; a titlebar lives outside it.
            // Measure GTK's requisition so larger themes and fonts are accounted for.
            let mut toolbar_width = 0;
            let mut toolbar_height = 0;
            gtk_widget_get_preferred_width(header_bar, &mut toolbar_width, null_mut());
            if !integrated_toolbar {
                gtk_widget_get_preferred_height(header_bar, null_mut(), &mut toolbar_height);
            }
            let width = (size.width.round() as i32).max(toolbar_width);
            let height = size.height.round() as i32 + toolbar_height;
            let Some(bwindow::NativeWindowHandle::Gtk(window)) = attachment.window_handle() else {
                unreachable!("GTK view requires a GTK window");
            };
            gtk_window_set_default_size(window, width, height);
            gtk_window_resize(window, width, height);
            gtk_widget_grab_focus(state.area);
        }
        state.update(Viewport::zoom_to_fit);
        state.start_animation();
        Self {
            state,
            _attachment: attachment,
        }
    }

    /// Replaces the media, keeping the zoom or fitting it to the window.
    pub(crate) fn set_media(&mut self, media: Media, zoom_to_fit: bool) {
        self.state.stop_animation();
        if self.state.integrated_toolbar {
            // SAFETY: The window owns the header bar while this view is attached.
            unsafe { toolbar::show_size(self.state.header_bar, &media) };
        }
        self.state
            .update(|viewport| viewport.set_media(media, zoom_to_fit));
        self.state.start_animation();
        let spinner = self.state.spinner.replace(null_mut());
        if !spinner.is_null() {
            // SAFETY: The spinner is a live child of the overlay, which drops it.
            unsafe { gtk_widget_destroy(spinner) };
        }
    }

    /// Magnifies by `factor`, keeping the middle of the visible area in place.
    pub(crate) fn zoom_by(&self, factor: f64) {
        self.state.update(|viewport| viewport.zoom_by(factor, None));
    }

    /// Shows the media at its natural size.
    pub(crate) fn actual_size(&self) {
        self.state.update(Viewport::actual_size);
    }

    /// Shows all of the media.
    pub(crate) fn zoom_to_fit(&self) {
        self.state.update(Viewport::zoom_to_fit);
    }

    /// Redraws the checkerboard in the colors of the new theme.
    pub(crate) fn theme_changed(&self, theme: Theme) {
        self.state.dark.set(theme == Theme::Dark);
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
            for object in [
                state.area,
                state.gesture,
                state.scrolled_window,
                state.adjustment(Axis::Horizontal),
                state.adjustment(Axis::Vertical),
            ] {
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
            g_object_unref(state.overlay);
        }
    }
}

impl State {
    fn connect(&self) {
        let data = std::ptr::from_ref(self).cast::<c_void>();
        let signals: [(*mut c_void, &std::ffi::CStr, *const c_void); 11] = [
            (self.area, c"draw", draw as *const c_void),
            (self.area, c"scroll-event", scroll as *const c_void),
            (self.area, c"key-press-event", key_press as *const c_void),
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
            (self.gesture, c"scale-changed", pinch as *const c_void),
            (
                self.scrolled_window,
                c"size-allocate",
                resized as *const c_void,
            ),
            (
                self.adjustment(Axis::Horizontal),
                c"value-changed",
                scrolled as *const c_void,
            ),
            (
                self.adjustment(Axis::Vertical),
                c"value-changed",
                scrolled as *const c_void,
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

    // The scroll position of the scrolled window.
    fn scroll(&self) -> (f64, f64) {
        // SAFETY: The adjustments are alive.
        unsafe {
            (
                gtk_adjustment_get_value(self.adjustment(Axis::Horizontal)),
                gtk_adjustment_get_value(self.adjustment(Axis::Vertical)),
            )
        }
    }

    // A point in the drawing area in the visible area.
    fn visible_point(&self, (x, y): (f64, f64)) -> (f64, f64) {
        let scroll = self.scroll();
        (x - scroll.0, y - scroll.1)
    }

    // Changes the viewport, then sizes the drawing area to the magnified media and scrolls the
    // scrolled window along. The ranges are set before the next layout, so the scroll position
    // is kept.
    fn update(&self, change: impl FnOnce(&mut Viewport)) {
        let (content, bars) = {
            let mut viewport = self.viewport.borrow_mut();
            change(&mut viewport);
            (
                viewport.content(),
                Axis::BOTH.map(|axis| viewport.scroll_bar(axis)),
            )
        };
        self.updating.set(true);
        // SAFETY: The drawing area and adjustments are alive.
        unsafe {
            // The scrolled window grows the drawing area to at least the visible area.
            let length = |length: f64| length.ceil().min(f64::from(i32::MAX / 2)) as i32;
            gtk_widget_set_size_request(self.area, length(content.0), length(content.1));
            for (axis, bar) in Axis::BOTH.into_iter().zip(bars) {
                let adjustment = self.adjustment(axis);
                gtk_adjustment_configure(
                    adjustment,
                    bar.position,
                    0.0,
                    bar.length,
                    LINE,
                    bar.page,
                    bar.page,
                );
            }
            gtk_widget_queue_draw(self.area);
        }
        self.updating.set(false);
    }

    // Shows the current animation frame and schedules the next one.
    fn start_animation(self: &Rc<Self>) {
        let (changed, next) = self.viewport.borrow_mut().advance_frame();
        if changed {
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

// The scrolled window caches what the drawing area draws beyond the visible area and reuses it
// while scrolling, so it is drawn as a document independent of the scroll position.
extern "C" fn draw(area: *mut c_void, cr: *mut c_void, state: &State) -> i32 {
    // SAFETY: GTK passes the live drawing area and its cairo context while drawing.
    unsafe {
        bcanvas::draw_native(
            NativeContext::Cairo(cr),
            gtk_widget_get_allocated_width(area) as f32,
            gtk_widget_get_allocated_height(area) as f32,
            gtk_widget_get_scale_factor(area) as f32,
            |canvas| {
                state
                    .viewport
                    .borrow()
                    .paint_document(canvas, state.dark.get());
            },
        );
    }
    1
}

// The visible area changed with the scrolled window.
extern "C" fn resized(scrolled_window: *mut c_void, _: *mut c_void, state: &State) {
    // SAFETY: GTK passes the live scrolled window.
    let visible = unsafe {
        (
            f64::from(gtk_widget_get_allocated_width(scrolled_window)),
            f64::from(gtk_widget_get_allocated_height(scrolled_window)),
        )
    };
    let mut viewport = state.viewport.borrow_mut();
    viewport.resize(visible);
    viewport.scroll_to(state.scroll());
}

// The scrolled window scrolled by itself.
extern "C" fn scrolled(_: *mut c_void, state: &State) {
    if !state.updating.get() {
        state.viewport.borrow_mut().scroll_to(state.scroll());
    }
}

// Control + scroll magnifies around the pointer, the scrolled window scrolls the others.
extern "C" fn scroll(_: *mut c_void, event: *const c_void, state: &State) -> i32 {
    let (mut modifiers, mut direction) = (0, 0);
    let (mut x, mut y, mut dx, mut dy) = (0.0, 0.0, 0.0, 0.0);
    // SAFETY: GTK passes a live scroll event.
    unsafe {
        gdk_event_get_state(event, &mut modifiers);
        gdk_event_get_coords(event, &mut x, &mut y);
        if gdk_event_get_scroll_deltas(event, &mut dx, &mut dy) == 0
            && gdk_event_get_scroll_direction(event, &mut direction) != 0
        {
            dy = match direction {
                GDK_SCROLL_UP => -1.0,
                GDK_SCROLL_DOWN => 1.0,
                _ => 0.0,
            };
        }
    }
    if modifiers & GDK_CONTROL_MASK == 0 || dy == 0.0 {
        return 0;
    }
    let anchor = state.visible_point((x, y));
    state.update(|viewport| viewport.zoom_by((-dy * WHEEL_ZOOM_RATE).exp(), Some(anchor)));
    1
}

extern "C" fn key_press(_: *mut c_void, event: *const c_void, state: &State) -> i32 {
    let mut keyval = 0;
    // SAFETY: GTK passes a live key event.
    unsafe { gdk_event_get_keyval(event, &mut keyval) };
    let step = match keyval {
        GDK_KEY_UP => Step::LineBack,
        GDK_KEY_DOWN => Step::LineForward,
        GDK_KEY_PAGE_UP => Step::PageBack,
        GDK_KEY_PAGE_DOWN => Step::PageForward,
        GDK_KEY_HOME => Step::Start,
        GDK_KEY_END => Step::End,
        _ => return 0,
    };
    state.update(|viewport| viewport.step(Axis::Vertical, step));
    1
}

// Dragging with the primary button pans the media, the screen position does not move with it.
extern "C" fn button_press(area: *mut c_void, event: *const c_void, state: &State) -> i32 {
    let (mut button, mut x, mut y) = (0, 0.0, 0.0);
    // SAFETY: GTK passes the live drawing area and a button event.
    unsafe {
        gtk_widget_grab_focus(area);
        if gdk_event_get_button(event, &mut button) == 0 || button != 1 {
            return 0;
        }
        gdk_event_get_root_coords(event, &mut x, &mut y);
    }
    if state.viewport.borrow_mut().drag_begin((x, y)) {
        // SAFETY: GTK passes the live realized drawing area.
        unsafe { set_cursor(area, c"grabbing".as_ptr()) };
    }
    1
}

extern "C" fn button_release(area: *mut c_void, _: *const c_void, state: &State) -> i32 {
    if !state.viewport.borrow_mut().drag_end() {
        return 0;
    }
    // SAFETY: GTK passes the live realized drawing area.
    unsafe { set_cursor(area, std::ptr::null()) };
    1
}

extern "C" fn motion(_: *mut c_void, event: *const c_void, state: &State) -> i32 {
    let (mut x, mut y) = (0.0, 0.0);
    // SAFETY: GTK passes a live motion event.
    unsafe { gdk_event_get_root_coords(event, &mut x, &mut y) };
    if !state.viewport.borrow_mut().drag_to((x, y)) {
        return 0;
    }
    state.update(|_| {});
    1
}

// Shows a named cursor over the realized widget, or the default one for null.
unsafe fn set_cursor(widget: *mut c_void, name: *const std::ffi::c_char) {
    // SAFETY: The caller supplies a live realized widget and a null-terminated name or null.
    unsafe {
        let cursor = if name.is_null() {
            null_mut()
        } else {
            gdk_cursor_new_from_name(gtk_widget_get_display(widget), name)
        };
        gdk_window_set_cursor(gtk_widget_get_window(widget), cursor);
        if !cursor.is_null() {
            g_object_unref(cursor);
        }
    }
}

// Pinching magnifies around the middle of the fingers, GTK reports the scale since its start.
extern "C" fn pinch_begin(_: *mut c_void, _: *mut c_void, state: &State) {
    state.pinch.set(1.0);
}

extern "C" fn pinch(gesture: *mut c_void, scale: f64, state: &State) {
    let (mut x, mut y) = (0.0, 0.0);
    // SAFETY: GTK passes the live zoom gesture.
    let anchor = unsafe { gtk_gesture_get_bounding_box_center(gesture, &mut x, &mut y) != 0 };
    let anchor = anchor.then(|| state.visible_point((x, y)));
    let previous = state.pinch.replace(scale);
    state.update(|viewport| viewport.zoom_by(scale / previous, anchor));
}

#[cfg(test)]
mod layout_tests {
    use std::time::{Duration, Instant};

    use bwindow::{EventLoop, LogicalSize, WindowBuilder};

    use super::*;

    unsafe extern "C" {
        fn gtk_main_iteration_do(blocking: i32) -> i32;
    }

    #[test]
    #[ignore = "requires a display and session bus; run separately for each GTK backend and desktop"]
    fn initial_layout_preserves_the_requested_media_area() {
        let _event_loop = EventLoop::new();
        let window = WindowBuilder::new()
            .title("ImageView layout test")
            .size(LogicalSize::new(640.0, 480.0))
            .build();
        let view = MediaView::new(&window, None, Theme::Light);
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            // SAFETY: GTK is initialized on this thread and the window remains alive.
            unsafe { gtk_main_iteration_do(0) };
            std::thread::sleep(Duration::from_millis(5));
        }
        // SAFETY: The view retains these live widgets.
        unsafe {
            assert_eq!(
                gtk_widget_get_allocated_width(view.state.scrolled_window),
                640
            );
            assert_eq!(
                gtk_widget_get_allocated_height(view.state.scrolled_window),
                480
            );
            if !view.state.integrated_toolbar {
                assert!(gtk_widget_get_allocated_height(view.state.header_bar) >= 48);
            }
        }
    }
}
