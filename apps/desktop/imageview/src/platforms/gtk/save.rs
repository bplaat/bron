/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The controls of the Save As dialog below its file chooser. The chooser destroys them when it
//! closes, so their values are followed here.

use std::cell::Cell;
use std::ffi::{CStr, CString, c_void};
use std::ptr::null;
use std::rc::Rc;

use bwindow::NativeFileDialog;

use super::headers::*;
use crate::save::{
    SaveRequest, format_of, has_quality, natural_scale, scale_at, scale_titles, with_extension,
};

const SPACING: u32 = 8;
const QUALITY_WIDTH: i32 = 160;

fn c_string(text: &str) -> CString {
    CString::new(text).expect("text contains no NUL")
}

struct State {
    chooser: *mut c_void,
    extensions: Vec<&'static str>,
    format: *mut c_void,
    quality_label: *mut c_void,
    quality_slider: *mut c_void,
    quality_value: *mut c_void,
    quality: Cell<u8>,
    scale: Cell<usize>,
}

impl State {
    // Changes the extension of the file name to the chosen format and shows the quality of
    // JPEG.
    fn format_changed(&self) {
        // SAFETY: The chooser and its controls are live while their signals run, GLib owns the
        // copy of the name until it is freed.
        unsafe {
            let index = gtk_combo_box_get_active(self.format).max(0) as usize;
            let extension = self.extensions[index];
            let raw = gtk_file_chooser_get_current_name(self.chooser);
            if !raw.is_null() {
                let name = CStr::from_ptr(raw).to_string_lossy().into_owned();
                g_free(raw.cast());
                let name = c_string(&with_extension(&name, extension));
                gtk_file_chooser_set_current_name(self.chooser, name.as_ptr());
            }
            let quality = i32::from(has_quality(extension));
            for widget in [self.quality_label, self.quality_slider, self.quality_value] {
                gtk_widget_set_visible(widget, quality);
            }
        }
    }

    fn show_quality(&self) {
        let text = c_string(&format!("{}%", self.quality.get()));
        // SAFETY: The label is live while the chooser is.
        unsafe { gtk_label_set_text(self.quality_value, text.as_ptr()) };
    }
}

extern "C" fn format_changed(_: *mut c_void, state: &State) {
    state.format_changed();
}

extern "C" fn quality_changed(slider: *mut c_void, state: &State) {
    // SAFETY: GTK passes the live slider.
    let quality = unsafe { gtk_range_get_value(slider) };
    state.quality.set(quality.round().clamp(1.0, 100.0) as u8);
    state.show_quality();
}

extern "C" fn scale_changed(combo: *mut c_void, state: &State) {
    // SAFETY: GTK passes the live combo box.
    let index = unsafe { gtk_combo_box_get_active(combo) };
    state.scale.set(index.max(0) as usize);
}

extern "C" fn drop_state(state: *mut c_void, _: *mut c_void) {
    // SAFETY: The signal owned a reference to the state.
    drop(unsafe { Rc::from_raw(state.cast::<State>().cast_const()) });
}

// A label at the end of its grid column.
unsafe fn label(text: &CStr) -> *mut c_void {
    // SAFETY: GTK copies the text.
    unsafe {
        let label = gtk_label_new(text.as_ptr());
        gtk_label_set_xalign(label, 1.0);
        label
    }
}

unsafe fn connect(widget: *mut c_void, signal: &CStr, handler: *const c_void, state: &Rc<State>) {
    // SAFETY: The handlers take the state, which the signal keeps alive with this reference.
    unsafe {
        g_signal_connect_data(
            widget,
            signal.as_ptr(),
            handler,
            Rc::into_raw(Rc::clone(state)).cast(),
            drop_state as *const c_void,
            0,
        );
    }
}

/// The values of the controls of a file chooser.
pub(crate) struct SaveOptions {
    state: Rc<State>,
}

impl SaveOptions {
    /// Adds the controls to the file chooser of a dialog.
    pub(crate) fn new(native: NativeFileDialog, request: &SaveRequest<'_>) -> Option<Self> {
        let NativeFileDialog::Gtk(chooser) = native else {
            return None;
        };
        // SAFETY: bwindow passes a live file chooser, which owns the grid with its children.
        unsafe {
            gtk_file_chooser_set_do_overwrite_confirmation(chooser, 1);
            let format = gtk_combo_box_text_new();
            for (name, _) in request.formats {
                gtk_combo_box_text_append_text(format, c_string(name).as_ptr());
            }
            gtk_combo_box_set_active(format, format_of(request.name, request.formats) as i32);
            let slider = gtk_scale_new_with_range(GTK_ORIENTATION_HORIZONTAL, 1.0, 100.0, 1.0);
            gtk_scale_set_draw_value(slider, 0);
            gtk_range_set_value(slider, f64::from(request.quality));
            gtk_widget_set_size_request(slider, QUALITY_WIDTH, -1);
            let state = Rc::new(State {
                chooser,
                extensions: request
                    .formats
                    .iter()
                    .map(|(_, extension)| *extension)
                    .collect(),
                format,
                quality_label: label(c"Quality:"),
                quality_slider: slider,
                quality_value: gtk_label_new(null()),
                quality: Cell::new(request.quality),
                scale: Cell::new(natural_scale()),
            });
            let grid = gtk_grid_new();
            gtk_grid_set_row_spacing(grid, SPACING);
            gtk_grid_set_column_spacing(grid, SPACING);
            gtk_widget_set_halign(grid, GTK_ALIGN_CENTER);
            gtk_grid_attach(grid, label(c"Format:"), 0, 0, 1, 1);
            gtk_grid_attach(grid, format, 1, 0, 1, 1);
            gtk_grid_attach(grid, state.quality_label, 0, 1, 1, 1);
            gtk_grid_attach(grid, slider, 1, 1, 1, 1);
            gtk_grid_attach(grid, state.quality_value, 2, 1, 1, 1);
            // A vector is drawn at a scale of its natural size, shown with its pixel size.
            if let Some(size) = request.size {
                let scales = gtk_combo_box_text_new();
                for title in scale_titles(size) {
                    gtk_combo_box_text_append_text(scales, c_string(&title).as_ptr());
                }
                gtk_combo_box_set_active(scales, natural_scale() as i32);
                connect(scales, c"changed", scale_changed as *const c_void, &state);
                gtk_grid_attach(grid, label(c"Size:"), 0, 2, 1, 1);
                gtk_grid_attach(grid, scales, 1, 2, 1, 1);
            }
            connect(format, c"changed", format_changed as *const c_void, &state);
            connect(
                slider,
                c"value-changed",
                quality_changed as *const c_void,
                &state,
            );
            gtk_widget_show_all(grid);
            gtk_file_chooser_set_extra_widget(chooser, grid);
            state.format_changed();
            state.show_quality();
            Some(Self { state })
        }
    }

    /// The chosen JPEG quality.
    pub(crate) fn quality(&self) -> u8 {
        self.state.quality.get()
    }

    /// The chosen scale of a vector.
    pub(crate) fn scale(&self) -> f64 {
        scale_at(self.state.scale.get())
    }
}
