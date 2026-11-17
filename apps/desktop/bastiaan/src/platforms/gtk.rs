/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The GTK scroll view: a native GtkScrolledWindow around a drawing area as tall as the page,
//! which draws the visible part of the page with bcanvas at the scroll position.

use std::cell::Cell;
use std::ffi::{CStr, c_char, c_void};
use std::ptr::null_mut;
use std::rc::Rc;

use bcanvas::{CanvasRenderingContext2d, NativeContext};
use bwindow::{
    LogicalPoint, Modifiers, MouseButton, MouseEvent, NativeViewHandle, Window, WindowAttachment,
    WindowEvent, WindowEventSender,
};

use super::LINE;

const GDK_POINTER_MOTION_MASK: i32 = 1 << 2;
const GDK_BUTTON_PRESS_MASK: i32 = 1 << 8;
const GDK_BUTTON_RELEASE_MASK: i32 = 1 << 9;
const GDK_KEY_PRESS_MASK: i32 = 1 << 10;
const GDK_LEAVE_NOTIFY_MASK: i32 = 1 << 13;
const GDK_KEY_HOME: u32 = 0xff50;
const GDK_KEY_UP: u32 = 0xff52;
const GDK_KEY_DOWN: u32 = 0xff54;
const GDK_KEY_PAGE_UP: u32 = 0xff55;
const GDK_KEY_PAGE_DOWN: u32 = 0xff56;
const GDK_KEY_END: u32 = 0xff57;
const GTK_POLICY_AUTOMATIC: i32 = 1;
const GTK_POLICY_NEVER: i32 = 2;
const G_SIGNAL_MATCH_DATA: i32 = 1 << 4;

// GTK, GDK, GLib and cairo, which bwindow and bcanvas link.
unsafe extern "C" {
    fn gtk_scrolled_window_new(hadjustment: *mut c_void, vadjustment: *mut c_void) -> *mut c_void;
    fn gtk_scrolled_window_set_policy(window: *mut c_void, horizontal: i32, vertical: i32);
    fn gtk_scrolled_window_get_vadjustment(window: *mut c_void) -> *mut c_void;
    fn gtk_drawing_area_new() -> *mut c_void;
    fn gtk_container_add(container: *mut c_void, widget: *mut c_void);
    fn gtk_widget_add_events(widget: *mut c_void, events: i32);
    fn gtk_widget_set_can_focus(widget: *mut c_void, can_focus: i32);
    fn gtk_widget_grab_focus(widget: *mut c_void);
    fn gtk_widget_set_size_request(widget: *mut c_void, width: i32, height: i32);
    fn gtk_widget_queue_draw(widget: *mut c_void);
    fn gtk_widget_get_allocated_width(widget: *mut c_void) -> i32;
    fn gtk_widget_get_allocated_height(widget: *mut c_void) -> i32;
    fn gtk_widget_get_scale_factor(widget: *mut c_void) -> i32;
    fn gtk_widget_get_window(widget: *mut c_void) -> *mut c_void;
    fn gtk_adjustment_get_value(adjustment: *mut c_void) -> f64;
    fn gtk_adjustment_set_value(adjustment: *mut c_void, value: f64);
    fn gtk_adjustment_get_page_size(adjustment: *mut c_void) -> f64;
    fn gdk_event_get_coords(event: *const c_void, x: *mut f64, y: *mut f64) -> i32;
    fn gdk_event_get_keyval(event: *const c_void, keyval: *mut u32) -> i32;
    fn gdk_window_get_display(window: *mut c_void) -> *mut c_void;
    fn gdk_window_set_cursor(window: *mut c_void, cursor: *mut c_void);
    fn gdk_cursor_new_from_name(display: *mut c_void, name: *const c_char) -> *mut c_void;
    fn g_signal_connect_data(
        instance: *mut c_void,
        signal: *const c_char,
        handler: *const c_void,
        data: *const c_void,
        destroy: *mut c_void,
        flags: i32,
    ) -> u64;
    fn g_signal_handlers_disconnect_matched(
        instance: *mut c_void,
        mask: i32,
        signal: u32,
        detail: u32,
        closure: *mut c_void,
        function: *mut c_void,
        data: *const c_void,
    ) -> u32;
    fn g_idle_add(function: extern "C" fn(*mut c_void) -> i32, data: *mut c_void) -> u32;
    fn g_object_ref(object: *mut c_void) -> *mut c_void;
    fn g_object_ref_sink(object: *mut c_void) -> *mut c_void;
    fn g_object_unref(object: *mut c_void);
    fn cairo_save(cr: *mut c_void);
    fn cairo_restore(cr: *mut c_void);
    fn cairo_translate(cr: *mut c_void, x: f64, y: f64);
}

struct State {
    sender: WindowEventSender,
    scrolled_window: *mut c_void,
    area: *mut c_void,
    // The cairo context while the drawing area draws itself.
    context: Cell<*mut c_void>,
}

/// A native view that scrolls a page vertically.
pub(crate) struct ScrollView {
    state: Rc<State>,
    attachment: WindowAttachment,
}

impl ScrollView {
    /// Fills the window with an empty page.
    pub(crate) fn new(window: &Window) -> Self {
        let attachment = window.attach_content().expect("window has no content yet");
        // SAFETY: The created widgets are owned by this view until it drops them, the window
        // keeps the scrolled window it embeds alive while the attachment is open.
        let state = unsafe {
            let scrolled_window =
                g_object_ref_sink(gtk_scrolled_window_new(null_mut(), null_mut()));
            gtk_scrolled_window_set_policy(scrolled_window, GTK_POLICY_NEVER, GTK_POLICY_AUTOMATIC);
            let area = g_object_ref(gtk_drawing_area_new());
            gtk_widget_add_events(
                area,
                GDK_POINTER_MOTION_MASK
                    | GDK_BUTTON_PRESS_MASK
                    | GDK_BUTTON_RELEASE_MASK
                    | GDK_KEY_PRESS_MASK
                    | GDK_LEAVE_NOTIFY_MASK,
            );
            gtk_widget_set_can_focus(area, 1);
            gtk_container_add(scrolled_window, area);
            Rc::new(State {
                sender: attachment.event_sender(),
                scrolled_window,
                area,
                context: Cell::new(null_mut()),
            })
        };
        state.connect();
        // SAFETY: The scrolled window is a live widget that the window lays out from now on.
        unsafe {
            attachment.set_native_view(NativeViewHandle::Gtk(state.scrolled_window));
            gtk_widget_grab_focus(state.area);
        }
        // Redraws, also the animation frames, paint through the drawing area
        let redraw_state = Rc::downgrade(&state);
        attachment.on_redraw(move || {
            if let Some(state) = redraw_state.upgrade() {
                state.redraw();
            }
        });
        Self { state, attachment }
    }

    /// Paints the visible part of the page during the window's `RedrawRequested`, returns
    /// whether it painted.
    pub(crate) fn draw(&self, draw: impl FnOnce(&mut CanvasRenderingContext2d<'_>)) -> bool {
        let state = &self.state;
        let cr = state.context.get();
        if cr.is_null() {
            return false;
        }
        // SAFETY: The drawing area is drawing into the context, which is moved to the visible
        // part of the page and back.
        unsafe {
            cairo_save(cr);
            cairo_translate(cr, 0.0, state.scroll());
            bcanvas::draw_native(
                NativeContext::Cairo(cr),
                gtk_widget_get_allocated_width(state.area) as f32,
                gtk_widget_get_allocated_height(state.scrolled_window) as f32,
                gtk_widget_get_scale_factor(state.area) as f32,
                draw,
            );
            cairo_restore(cr);
        }
        true
    }

    /// The scroll position in logical pixels.
    pub(crate) fn scroll(&self) -> f32 {
        self.state.scroll() as f32
    }

    /// Sets the height of the page, which the scroll bar scrolls through.
    pub(crate) fn set_content_height(&self, height: f32) {
        // SAFETY: The drawing area is alive, the scrolled window grows it to the visible area.
        unsafe { gtk_widget_set_size_request(self.state.area, -1, height.ceil() as i32) };
    }

    /// Paints the page again.
    pub(crate) fn request_redraw(&self) {
        self.state.redraw();
    }

    /// Paints the page again at the next display frame.
    pub(crate) fn request_animation_frame(&self) {
        self.attachment.request_animation_frame();
    }

    /// Shows a pointing hand over the page instead of the arrow.
    pub(crate) fn set_pointer_cursor(&self, pointer: bool) {
        let name: &CStr = if pointer { c"pointer" } else { c"default" };
        // SAFETY: The drawing area's window is alive while it is realized, the window keeps
        // a reference to the cursor.
        unsafe {
            let window = gtk_widget_get_window(self.state.area);
            if window.is_null() {
                return;
            }
            let cursor = gdk_cursor_new_from_name(gdk_window_get_display(window), name.as_ptr());
            gdk_window_set_cursor(window, cursor);
            if !cursor.is_null() {
                g_object_unref(cursor);
            }
        }
    }

    /// The scroll bar follows the GTK theme by itself.
    pub(crate) const fn set_dark(&self, _: bool) {}
}

impl Drop for ScrollView {
    fn drop(&mut self) {
        let state = &self.state;
        let data = Rc::as_ptr(state).cast::<c_void>();
        // SAFETY: This view owns a reference to each widget, the handlers using the state are
        // disconnected before it is freed.
        unsafe {
            for object in [state.area, state.adjustment()] {
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
            g_object_unref(state.area);
            g_object_unref(state.scrolled_window);
        }
    }
}

impl State {
    fn connect(&self) {
        let data = std::ptr::from_ref(self).cast::<c_void>();
        let signals: [(*mut c_void, &CStr, *const c_void); 6] = [
            (self.area, c"draw", draw as *const c_void),
            (self.area, c"motion-notify-event", motion as *const c_void),
            (self.area, c"button-press-event", button as *const c_void),
            (self.area, c"button-release-event", button as *const c_void),
            (self.area, c"leave-notify-event", leave as *const c_void),
            (self.area, c"key-press-event", key_press as *const c_void),
        ];
        for (object, signal, handler) in signals {
            // SAFETY: The handlers match the signal signatures and the view disconnects them
            // before the state they receive is freed.
            unsafe { g_signal_connect_data(object, signal.as_ptr(), handler, data, null_mut(), 0) };
        }
        // SAFETY: As above, the visible part moves with the scroll position.
        unsafe {
            g_signal_connect_data(
                self.adjustment(),
                c"value-changed".as_ptr(),
                scrolled as *const c_void,
                data,
                null_mut(),
                0,
            );
        }
    }

    fn adjustment(&self) -> *mut c_void {
        // SAFETY: The scrolled window is alive and owns its adjustment.
        unsafe { gtk_scrolled_window_get_vadjustment(self.scrolled_window) }
    }

    fn scroll(&self) -> f64 {
        // SAFETY: The adjustment is alive.
        unsafe { gtk_adjustment_get_value(self.adjustment()) }
    }

    fn redraw(&self) {
        // SAFETY: The drawing area is alive while the state is.
        unsafe { gtk_widget_queue_draw(self.area) };
    }

    // A point of the drawing area in the visible area.
    fn position(&self, event: *const c_void) -> LogicalPoint {
        let (mut x, mut y) = (0.0, 0.0);
        // SAFETY: GTK passes a live pointer event.
        unsafe { gdk_event_get_coords(event, &mut x, &mut y) };
        LogicalPoint::new(x as f32, (y - self.scroll()) as f32)
    }
}

// Draws through the window's RedrawRequested, or again when the event loop is idle.
extern "C" fn draw(_: *mut c_void, cr: *mut c_void, state: &State) -> i32 {
    state.context.set(cr);
    let drawn = state.sender.redraw(|| {});
    state.context.set(null_mut());
    if !drawn {
        // SAFETY: The idle callback holds a reference to the drawing area.
        unsafe { g_idle_add(redraw_later, g_object_ref(state.area)) };
    }
    1
}

extern "C" fn redraw_later(area: *mut c_void) -> i32 {
    // SAFETY: The callback owns a reference to the drawing area.
    unsafe {
        gtk_widget_queue_draw(area);
        g_object_unref(area);
    }
    0
}

extern "C" fn scrolled(_: *mut c_void, state: &State) {
    state.redraw();
}

extern "C" fn motion(_: *mut c_void, event: *const c_void, state: &State) -> i32 {
    state.sender.send(WindowEvent::MouseMove {
        position: state.position(event),
        movement: LogicalPoint::new(0.0, 0.0),
        modifiers: Modifiers::NONE,
    });
    0
}

// The event type is the first field of every GDK event: 4 is a press, 7 a release.
extern "C" fn button(area: *mut c_void, event: *const c_void, state: &State) -> i32 {
    let mouse_event = MouseEvent {
        button: MouseButton::Left,
        position: state.position(event),
        modifiers: Modifiers::NONE,
    };
    // SAFETY: GTK passes a live button event, which starts with its type.
    let pressed = unsafe { *event.cast::<i32>() } == 4;
    if pressed {
        // SAFETY: The drawing area is alive.
        unsafe { gtk_widget_grab_focus(area) };
    }
    state.sender.send(if pressed {
        WindowEvent::MouseDown(mouse_event)
    } else {
        WindowEvent::MouseUp(mouse_event)
    });
    0
}

extern "C" fn leave(_: *mut c_void, _: *const c_void, state: &State) -> i32 {
    state.sender.send(WindowEvent::MouseLeave);
    0
}

extern "C" fn key_press(_: *mut c_void, event: *const c_void, state: &State) -> i32 {
    let mut keyval = 0;
    // SAFETY: GTK passes a live key event.
    unsafe { gdk_event_get_keyval(event, &mut keyval) };
    let adjustment = state.adjustment();
    // SAFETY: The adjustment is alive, it clamps the value to its range.
    unsafe {
        let (scroll, page) = (
            gtk_adjustment_get_value(adjustment),
            gtk_adjustment_get_page_size(adjustment),
        );
        let scroll = match keyval {
            GDK_KEY_UP => scroll - f64::from(LINE),
            GDK_KEY_DOWN => scroll + f64::from(LINE),
            GDK_KEY_PAGE_UP => scroll - page,
            GDK_KEY_PAGE_DOWN => scroll + page,
            GDK_KEY_HOME => 0.0,
            GDK_KEY_END => f64::MAX,
            _ => return 0,
        };
        gtk_adjustment_set_value(adjustment, scroll);
    }
    1
}
