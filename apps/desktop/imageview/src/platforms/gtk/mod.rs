/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The GTK media view: a native GtkScrolledWindow around a drawing area as large as the
//! magnified media, which draws the viewport with bcanvas, in an overlay that shows a spinner
//! while the media loads or an edit is applied. The scrolled window scrolls the drawing area, its
//! scroll position is followed by the viewport and the other way around.

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::ffi::{CStr, c_void};
use std::ptr::{from_ref, null, null_mut};
use std::rc::{Rc, Weak};

use bcanvas::NativeContext;
use bwindow::{NativeViewHandle, Theme, Window, WindowAttachment, WindowId};
use imageview_core::edit::{Control, Selection, Shape, Tool};
use imageview_core::{InfoSection, Media, WHEEL_ZOOM_RATE};

use self::headers::*;
use self::panels::{Inspector, Sidebar};
use self::popover::Popover;
pub(crate) use self::save::SaveOptions;
pub(crate) use self::thumbnail::thumbnail;
use self::toolbar::{Toolbar, send};
use crate::AppEvent;
use crate::viewport::{Axis, LINE, Step, Viewport};

mod headers;
mod panels;
mod popover;
mod save;
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
    toolbar: Toolbar,
    integrated_toolbar: bool,
    scrolled_window: *mut c_void,
    // The horizontal and vertical adjustments of the scrolled window, which a destroyed
    // scrolled window no longer returns.
    adjustments: [*mut c_void; 2],
    // The spinner shown until the media is decoded or an edit is applied, or null.
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
    // The popover of the open edit tool.
    popover: Option<Popover>,
    // The pages of a file with several on the left of the image, the information about the
    // image on the right.
    sidebar: Sidebar,
    inspector: Inspector,
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
        let toolbar = unsafe {
            let Some(bwindow::NativeWindowHandle::Gtk(window)) = attachment.window_handle() else {
                unreachable!("GTK view requires a GTK window");
            };
            let toolbar = Toolbar::install(window, integrated_toolbar);
            if integrated_toolbar && let Some(media) = &media {
                toolbar::show_size(toolbar.bar, media);
            }
            toolbar
        };
        let header_bar = toolbar.bar;
        // Only still bitmaps can be edited, the loading media has no frames.
        toolbar.set_editable(media.as_ref().is_some_and(Media::is_still));
        // SAFETY: The created widgets are owned by this view until it drops them, the window
        // keeps the overlay it embeds alive while the attachment is open.
        let state = unsafe {
            let overlay = g_object_ref_sink(gtk_overlay_new());
            let scrolled_window =
                g_object_ref_sink(gtk_scrolled_window_new(null_mut(), null_mut()));
            gtk_container_add(overlay, scrolled_window);
            let spinner = if loading {
                add_spinner(overlay)
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
                toolbar,
                integrated_toolbar,
                scrolled_window,
                adjustments: [
                    g_object_ref(gtk_scrolled_window_get_hadjustment(scrolled_window)),
                    g_object_ref(gtk_scrolled_window_get_vadjustment(scrolled_window)),
                ],
                spinner: Cell::new(spinner),
                area,
                gesture: gtk_gesture_zoom_new(area),
                updating: Cell::new(false),
                pinch: Cell::new(1.0),
                timer: Cell::new(0),
            })
        };
        state.connect();
        let sidebar = Sidebar::new(window.id());
        let inspector = Inspector::new();
        // SAFETY: The overlay is a live widget that the window lays out from now on.
        unsafe {
            let body = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 0);
            gtk_box_pack_start(body, sidebar.widget(), 0, 0, 0);
            gtk_box_pack_start(body, state.overlay, 1, 1, 0);
            gtk_box_pack_start(body, inspector.widget(), 0, 0, 0);
            let content = if integrated_toolbar {
                body
            } else {
                let content = gtk_box_new(GTK_ORIENTATION_VERTICAL, 0);
                gtk_box_pack_start(content, header_bar, 0, 0, 0);
                gtk_box_pack_start(content, body, 1, 1, 0);
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
        let view = Self {
            state,
            popover: None,
            sidebar,
            inspector,
            _attachment: attachment,
        };
        view.update_switches();
        view
    }

    /// Lists the pages of a file with several in the sidebar with one selected, or hides it.
    pub(crate) fn set_pages(&self, pages: &[Media], page: usize) {
        self.sidebar.set_pages(pages, page);
        self.update_switches();
    }

    /// Selects the shown page in the sidebar.
    pub(crate) fn select_page(&self, page: usize) {
        self.sidebar.select(page);
    }

    /// Opens or closes the sidebar with the pages, when there is one.
    pub(crate) fn toggle_sidebar(&self) {
        if self.sidebar.is_available() {
            self.sidebar.toggle();
        }
        self.update_switches();
    }

    /// Opens or closes the inspector with the information about the image.
    pub(crate) fn toggle_info(&self) {
        self.inspector.toggle();
        self.update_switches();
    }

    /// Lists the information about the image in the inspector.
    pub(crate) fn set_info(&self, sections: &[InfoSection]) {
        self.inspector.set_sections(sections);
    }

    // Shows the header bar switches of the panels as they are, the sidebar one only with
    // several pages.
    fn update_switches(&self) {
        let toolbar = &self.state.toolbar;
        toolbar.set_switch_visible("view.sidebar", self.sidebar.is_available());
        toolbar.set_switch("view.sidebar", self.sidebar.is_open());
        toolbar.set_switch("view.info", self.inspector.is_open());
    }

    /// The shown media.
    pub(crate) fn media(&self) -> Media {
        self.state.viewport.borrow().media().clone()
    }

    /// Replaces the media, keeping the zoom or fitting it to the window.
    pub(crate) fn set_media(&mut self, media: Media, zoom_to_fit: bool) {
        self.state.stop_animation();
        // Vectors and animations cannot be edited, only saved as bitmaps.
        self.state.toolbar.set_editable(media.is_still());
        if self.state.integrated_toolbar {
            // SAFETY: The window owns the header bar while this view is attached.
            unsafe { toolbar::show_size(self.state.toolbar.bar, &media) };
        }
        self.state
            .update(|viewport| viewport.set_media(media, zoom_to_fit));
        self.state.start_animation();
        self.remove_spinner();
    }

    fn remove_spinner(&self) {
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

    /// Shows the edit controls, or the view controls again.
    pub(crate) fn set_editing(&mut self, editing: bool) {
        self.close_tool();
        self.state.toolbar.set_editing(editing);
        // Only the shown page is edited, so the other pages and the information are hidden
        // meanwhile.
        self.sidebar.set_hidden(editing);
        self.inspector.set_hidden(editing);
        self.update_switches();
    }

    /// Shows the popover with controls of a tool below the toolbar button of `action`, and
    /// the crop rectangle for a crop. Changes of the controls go to `window`, reset goes back
    /// to the defaults.
    pub(crate) fn show_tool(
        &mut self,
        window: WindowId,
        tool: Tool,
        controls: &[Control],
        values: &[f32],
        defaults: &[f32],
        action: &str,
    ) {
        self.close_tool();
        if tool == Tool::Crop {
            self.state.update(|viewport| viewport.set_cropping(true));
            self.state.show_cursor();
        }
        let anchor = self.state.toolbar.button(action).unwrap_or(self.state.area);
        // SAFETY: The anchor is a live widget of the window of this view.
        self.popover =
            Some(unsafe { Popover::new(anchor, window, tool, controls, values, defaults) });
    }

    /// Shows changed values of the open tool.
    pub(crate) fn set_tool_values(&self, values: &[f32]) {
        if let Some(popover) = &self.popover {
            popover.set_values(values);
        }
    }

    /// Closes the popover and crop rectangle of the open tool.
    pub(crate) fn close_tool(&mut self) {
        if self.popover.take().is_some() {
            self.state.update(|viewport| viewport.set_cropping(false));
            self.state.show_cursor();
            // SAFETY: The drawing area is alive while this view is.
            unsafe { gtk_widget_grab_focus(self.state.area) };
        }
    }

    /// Makes drags draw selections of a shape, or selects the whole image again, shown as on
    /// in the toolbar.
    pub(crate) fn set_selecting(&self, shape: Option<Shape>) {
        self.state.update(|viewport| viewport.set_selecting(shape));
        self.state.show_cursor();
        self.state.toolbar.set_toggled(match shape {
            None => "select.whole",
            Some(Shape::Rectangle) => "select.rectangle",
            Some(Shape::Ellipse) => "select.ellipse",
        });
    }

    /// The selected part of the media.
    pub(crate) fn selection(&self) -> Option<Selection> {
        self.state.viewport.borrow().selection()
    }

    /// Selects everything outside the selection instead.
    pub(crate) fn invert_selection(&self) {
        self.state.update(Viewport::invert_selection);
    }

    /// Puts a PNG image on the clipboard, for as long as this application lives or the desktop
    /// keeps a copy.
    pub(crate) fn copy_png(&self, png: Vec<u8>) {
        let targets = [GtkTargetEntry {
            target: c"image/png".as_ptr(),
            flags: 0,
            info: 0,
        }];
        let png = Box::into_raw(Box::new(png)).cast::<c_void>();
        // SAFETY: GTK copies the targets and hands the boxed image to the clipboard handlers,
        // the last of which frees it.
        unsafe {
            let clipboard = gtk_clipboard_get(gdk_atom_intern(c"CLIPBOARD".as_ptr(), 0));
            if gtk_clipboard_set_with_data(
                clipboard,
                targets.as_ptr(),
                1,
                clipboard_get,
                clipboard_clear,
                png,
            ) == 0
            {
                drop(Box::from_raw(png.cast::<Vec<u8>>()));
                return;
            }
            // The application hands the image to the clipboard manager of the desktop when it
            // quits, which storing right away would do in a nested main loop.
            gtk_clipboard_set_can_store(clipboard, targets.as_ptr(), 1);
        }
    }

    /// Shows a preview of any size in the place of the image, or the image again.
    pub(crate) fn set_preview(&self, preview: Option<Media>) {
        self.state.update(|viewport| viewport.set_preview(preview));
    }

    /// Places the crop rectangle at a position with a size in pixels.
    pub(crate) fn set_crop_rect(&self, rect: (u32, u32, u32, u32)) {
        self.state.update(|viewport| {
            if let Some(crop) = viewport.crop() {
                crop.set_rect(rect);
            }
        });
    }

    /// Makes the crop rectangle keep a width divided by height, or any shape.
    pub(crate) fn set_crop_aspect(&self, aspect: Option<f64>) {
        self.state.update(|viewport| {
            if let Some(crop) = viewport.crop() {
                crop.set_aspect(aspect);
            }
        });
    }

    /// The crop rectangle in pixels, or all of the media without one.
    pub(crate) fn crop_rect(&self) -> (u32, u32, u32, u32) {
        let mut viewport = self.state.viewport.borrow_mut();
        let (width, height) = viewport.media().size();
        viewport
            .crop()
            .map_or((0, 0, width as u32, height as u32), |crop| crop.rect())
    }

    /// The longest side of the view in device pixels, which previews do not need to exceed.
    pub(crate) fn longest_pixels(&self) -> u32 {
        let area = self.state.scrolled_window;
        // SAFETY: The scrolled window is alive while this view is.
        unsafe {
            let longest = gtk_widget_get_allocated_width(area)
                .max(gtk_widget_get_allocated_height(area))
                .max(1);
            (longest * gtk_widget_get_scale_factor(area)) as u32
        }
    }

    /// Shows a spinner while an edit is applied.
    pub(crate) fn set_busy(&self, busy: bool) {
        if !busy {
            self.remove_spinner();
        } else if self.state.spinner.get().is_null() {
            // SAFETY: The overlay is alive while this view is.
            self.state
                .spinner
                .set(unsafe { add_spinner(self.state.overlay) });
        }
    }

    /// Redraws the checkerboard in the colors of the new theme.
    pub(crate) fn theme_changed(&self, theme: Theme) {
        self.state.dark.set(theme == Theme::Dark);
        // SAFETY: The drawing area is alive while this view is.
        unsafe { gtk_widget_queue_draw(self.state.area) };
    }
}

// Adds a running spinner in the middle of an overlay, which keeps it there.
unsafe fn add_spinner(overlay: *mut c_void) -> *mut c_void {
    // SAFETY: The caller supplies a live overlay, which owns its children.
    unsafe {
        let spinner = gtk_spinner_new();
        gtk_widget_set_halign(spinner, GTK_ALIGN_CENTER);
        gtk_widget_set_valign(spinner, GTK_ALIGN_CENTER);
        gtk_widget_set_size_request(spinner, 32, 32);
        gtk_overlay_add_overlay(overlay, spinner);
        gtk_overlay_set_overlay_pass_through(overlay, spinner, 1);
        gtk_spinner_start(spinner);
        gtk_widget_show(spinner);
        spinner
    }
}

// Gives the image to an application that pastes it.
extern "C" fn clipboard_get(_: *mut c_void, data: *mut c_void, _: u32, png: *mut c_void) {
    // SAFETY: GTK passes the selection data and the boxed image of this clipboard content.
    unsafe {
        let png = &*png.cast::<Vec<u8>>();
        gtk_selection_data_set(
            data,
            gdk_atom_intern(c"image/png".as_ptr(), 0),
            8,
            png.as_ptr(),
            png.len() as i32,
        );
    }
}

extern "C" fn clipboard_clear(_: *mut c_void, png: *mut c_void) {
    // SAFETY: GTK is done with the boxed image of this clipboard content.
    drop(unsafe { Box::from_raw(png.cast::<Vec<u8>>()) });
}

impl Drop for MediaView {
    fn drop(&mut self) {
        self.popover = None;
        let state = &self.state;
        state.stop_animation();
        let data = from_ref::<State>(state).cast::<c_void>();
        // SAFETY: This view owns a reference to each object, the handlers using the state are
        // disconnected before it is freed.
        unsafe {
            let [horizontal, vertical] = state.adjustments;
            for object in [
                state.area,
                state.gesture,
                state.scrolled_window,
                horizontal,
                vertical,
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
            g_object_unref(horizontal);
            g_object_unref(vertical);
            g_object_unref(state.scrolled_window);
            g_object_unref(state.overlay);
        }
    }
}

impl State {
    fn connect(&self) {
        let data = from_ref(self).cast::<c_void>();
        let signals: [(*mut c_void, &CStr, *const c_void); 11] = [
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

    const fn adjustment(&self, axis: Axis) -> *mut c_void {
        match axis {
            Axis::Horizontal => self.adjustments[0],
            Axis::Vertical => self.adjustments[1],
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

    // Shows a crosshair while cropping or selecting.
    fn show_cursor(&self) {
        let marking = self.viewport.borrow().is_marking();
        let name = if marking {
            c"crosshair".as_ptr()
        } else {
            null()
        };
        // SAFETY: The drawing area is alive while the state is.
        unsafe { set_cursor(self.area, name) };
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
        // Return applies the open tool and escape cancels it.
        GDK_KEY_RETURN | GDK_KEY_KP_ENTER => {
            send(AppEvent::Action("tool.apply"));
            return 1;
        }
        GDK_KEY_ESCAPE => {
            send(AppEvent::Action("tool.cancel"));
            return 1;
        }
        _ => return 0,
    };
    state.update(|viewport| viewport.step(Axis::Vertical, step));
    1
}

// Dragging with the primary button draws or changes the crop rectangle or selection, or else
// pans the media, the screen position does not move with it.
extern "C" fn button_press(area: *mut c_void, event: *const c_void, state: &State) -> i32 {
    let (mut button, mut x, mut y, mut area_x, mut area_y) = (0, 0.0, 0.0, 0.0, 0.0);
    // SAFETY: GTK passes the live drawing area and a button event.
    unsafe {
        gtk_widget_grab_focus(area);
        if gdk_event_get_button(event, &mut button) == 0 || button != 1 {
            return 0;
        }
        // A double or triple click reports another press after the presses of its clicks.
        if gdk_event_get_event_type(event) != GDK_BUTTON_PRESS {
            return 1;
        }
        gdk_event_get_root_coords(event, &mut x, &mut y);
        gdk_event_get_coords(event, &mut area_x, &mut area_y);
    }
    let pointer = state.visible_point((area_x, area_y));
    if state.viewport.borrow_mut().mark_begin(pointer) {
        state.update(|_| {});
        return 1;
    }
    if state.viewport.borrow_mut().drag_begin((x, y)) {
        // SAFETY: GTK passes the live realized drawing area.
        unsafe { set_cursor(area, c"grabbing".as_ptr()) };
    }
    1
}

extern "C" fn button_release(_: *mut c_void, event: *const c_void, state: &State) -> i32 {
    let mut button = 0;
    // SAFETY: GTK passes a live button event.
    if unsafe { gdk_event_get_button(event, &mut button) } == 0 || button != 1 {
        return 0;
    }
    let (marked, cleared) = {
        let mut viewport = state.viewport.borrow_mut();
        let marked = viewport.mark_end();
        let cleared = viewport.selecting().is_some() && viewport.selection().is_none();
        (marked, marked && cleared)
    };
    // A click without a drag goes back to editing the whole image, the menu follows a drawn
    // selection.
    if cleared {
        send(AppEvent::Action("select.whole"));
    } else if marked {
        send(AppEvent::Action("select.changed"));
    }
    if marked {
        state.update(|_| {});
        return 1;
    }
    if !state.viewport.borrow_mut().drag_end() {
        return 0;
    }
    state.show_cursor();
    1
}

extern "C" fn motion(_: *mut c_void, event: *const c_void, state: &State) -> i32 {
    let (mut x, mut y, mut area_x, mut area_y, mut modifiers) = (0.0, 0.0, 0.0, 0.0, 0);
    // SAFETY: GTK passes a live motion event.
    unsafe {
        gdk_event_get_root_coords(event, &mut x, &mut y);
        gdk_event_get_coords(event, &mut area_x, &mut area_y);
        gdk_event_get_state(event, &mut modifiers);
    }
    let pointer = state.visible_point((area_x, area_y));
    let moved = {
        let mut viewport = state.viewport.borrow_mut();
        viewport.mark_drag(pointer, modifiers & GDK_SHIFT_MASK != 0) || viewport.drag_to((x, y))
    };
    if !moved {
        return 0;
    }
    state.update(|_| {});
    1
}

// Shows a named cursor over the realized widget, or the default one for null.
unsafe fn set_cursor(widget: *mut c_void, name: *const std::ffi::c_char) {
    // SAFETY: The caller supplies a live realized widget and a null-terminated name or null.
    unsafe {
        let window = gtk_widget_get_window(widget);
        if window.is_null() {
            return;
        }
        let cursor = if name.is_null() {
            null_mut()
        } else {
            gdk_cursor_new_from_name(gtk_widget_get_display(widget), name)
        };
        gdk_window_set_cursor(window, cursor);
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
        fn g_object_weak_ref(
            object: *mut c_void,
            notify: extern "C" fn(*mut c_void, *mut c_void),
            data: *const c_void,
        );
    }

    #[test]
    #[ignore = "requires a display and session bus"]
    fn closing_a_view_releases_its_native_objects() {
        extern "C" fn finalized(data: *mut c_void, _: *mut c_void) {
            // SAFETY: Each weak notification owns one reference until its object is finalized.
            let count = unsafe { Rc::from_raw(data.cast::<Cell<usize>>()) };
            count.set(count.get() + 1);
        }
        let _event_loop = EventLoop::new();
        let mut window = WindowBuilder::new().build();
        let view = MediaView::new(&window, None, Theme::Light);
        let state = Rc::downgrade(&view.state);
        let count = Rc::new(Cell::new(0));
        let objects = [
            view.state.overlay,
            view.state.scrolled_window,
            view.state.area,
            view.state.gesture,
            view.state.adjustments[0],
            view.state.adjustments[1],
        ];
        for object in objects {
            // SAFETY: The view retains these objects and every notification owns its counter.
            unsafe { g_object_weak_ref(object, finalized, Rc::into_raw(count.clone()).cast()) };
        }
        drop(view);
        window.close();
        drop(window);
        for _ in 0..32 {
            // SAFETY: GTK remains initialized while deferred destruction is processed.
            unsafe { gtk_main_iteration_do(0) };
        }
        assert!(
            state.upgrade().is_none(),
            "callbacks retained the view state"
        );
        assert_eq!(count.get(), objects.len(), "native objects were retained");
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
                assert!(gtk_widget_get_allocated_height(view.state.toolbar.bar) >= 48);
            }
        }
    }
}
