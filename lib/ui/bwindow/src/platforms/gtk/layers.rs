/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Native window layers: a GDK window for each layer, placed over the content area without
//! input. On Wayland it is a subsurface the compositor blends, cropped to the content area by
//! a viewport. On X11 it is a child window that GDK redirects offscreen with the Composite
//! extension, and the window paints the layer windows over the content area in stack order
//! whenever one changes, which the X server blends with XRender. Any renderer draws a layer
//! window, like Cairo or Vulkan. A layer reaching outside the content area keeps its size, so
//! moving it needs no redraw. Without Composite there are no layers.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::null_mut;

use super::headers::*;
use super::wayland::{Viewport, wayland_surface};
use crate::{LogicalRect, NativeContentHandle};

// The state of a layer window: its content area, its Wayland viewport with the source
// rectangle and destination size it crops to, and whether it is hidden or outside the content.
struct LayerState {
    area: *mut GtkWidget,
    viewport: Option<Viewport>,
    crop: Option<([f64; 4], i32, i32)>,
    hidden: bool,
    outside: bool,
}

thread_local! {
    static LAYERS: RefCell<HashMap<usize, LayerState>> = RefCell::new(HashMap::new());
    // The X11 layer windows of every window from bottom to top, which the windows paint.
    static STACK: RefCell<Vec<*mut c_void>> = const { RefCell::new(Vec::new()) };
}

const GDK_INPUT_OUTPUT: i32 = 0;
const GDK_WINDOW_CHILD: i32 = 2;
const GDK_WINDOW_SUBSURFACE: i32 = 6;
const GDK_WA_X: i32 = 1 << 2;
const GDK_WA_Y: i32 = 1 << 3;
const GDK_WA_VISUAL: i32 = 1 << 5;

#[repr(C)]
struct WindowAttributes {
    title: *mut c_void,
    event_mask: i32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    wclass: i32,
    visual: *mut c_void,
    window_type: i32,
    cursor: *mut c_void,
    wmclass_name: *mut c_void,
    wmclass_class: *mut c_void,
    override_redirect: i32,
    type_hint: i32,
}

unsafe extern "C" {
    fn gtk_widget_get_parent_window(widget: *mut GtkWidget) -> *mut c_void;
    fn gtk_widget_get_allocation(widget: *mut GtkWidget, allocation: *mut GdkRectangle);
    fn gdk_window_new(
        parent: *mut c_void,
        attributes: *const WindowAttributes,
        mask: i32,
    ) -> *mut c_void;
    fn gdk_window_set_transient_for(window: *mut c_void, parent: *mut c_void);
    fn gdk_window_get_screen(window: *mut c_void) -> *mut c_void;
    fn gdk_screen_get_rgba_visual(screen: *mut c_void) -> *mut c_void;
    fn gdk_window_input_shape_combine_region(
        window: *mut c_void,
        region: *const c_void,
        x: i32,
        y: i32,
    );
    fn gdk_window_move_resize(window: *mut c_void, x: i32, y: i32, width: i32, height: i32);
    fn gdk_window_show(window: *mut c_void);
    fn gdk_window_hide(window: *mut c_void);
    fn gdk_window_destroy(window: *mut c_void);
    fn gdk_window_is_visible(window: *mut c_void) -> i32;
    fn gdk_window_get_display(window: *mut c_void) -> *mut GdkDisplay;
    fn gdk_window_get_parent(window: *mut c_void) -> *mut c_void;
    fn gdk_window_ensure_native(window: *mut c_void) -> i32;
    fn gdk_window_set_composited(window: *mut c_void, composited: i32);
    fn gtk_widget_get_toplevel(widget: *mut GtkWidget) -> *mut GtkWidget;
    fn cairo_save(cr: *mut c_void);
    fn cairo_restore(cr: *mut c_void);
    fn cairo_rectangle(cr: *mut c_void, x: f64, y: f64, width: f64, height: f64);
    fn cairo_clip(cr: *mut c_void);
    fn gdk_window_get_position(window: *mut c_void, x: *mut i32, y: *mut i32);
    fn gdk_display_supports_composite(display: *mut GdkDisplay) -> i32;
    fn gdk_cairo_set_source_window(cr: *mut c_void, window: *mut c_void, x: f64, y: f64);
    fn gtk_widget_queue_draw(widget: *mut GtkWidget);
    fn cairo_paint(cr: *mut c_void);
    fn cairo_region_create() -> *mut c_void;
    fn cairo_region_destroy(region: *mut c_void);
}

fn area(content: NativeContentHandle) -> *mut GtkWidget {
    let NativeContentHandle::Gtk(area) = content else {
        panic!("expected the GTK content area");
    };
    area.cast()
}

fn window(layer: NativeContentHandle) -> *mut c_void {
    let NativeContentHandle::GdkLayer(window) = layer else {
        panic!("expected a GDK layer window");
    };
    window
}

fn wayland() -> bool {
    unsafe { gdk_display_is_wayland(gdk_display_get_default()) }
}

/// Whether layers get native windows, which the Wayland compositor or X Composite blends.
pub(crate) unsafe fn native_layers(_: NativeContentHandle) -> bool {
    wayland()
        || unsafe {
            gdk_display_supports_composite(gdk_display_get_default()) != 0
                && !gdk_screen_get_rgba_visual(gdk_screen_get_default().cast()).is_null()
        }
}

/// Add a layer window over the content area, above or below a neighboring layer window, or
/// else above every other one.
pub(crate) unsafe fn add_layer_view(
    content: NativeContentHandle,
    neighbor: Option<(NativeContentHandle, bool)>,
) -> NativeContentHandle {
    let wayland = wayland();
    let area = area(content);
    unsafe {
        let parent = gtk_widget_get_parent_window(area);
        assert!(!parent.is_null(), "content area is not realized");
        let mut allocation = GdkRectangle::default();
        gtk_widget_get_allocation(area, &mut allocation);
        let attributes = WindowAttributes {
            title: null_mut(),
            event_mask: 0,
            x: allocation.x,
            y: allocation.y,
            width: allocation.width.max(1),
            height: allocation.height.max(1),
            wclass: GDK_INPUT_OUTPUT,
            visual: gdk_screen_get_rgba_visual(gdk_window_get_screen(parent)),
            window_type: if wayland {
                GDK_WINDOW_SUBSURFACE
            } else {
                GDK_WINDOW_CHILD
            },
            cursor: null_mut(),
            wmclass_name: null_mut(),
            wmclass_class: null_mut(),
            override_redirect: 0,
            type_hint: 0,
        };
        // GDK places a subsurface relative to a transient parent, not a parent window.
        let mask = GDK_WA_X | GDK_WA_Y | GDK_WA_VISUAL;
        let window = gdk_window_new(if wayland { null_mut() } else { parent }, &attributes, mask);
        assert!(!window.is_null(), "cannot create a layer window");
        if wayland {
            gdk_window_set_transient_for(window, parent);
        } else {
            // An X window of its own, redirected offscreen for the window to paint.
            gdk_window_ensure_native(window);
            gdk_window_set_composited(window, 1);
        }
        STACK.with_borrow_mut(|stack| {
            let index = neighbor
                .and_then(|(neighbor, above)| {
                    let neighbor = self::window(neighbor);
                    let index = stack.iter().position(|other| *other == neighbor)?;
                    Some(index + usize::from(above))
                })
                .unwrap_or(stack.len());
            stack.insert(index, window);
        });
        if !wayland && g_object_get_data(area.cast(), PAINTS_LAYERS.as_ptr()).is_null() {
            g_object_set_data(area.cast(), PAINTS_LAYERS.as_ptr(), area.cast());
            g_signal_connect_data(
                gtk_widget_get_toplevel(area).cast(),
                c"draw".as_ptr(),
                paint_window as *const c_void,
                area.cast(),
                null_mut(),
                1, // G_CONNECT_AFTER
            );
        }
        // The content area below gets all input.
        let empty = cairo_region_create();
        gdk_window_input_shape_combine_region(window, empty, 0, 0);
        cairo_region_destroy(empty);
        gdk_window_show(window);
        let state = LayerState {
            area,
            viewport: None,
            crop: None,
            hidden: false,
            outside: false,
        };
        LAYERS.with_borrow_mut(|layers| layers.insert(window as usize, state));
        NativeContentHandle::GdkLayer(window)
    }
}

/// Place a layer window at an area of the content area, or fill it, cropped to the content.
pub(crate) unsafe fn set_layer_view_frame(
    content: NativeContentHandle,
    layer: NativeContentHandle,
    frame: Option<LogicalRect>,
) {
    let window = window(layer);
    unsafe {
        let mut allocation = GdkRectangle::default();
        gtk_widget_get_allocation(area(content), &mut allocation);
        let (width, height) = (f64::from(allocation.width), f64::from(allocation.height));
        let [x, y, w, h] = frame.map_or([0.0, 0.0, width, height], |frame| {
            [frame.x, frame.y, frame.width, frame.height].map(f64::from)
        });
        // The part of the layer inside the content area.
        let (left, top) = (x.max(0.0), y.max(0.0));
        let (right, bottom) = ((x + w).min(width), (y + h).min(height));
        let outside = right <= left || bottom <= top;
        LAYERS.with_borrow_mut(|layers| {
            let Some(state) = layers.get_mut(&(window as usize)) else {
                return;
            };
            state.outside = outside;
            if !outside {
                // A subsurface starts at the visible part, an X window at the whole layer.
                let (origin_x, origin_y) = if wayland() { (left, top) } else { (x, y) };
                gdk_window_move_resize(
                    window,
                    allocation.x + origin_x.round() as i32,
                    allocation.y + origin_y.round() as i32,
                    (w.round() as i32).max(1),
                    (h.round() as i32).max(1),
                );
                let cropped = (left, top, right, bottom) != (x, y, x + w, y + h);
                if !wayland() {
                    // The window paints the layer windows clipped to the content area.
                    gtk_widget_queue_draw(gtk_widget_get_toplevel(state.area));
                } else if cropped || state.crop.is_some() {
                    state.crop = Some((
                        [left - x, top - y, right - left, bottom - top],
                        (right - left).round() as i32,
                        (bottom - top).round() as i32,
                    ));
                }
            }
            update_visibility(window, state);
        });
    }
}

// Show a layer window when it is neither hidden nor outside the content, cropped by its
// viewport on Wayland.
unsafe fn update_visibility(window: *mut c_void, state: &mut LayerState) {
    let visible = !state.hidden && !state.outside;
    unsafe {
        if visible != (gdk_window_is_visible(window) != 0) {
            if visible {
                gdk_window_show(window);
            } else {
                gdk_window_hide(window);
            }
            if !wayland() {
                gtk_widget_queue_draw(gtk_widget_get_toplevel(state.area));
            }
        }
        if visible && let Some((source, width, height)) = state.crop {
            // GDK gives a window a new surface when it is shown again.
            let surface = wayland_surface(window);
            if state
                .viewport
                .as_ref()
                .is_none_or(|viewport| viewport.surface() != surface)
            {
                state.viewport = Viewport::new(gdk_window_get_display(window), window);
            }
            if let Some(viewport) = &state.viewport {
                viewport.set(source, width, height);
            }
        }
    }
}

const PAINTS_LAYERS: &std::ffi::CStr = c"bwindow-paints-layers";

// Paint the visible X11 layer windows of a content area over the window from bottom to top,
// clipped to the area. GDK repaints the window when the X server reports damage of a
// redirected layer window.
extern "C" fn paint_window(toplevel: *mut GtkWidget, cr: *mut c_void, area: *mut GtkWidget) -> i32 {
    unsafe {
        let parent = gtk_widget_get_window(toplevel);
        let mut allocation = GdkRectangle::default();
        gtk_widget_get_allocation(area, &mut allocation);
        cairo_save(cr);
        cairo_rectangle(
            cr,
            f64::from(allocation.x),
            f64::from(allocation.y),
            f64::from(allocation.width),
            f64::from(allocation.height),
        );
        cairo_clip(cr);
        STACK.with_borrow(|stack| {
            for &window in stack {
                if gdk_window_get_parent(window) != parent || gdk_window_is_visible(window) == 0 {
                    continue;
                }
                let (mut x, mut y) = (0, 0);
                gdk_window_get_position(window, &mut x, &mut y);
                gdk_cairo_set_source_window(cr, window, f64::from(x), f64::from(y));
                cairo_paint(cr);
            }
        });
        cairo_restore(cr);
    }
    0
}

pub(crate) unsafe fn set_layer_view_hidden(layer: NativeContentHandle, hidden: bool) {
    let window = window(layer);
    LAYERS.with_borrow_mut(|layers| {
        if let Some(state) = layers.get_mut(&(window as usize)) {
            state.hidden = hidden;
            unsafe { update_visibility(window, state) };
        }
    });
}

pub(crate) unsafe fn remove_layer_view(layer: NativeContentHandle) {
    let window = window(layer);
    let state = LAYERS.with_borrow_mut(|layers| layers.remove(&(window as usize)));
    STACK.with_borrow_mut(|stack| stack.retain(|other| *other != window));
    unsafe {
        gdk_window_destroy(window);
        if let Some(state) = state
            && !wayland()
        {
            gtk_widget_queue_draw(gtk_widget_get_toplevel(state.area));
        }
    }
}
