/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Editing in the GTK media view: the edit controls, the popover of the open tool, the preview,
//! the crop rectangle and selection that the primary button draws, and copying to the clipboard.

use std::ffi::c_void;

use bcanvas::NativeContext;
use bwindow::WindowId;
use imageview_core::Media;
use imageview_core::edit::{Control, Selection, Shape, Tool};

use super::headers::*;
use super::popover::Popover;
use super::toolbar::send;
use super::{MediaView, State, add_spinner};
use crate::AppEvent;
use crate::viewport::Viewport;

impl MediaView {
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
            self.state.mark(|viewport| viewport.set_cropping(true));
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
            self.state.mark(|viewport| viewport.set_cropping(false));
            self.state.show_cursor();
            // SAFETY: The drawing area is alive while this view is.
            unsafe { gtk_widget_grab_focus(self.state.area) };
        }
    }

    /// Makes drags draw selections of a shape, or selects the whole image again, shown as on
    /// in the toolbar.
    pub(crate) fn set_selecting(&self, shape: Option<Shape>) {
        self.state.mark(|viewport| viewport.set_selecting(shape));
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
        self.state.mark(Viewport::invert_selection);
    }

    /// Puts a PNG image on the clipboard, for as long as this application lives or the desktop
    /// keeps a copy.
    pub(crate) fn copy_png(&self, png: &[u8]) {
        let targets = [GtkTargetEntry {
            target: c"image/png".as_ptr(),
            flags: 0,
            info: 0,
        }];
        let png = Box::into_raw(Box::new(png.to_vec())).cast::<c_void>();
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
        self.state.mark(|viewport| {
            if let Some(crop) = viewport.crop() {
                crop.set_rect(rect);
            }
        });
    }

    /// Makes the crop rectangle keep a width divided by height, or any shape.
    pub(crate) fn set_crop_aspect(&self, aspect: Option<f64>) {
        self.state.mark(|viewport| {
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
}

impl State {
    // Changes only the crop rectangle or selection, which are drawn over the cached media.
    fn mark(&self, change: impl FnOnce(&mut Viewport)) {
        change(&mut self.viewport.borrow_mut());
        self.redraw();
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

// Draws the crop rectangle or selection over the drawing area of a size and scale factor.
pub(super) unsafe fn paint_marks(
    cr: *mut c_void,
    state: &State,
    (width, height, scale): (i32, i32, i32),
) {
    // SAFETY: The caller supplies a cairo context to draw with.
    unsafe {
        bcanvas::draw_native(
            NativeContext::Cairo(cr),
            width as f32,
            height as f32,
            scale as f32,
            |canvas| state.viewport.borrow().paint_document_marks(canvas),
        );
    }
}

// Return applies the open tool and escape cancels it, returns whether the key is one of them.
pub(super) fn key_press(keyval: u32) -> bool {
    let action = match keyval {
        GDK_KEY_RETURN | GDK_KEY_KP_ENTER => "tool.apply",
        GDK_KEY_ESCAPE => "tool.cancel",
        _ => return false,
    };
    send(AppEvent::Action(action));
    true
}

// The position of the pointer of an event in the visible area.
fn pointer(event: *const c_void, state: &State) -> (f64, f64) {
    let (mut x, mut y) = (0.0, 0.0);
    // SAFETY: GTK passes a live pointer event.
    unsafe { gdk_event_get_coords(event, &mut x, &mut y) };
    state.visible_point((x, y))
}

// Takes hold of the crop rectangle, the selection or a new selection at the pointer of a button
// press, returns whether it is one of them instead of the media.
pub(super) fn mark_begin(event: *const c_void, state: &State) -> bool {
    let marked = state
        .viewport
        .borrow_mut()
        .mark_begin(pointer(event, state));
    if marked {
        state.redraw();
    }
    marked
}

// Moves the held crop rectangle or selection along with the pointer of a motion, as a square
// with shift, returns whether one is held.
pub(super) fn mark_drag(event: *const c_void, state: &State) -> bool {
    let mut modifiers = 0;
    // SAFETY: GTK passes a live motion event.
    unsafe { gdk_event_get_state(event, &mut modifiers) };
    let square = modifiers & GDK_SHIFT_MASK != 0;
    let marked = state
        .viewport
        .borrow_mut()
        .mark_drag(pointer(event, state), square);
    if marked {
        state.redraw();
    }
    marked
}

// Lets go of the held crop rectangle or selection, returns whether one was held. A click
// without a drag goes back to editing the whole image, the menu follows a drawn selection.
pub(super) fn mark_end(state: &State) -> bool {
    let (marked, cleared) = {
        let mut viewport = state.viewport.borrow_mut();
        let marked = viewport.mark_end();
        let cleared = viewport.selecting().is_some() && viewport.selection().is_none();
        (marked, marked && cleared)
    };
    if cleared {
        send(AppEvent::Action("select.whole"));
    } else if marked {
        send(AppEvent::Action("select.changed"));
    }
    if marked {
        state.redraw();
    }
    marked
}
