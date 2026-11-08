/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The panels at the sides of the image, which slide in and out with their header bar switches
//! and are hidden while editing: a sidebar with the pages of a file with several, like the sizes
//! of an icon, and an inspector with the information about the image. The sidebar starts open,
//! the inspector closed.

use std::cell::{Cell, RefCell};
use std::ffi::{CString, c_void};
use std::ptr::null_mut;
use std::rc::{Rc, Weak};

use bcanvas::NativeContext;
use bwindow::WindowId;
use imageview_core::{InfoSection, Media, MediaRenderer};

use super::Cache;
use super::headers::*;
use super::toolbar::send;
use crate::AppEvent;

const SIDEBAR_WIDTH: i32 = 168;
const INSPECTOR_WIDTH: i32 = 260;
const THUMBNAIL_SIZE: i32 = 112;
const MARGIN: i32 = 12;

// A panel at a side of the image: a revealer with a separator towards the image and a
// scrolled window, whose content is replaced.
struct Panel {
    revealer: *mut c_void,
    // The box in the scrolled window that holds the content.
    holder: *mut c_void,
    content: Cell<*mut c_void>,
    open: Cell<bool>,
    // Whether there is something to show, and whether the panel is hidden while editing.
    available: Cell<bool>,
    hidden: Cell<bool>,
}

impl Panel {
    // A panel on the left or right of the image, open or closed.
    unsafe fn new(left: bool, width: i32, open: bool) -> Self {
        // SAFETY: The revealer owns the widgets it contains, the window owns the revealer once
        // it is packed.
        unsafe {
            let revealer = gtk_revealer_new();
            gtk_revealer_set_transition_type(
                revealer,
                if left {
                    GTK_REVEALER_TRANSITION_TYPE_SLIDE_RIGHT
                } else {
                    GTK_REVEALER_TRANSITION_TYPE_SLIDE_LEFT
                },
            );
            let row = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 0);
            let scrolled = gtk_scrolled_window_new(null_mut(), null_mut());
            gtk_scrolled_window_set_policy(scrolled, GTK_POLICY_NEVER, GTK_POLICY_AUTOMATIC);
            gtk_widget_set_size_request(scrolled, width, -1);
            let holder = gtk_box_new(GTK_ORIENTATION_VERTICAL, 0);
            gtk_container_add(scrolled, holder);
            let separator = gtk_separator_new(GTK_ORIENTATION_VERTICAL);
            if left {
                gtk_box_pack_start(row, scrolled, 1, 1, 0);
                gtk_box_pack_start(row, separator, 0, 0, 0);
            } else {
                gtk_box_pack_start(row, separator, 0, 0, 0);
                gtk_box_pack_start(row, scrolled, 1, 1, 0);
            }
            gtk_style_context_add_class(gtk_widget_get_style_context(scrolled), c"view".as_ptr());
            gtk_container_add(revealer, row);
            gtk_widget_show_all(row);
            // The panel decides when it shows, also for showing all children of the window.
            gtk_widget_set_no_show_all(revealer, 1);
            let panel = Self {
                revealer,
                holder,
                content: Cell::new(null_mut()),
                open: Cell::new(open),
                available: Cell::new(false),
                hidden: Cell::new(false),
            };
            panel.update();
            panel
        }
    }

    // Whether the panel slides in when it is available, which its switch shows.
    const fn is_open(&self) -> bool {
        self.open.get()
    }

    // Whether the panel can be opened now.
    const fn is_available(&self) -> bool {
        self.available.get() && !self.hidden.get()
    }

    fn toggle(&self) {
        self.open.set(!self.open.get());
        self.update();
    }

    fn set_available(&self, available: bool) {
        self.available.set(available);
        self.update();
    }

    fn set_hidden(&self, hidden: bool) {
        self.hidden.set(hidden);
        self.update();
    }

    // Shows the panel while it is available, sliding it in and out as it opens and closes.
    fn update(&self) {
        // SAFETY: The revealer is a live widget of the window.
        unsafe {
            gtk_widget_set_visible(self.revealer, i32::from(self.is_available()));
            gtk_revealer_set_reveal_child(self.revealer, i32::from(self.open.get()));
        }
    }

    // Replaces the content of the panel.
    fn set_content(&self, content: *mut c_void) {
        // SAFETY: The holder owns its content, the old content is destroyed with its children.
        unsafe {
            let old = self.content.replace(content);
            if !old.is_null() {
                gtk_widget_destroy(old);
            }
            gtk_box_pack_start(self.holder, content, 1, 1, 0);
            gtk_widget_show_all(content);
        }
    }
}

// A label of text, which GTK copies.
unsafe fn label(text: &str) -> *mut c_void {
    let text = CString::new(text.replace('\0', "")).expect("text without NUL");
    // SAFETY: GTK copies the text.
    unsafe {
        let label = gtk_label_new(text.as_ptr());
        gtk_label_set_xalign(label, 0.0);
        label
    }
}

// Adds a style class to a widget.
unsafe fn add_class(widget: *mut c_void, class: &std::ffi::CStr) {
    // SAFETY: The widget is live and GTK copies the class name.
    unsafe { gtk_style_context_add_class(gtk_widget_get_style_context(widget), class.as_ptr()) };
}

// MARK: Sidebar

// What the handlers of the page list need: the window to send picked pages to, and whether the
// selection is changed by the window instead of the user.
struct Pages {
    window: WindowId,
    selecting: Cell<bool>,
}

/// The sidebar with the pages of a file with several.
pub(super) struct Sidebar {
    panel: Panel,
    pages: Rc<Pages>,
    list: Cell<*mut c_void>,
}

impl Sidebar {
    /// A sidebar for a window, without pages yet.
    pub(super) fn new(window: WindowId) -> Self {
        Self {
            // SAFETY: The window packs the revealer of the panel.
            panel: unsafe { Panel::new(true, SIDEBAR_WIDTH, true) },
            pages: Rc::new(Pages {
                window,
                selecting: Cell::new(false),
            }),
            list: Cell::new(null_mut()),
        }
    }

    /// The widget the window packs on the left of the image.
    pub(super) const fn widget(&self) -> *mut c_void {
        self.panel.revealer
    }

    /// Whether the sidebar can be shown, for a file with several pages while not editing.
    pub(super) const fn is_available(&self) -> bool {
        self.panel.is_available()
    }

    /// Whether the sidebar is open when it can be shown.
    pub(super) const fn is_open(&self) -> bool {
        self.panel.is_open()
    }

    /// Opens or closes the sidebar.
    pub(super) fn toggle(&self) {
        self.panel.toggle();
    }

    /// Hides the sidebar while editing, or shows it again.
    pub(super) fn set_hidden(&self, hidden: bool) {
        self.panel.set_hidden(hidden);
    }

    /// Lists the pages with one selected, the sidebar shows when there are several.
    pub(super) fn set_pages(&self, pages: &[Media], page: usize) {
        // SAFETY: The list and its rows are owned by the panel, the handler data is a boxed
        // weak reference that its destroy notification drops.
        unsafe {
            let list = gtk_list_box_new();
            add_class(list, c"navigation-sidebar");
            for (index, media) in pages.iter().enumerate() {
                gtk_container_add(list, page_row(index, media));
            }
            let data = Box::into_raw(Box::new(Rc::downgrade(&self.pages)));
            g_signal_connect_data(
                list,
                c"row-selected".as_ptr(),
                row_selected as *const c_void,
                data.cast(),
                drop_pages as *const c_void,
                0,
            );
            self.list.set(list);
            self.panel.set_content(list);
        }
        self.panel.set_available(pages.len() > 1);
        self.select(page);
    }

    /// Selects a page without showing it, which the window does.
    pub(super) fn select(&self, page: usize) {
        let list = self.list.get();
        if list.is_null() {
            return;
        }
        self.pages.selecting.set(true);
        // SAFETY: The list is a live widget of the panel.
        unsafe {
            let row = gtk_list_box_get_row_at_index(list, i32::try_from(page).unwrap_or(-1));
            if !row.is_null() {
                gtk_list_box_select_row(list, row);
            }
        }
        self.pages.selecting.set(false);
    }
}

// The media of a page, drawn once for the size and scale factor of its drawing area.
struct Thumbnail {
    renderer: MediaRenderer,
    cache: RefCell<Option<Cache>>,
}

// The row of a page: its media fitted in a square, with its number and size below it.
unsafe fn page_row(index: usize, media: &Media) -> *mut c_void {
    let (width, height) = media.size();
    // SAFETY: The row owns its children, the drawing area owns its boxed thumbnail through the
    // destroy notification of its draw handler.
    unsafe {
        let row = gtk_box_new(GTK_ORIENTATION_VERTICAL, 2);
        gtk_widget_set_margin_top(row, 8);
        gtk_widget_set_margin_bottom(row, 8);
        let area = gtk_drawing_area_new();
        gtk_widget_set_size_request(area, THUMBNAIL_SIZE, THUMBNAIL_SIZE);
        gtk_widget_set_halign(area, GTK_ALIGN_CENTER);
        let thumbnail = Box::into_raw(Box::new(Thumbnail {
            renderer: MediaRenderer::new(media.clone()),
            cache: RefCell::new(None),
        }));
        g_signal_connect_data(
            area,
            c"draw".as_ptr(),
            draw_thumbnail as *const c_void,
            thumbnail.cast(),
            drop_thumbnail as *const c_void,
            0,
        );
        gtk_box_pack_start(row, area, 0, 0, 0);
        let number = label(&(index + 1).to_string());
        gtk_widget_set_halign(number, GTK_ALIGN_CENTER);
        gtk_box_pack_start(row, number, 0, 0, 0);
        let size = label(&format!(
            "{} \u{d7} {}",
            width.round() as i64,
            height.round() as i64
        ));
        gtk_widget_set_halign(size, GTK_ALIGN_CENTER);
        add_class(size, c"dim-label");
        gtk_box_pack_start(row, size, 0, 0, 0);
        row
    }
}

// Copies the media scaled down once, as scaling a large page down again is slow.
extern "C" fn draw_thumbnail(area: *mut c_void, cr: *mut c_void, thumbnail: &Thumbnail) -> i32 {
    // SAFETY: GTK passes the live realized drawing area and its cairo context while drawing.
    unsafe {
        let size = (
            gtk_widget_get_allocated_width(area),
            gtk_widget_get_allocated_height(area),
            gtk_widget_get_scale_factor(area),
        );
        let rect = GdkRectangle {
            x: 0,
            y: 0,
            width: size.0,
            height: size.1,
        };
        let draw = |cr| {
            let (width, height) = (size.0 as f32, size.1 as f32);
            bcanvas::draw_native(
                NativeContext::Cairo(cr),
                width,
                height,
                size.2 as f32,
                |canvas| {
                    thumbnail
                        .renderer
                        .draw_fitted(canvas, 0, (0.0, 0.0, width, height));
                },
            );
        };
        let mut cache = thumbnail.cache.borrow_mut();
        if !cache.as_ref().is_some_and(|cache| cache.covers(size, rect)) {
            *cache = None;
            *cache = Cache::new(
                gtk_widget_get_window(area),
                CAIRO_CONTENT_COLOR_ALPHA,
                size,
                rect,
                draw,
            );
        }
        match &*cache {
            Some(cache) => cache.paint(cr),
            None => draw(cr),
        }
    }
    1
}

extern "C" fn drop_thumbnail(data: *mut c_void, _: *mut c_void) {
    // SAFETY: The draw handler owned the boxed thumbnail created in `page_row`.
    drop(unsafe { Box::from_raw(data.cast::<Thumbnail>()) });
}

extern "C" fn row_selected(_: *mut c_void, row: *mut c_void, pages: &Weak<Pages>) {
    let Some(pages) = pages.upgrade() else {
        return;
    };
    if pages.selecting.get() || row.is_null() {
        return;
    }
    // SAFETY: GTK passes the selected row of the list.
    let index = unsafe { gtk_list_box_row_get_index(row) };
    if let Ok(page) = usize::try_from(index) {
        send(AppEvent::ShowPage(pages.window, page));
    }
}

extern "C" fn drop_pages(data: *mut c_void, _: *mut c_void) {
    // SAFETY: The selection handler owned the boxed weak reference created in `set_pages`.
    drop(unsafe { Box::from_raw(data.cast::<Weak<Pages>>()) });
}

// MARK: Inspector

/// The inspector with the information about the image.
pub(super) struct Inspector {
    panel: Panel,
}

impl Inspector {
    /// A closed inspector.
    pub(super) fn new() -> Self {
        // SAFETY: The window packs the revealer of the panel.
        let panel = unsafe { Panel::new(false, INSPECTOR_WIDTH, false) };
        panel.set_available(true);
        Self { panel }
    }

    /// The widget the window packs on the right of the image.
    pub(super) const fn widget(&self) -> *mut c_void {
        self.panel.revealer
    }

    /// Whether the inspector is open.
    pub(super) const fn is_open(&self) -> bool {
        self.panel.is_open()
    }

    /// Opens or closes the inspector.
    pub(super) fn toggle(&self) {
        self.panel.toggle();
    }

    /// Hides the inspector while editing, or shows it again.
    pub(super) fn set_hidden(&self, hidden: bool) {
        self.panel.set_hidden(hidden);
    }

    /// Lists sections, each a title with names and values below it.
    pub(super) fn set_sections(&self, sections: &[InfoSection]) {
        // SAFETY: The content box owns the labels.
        unsafe {
            let content = gtk_box_new(GTK_ORIENTATION_VERTICAL, 0);
            gtk_widget_set_margin_start(content, MARGIN);
            gtk_widget_set_margin_end(content, MARGIN);
            gtk_widget_set_margin_bottom(content, MARGIN);
            for section in sections {
                let title = label(section.title);
                gtk_label_set_markup(
                    title,
                    CString::new(format!("<b>{}</b>", escape(section.title)))
                        .expect("title without NUL")
                        .as_ptr(),
                );
                gtk_widget_set_margin_top(title, 18);
                gtk_widget_set_margin_bottom(title, 6);
                gtk_box_pack_start(content, title, 0, 0, 0);
                for (name, value) in &section.rows {
                    let name = label(name);
                    add_class(name, c"dim-label");
                    gtk_box_pack_start(content, name, 0, 0, 0);
                    // Values can be copied, like a camera serial number or coordinates.
                    let value = label(value);
                    gtk_label_set_line_wrap(value, 1);
                    gtk_label_set_line_wrap_mode(value, GTK_WRAP_WORD_CHAR);
                    gtk_label_set_selectable(value, 1);
                    gtk_widget_set_halign(value, GTK_ALIGN_START);
                    gtk_widget_set_margin_bottom(value, 8);
                    gtk_box_pack_start(content, value, 0, 0, 0);
                }
            }
            self.panel.set_content(content);
        }
    }
}

// Text with the characters of Pango markup escaped.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
