/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The popover of an edit tool: its controls in a grid with labels, and Cancel and apply
//! buttons. Every change of a control sends all values to the application, which previews
//! them. It stays open while the image is used, like dragging the crop rectangle.

use std::cell::{Cell, RefCell};
use std::ffi::{CStr, CString, c_void};
use std::ptr::{null, null_mut};
use std::rc::Rc;

use bwindow::WindowId;
use imageview_core::edit::{Control, Tool};

use super::headers::*;
use super::toolbar::send;
use crate::AppEvent;

const SPACING: u32 = 8;
const INSET: i32 = 12;
const SLIDER_WIDTH: i32 = 180;
// The room of an overlay scroll bar next to the values.
const SCROLL_BAR: i32 = 12;
// What a popover needs of the height of its window besides its controls: the toolbar, title and
// buttons. GTK 3 cannot draw it outside of the window, and moves it next to its button instead.
const WINDOW_MARGIN: i32 = 220;
// The largest size of an image side.
const MAXIMUM_LENGTH: f64 = 1_000_000.0;

fn c_string(text: &str) -> CString {
    CString::new(text).expect("text contains no NUL")
}

// A control, with the label of the value of a slider.
struct Entry {
    control: Control,
    widget: *mut c_void,
    value: *mut c_void,
}

struct State {
    // The window the changes go to.
    window: WindowId,
    entries: RefCell<Vec<Entry>>,
    defaults: Vec<f32>,
    // Whether values are being shown, which does not count as a change.
    setting: Cell<bool>,
    // Whether all values go back to the defaults, also of the field being typed in.
    resetting: Cell<bool>,
}

// What the handlers of a control know.
struct Slot {
    state: Rc<State>,
    index: usize,
}

impl State {
    fn changed(&self, index: usize) {
        if self.setting.get() {
            return;
        }
        let values = self.values();
        self.show_values(&values);
        send(AppEvent::ToolChanged(self.window, index, values));
    }

    // The value of every control.
    fn values(&self) -> Vec<f32> {
        self.entries
            .borrow()
            .iter()
            .map(|entry| {
                // SAFETY: Every widget is the control of its entry, GTK owns the entry text.
                unsafe {
                    match entry.control {
                        Control::Slider(_) => gtk_range_get_value(entry.widget) as f32,
                        // Only the digits count, like while typing.
                        Control::Number { .. } => CStr::from_ptr(gtk_entry_get_text(entry.widget))
                            .to_string_lossy()
                            .chars()
                            .filter(char::is_ascii_digit)
                            .collect::<String>()
                            .parse::<u32>()
                            .map_or(0.0, |value| value as f32),
                        Control::Choice { .. } => {
                            gtk_combo_box_get_active(entry.widget).max(0) as f32
                        }
                        Control::Toggle { .. } => gtk_toggle_button_get_active(entry.widget) as f32,
                    }
                }
            })
            .collect()
    }

    // Shows values in the controls that differ.
    fn set_values(&self, values: &[f32]) {
        self.setting.set(true);
        let current = self.values();
        for ((entry, value), current) in self.entries.borrow().iter().zip(values).zip(current) {
            if *value == current {
                continue;
            }
            // SAFETY: Every widget is the control of its entry.
            unsafe {
                // Changes come back after later keys, so a field being typed in keeps its text.
                if matches!(entry.control, Control::Number { .. })
                    && gtk_widget_has_focus(entry.widget) != 0
                    && !self.resetting.get()
                {
                    continue;
                }
                match entry.control {
                    Control::Slider(_) => gtk_range_set_value(entry.widget, f64::from(*value)),
                    Control::Number { .. } => {
                        gtk_spin_button_set_value(entry.widget, f64::from(value.round()));
                    }
                    Control::Choice { .. } => {
                        gtk_combo_box_set_active(entry.widget, *value as i32);
                    }
                    Control::Toggle { .. } => {
                        gtk_toggle_button_set_active(entry.widget, i32::from(*value != 0.0));
                    }
                }
            }
        }
        self.setting.set(false);
        self.show_values(values);
    }

    // Shows the slider values next to them.
    fn show_values(&self, values: &[f32]) {
        for (entry, value) in self.entries.borrow().iter().zip(values) {
            if let Control::Slider(parameter) = entry.control {
                // Small ranges show a decimal.
                let text = if parameter.max - parameter.min <= 10.0 {
                    format!("{value:.1}")
                } else {
                    format!("{}", value.round() as i64)
                };
                // SAFETY: The label is a live child of the grid.
                unsafe { gtk_label_set_text(entry.value, c_string(&text).as_ptr()) };
            }
        }
    }
}

extern "C" fn control_changed(_: *mut c_void, slot: &Slot) {
    slot.state.changed(slot.index);
}

extern "C" fn drop_slot(slot: *mut c_void, _: *mut c_void) {
    // SAFETY: The signal owned the slot that it was connected with.
    drop(unsafe { Box::from_raw(slot.cast::<Slot>()) });
}

extern "C" fn drop_state(state: *mut c_void, _: *mut c_void) {
    // SAFETY: The signal owned a reference to the state.
    drop(unsafe { Rc::from_raw(state.cast::<State>().cast_const()) });
}

extern "C" fn reset(_: *mut c_void, state: &State) {
    state.resetting.set(true);
    state.set_values(&state.defaults);
    state.resetting.set(false);
    state.changed(usize::MAX);
}

extern "C" fn apply(_: *mut c_void, _: *mut c_void) {
    send(AppEvent::Action("tool.apply"));
}

extern "C" fn cancel(_: *mut c_void, _: *mut c_void) {
    send(AppEvent::Action("tool.cancel"));
}

// Escape cancels the tool.
extern "C" fn key_press(_: *mut c_void, event: *const c_void, _: *mut c_void) -> i32 {
    let mut keyval = 0;
    // SAFETY: GTK passes a live key event.
    unsafe { gdk_event_get_keyval(event, &mut keyval) };
    if keyval == GDK_KEY_ESCAPE {
        send(AppEvent::Action("tool.cancel"));
        return 1;
    }
    0
}

// A label with a colon at the end of its text.
unsafe fn label(text: &str) -> *mut c_void {
    // SAFETY: GTK copies the text.
    unsafe {
        let label = gtk_label_new(c_string(&format!("{text}:")).as_ptr());
        gtk_label_set_xalign(label, 1.0);
        label
    }
}

// A bold label.
unsafe fn heading(text: &str) -> *mut c_void {
    // SAFETY: GTK copies the text.
    unsafe {
        let label = gtk_label_new(null());
        gtk_label_set_markup(label, c_string(&format!("<b>{text}</b>")).as_ptr());
        gtk_label_set_xalign(label, 0.0);
        label
    }
}

// Creates the widgets of a control: its label, control and value label.
unsafe fn control_widgets(control: Control) -> (*mut c_void, *mut c_void, *mut c_void) {
    // SAFETY: GTK copies the strings, the caller adds the widgets to a container.
    unsafe {
        match control {
            Control::Slider(parameter) => {
                let range = f64::from(parameter.max - parameter.min);
                let slider = gtk_scale_new_with_range(
                    GTK_ORIENTATION_HORIZONTAL,
                    f64::from(parameter.min),
                    f64::from(parameter.max),
                    if range <= 10.0 { 0.1 } else { 1.0 },
                );
                gtk_scale_set_draw_value(slider, 0);
                gtk_widget_set_size_request(slider, SLIDER_WIDTH, -1);
                let value = gtk_label_new(null());
                gtk_label_set_xalign(value, 1.0);
                gtk_label_set_width_chars(value, 5);
                (label(parameter.label), slider, value)
            }
            Control::Number { label: text } => {
                let field = gtk_spin_button_new_with_range(1.0, MAXIMUM_LENGTH, 1.0);
                gtk_spin_button_set_numeric(field, 1);
                (label(text), field, gtk_label_new(c"pixels".as_ptr()))
            }
            Control::Choice {
                label: text,
                options,
            } => {
                let choice = gtk_combo_box_text_new();
                for option in options {
                    gtk_combo_box_text_append_text(choice, c_string(option).as_ptr());
                }
                (label(text), choice, null_mut())
            }
            Control::Toggle { label: text } => (
                null_mut(),
                gtk_check_button_new_with_label(c_string(text).as_ptr()),
                null_mut(),
            ),
        }
    }
}

/// The popover of an open tool, which closes when dropped.
pub(super) struct Popover {
    popover: *mut c_void,
    state: Rc<State>,
}

impl Popover {
    /// Creates a popover with controls of a tool showing values at a widget for
    /// `window`, reset goes back to the defaults.
    pub(super) unsafe fn new(
        anchor: *mut c_void,
        window: WindowId,
        tool: Tool,
        controls: &[Control],
        values: &[f32],
        defaults: &[f32],
    ) -> Self {
        let state = Rc::new(State {
            window,
            entries: RefCell::new(Vec::new()),
            defaults: defaults.to_vec(),
            setting: Cell::new(false),
            resetting: Cell::new(false),
        });
        // SAFETY: Containers own their children. The signals own their data until the widgets
        // are destroyed with the popover.
        unsafe {
            let grid = gtk_grid_new();
            gtk_grid_set_row_spacing(grid, SPACING);
            gtk_grid_set_column_spacing(grid, SPACING);
            let mut row = 0;
            for control in controls.iter().copied() {
                if let Control::Slider(parameter) = control
                    && !parameter.section.is_empty()
                {
                    gtk_grid_attach(grid, heading(parameter.section), 0, row, 3, 1);
                    row += 1;
                }
                let (name, widget, value) = control_widgets(control);
                if !name.is_null() {
                    gtk_grid_attach(grid, name, 0, row, 1, 1);
                }
                gtk_grid_attach(grid, widget, 1, row, 1, 1);
                if !value.is_null() {
                    gtk_grid_attach(grid, value, 2, row, 1, 1);
                }
                row += 1;
                state.entries.borrow_mut().push(Entry {
                    control,
                    widget,
                    value,
                });
            }
            // Show the values before following changes.
            state.setting.set(true);
            for entry in state.entries.borrow().iter() {
                if let Control::Choice { .. } = entry.control {
                    gtk_combo_box_set_active(entry.widget, 0);
                }
            }
            state.setting.set(false);
            state.set_values(values);
            for (index, entry) in state.entries.borrow().iter().enumerate() {
                let signal = match entry.control {
                    Control::Slider(_) => c"value-changed",
                    Control::Number { .. } | Control::Choice { .. } => c"changed",
                    Control::Toggle { .. } => c"toggled",
                };
                let slot = Box::new(Slot {
                    state: Rc::clone(&state),
                    index,
                });
                g_signal_connect_data(
                    entry.widget,
                    signal.as_ptr(),
                    control_changed as *const c_void,
                    Box::into_raw(slot).cast(),
                    drop_slot as *const c_void,
                    0,
                );
                if let Control::Number { .. } = entry.control {
                    g_signal_connect_data(
                        entry.widget,
                        c"activate".as_ptr(),
                        apply as *const c_void,
                        null(),
                        null(),
                        0,
                    );
                }
            }

            let buttons = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 6);
            if tool.previews() {
                let reset_button = gtk_button_new_with_label(c"Reset".as_ptr());
                g_signal_connect_data(
                    reset_button,
                    c"clicked".as_ptr(),
                    reset as *const c_void,
                    Rc::into_raw(Rc::clone(&state)).cast(),
                    drop_state as *const c_void,
                    0,
                );
                gtk_box_pack_start(buttons, reset_button, 0, 0, 0);
            }
            let apply_button = gtk_button_new_with_label(c_string(tool.apply_label()).as_ptr());
            gtk_style_context_add_class(
                gtk_widget_get_style_context(apply_button),
                c"suggested-action".as_ptr(),
            );
            let cancel_button = gtk_button_new_with_label(c"Cancel".as_ptr());
            for (button, handler) in [
                (apply_button, apply as *const c_void),
                (cancel_button, cancel as *const c_void),
            ] {
                g_signal_connect_data(button, c"clicked".as_ptr(), handler, null(), null(), 0);
                gtk_box_pack_end(buttons, button, 0, 0, 0);
            }

            // Tall tools scroll in small windows.
            let scrolled = gtk_scrolled_window_new(null_mut(), null_mut());
            gtk_scrolled_window_set_policy(scrolled, GTK_POLICY_NEVER, GTK_POLICY_AUTOMATIC);
            gtk_container_add(scrolled, grid);
            gtk_widget_set_margin_end(grid, SCROLL_BAR);
            gtk_widget_show_all(grid);
            let mut natural = [0; 2];
            gtk_widget_get_preferred_size(grid, null_mut(), &mut natural);
            let window = gtk_widget_get_toplevel(anchor);
            let room = (gtk_widget_get_allocated_height(window) - WINDOW_MARGIN).max(INSET * 8);
            gtk_widget_set_size_request(scrolled, natural[0], natural[1].min(room));

            let content = gtk_box_new(GTK_ORIENTATION_VERTICAL, SPACING as i32);
            for margin in [
                gtk_widget_set_margin_top,
                gtk_widget_set_margin_bottom,
                gtk_widget_set_margin_start,
                gtk_widget_set_margin_end,
            ] {
                margin(content, INSET);
            }
            gtk_box_pack_start(content, heading(tool.title()), 0, 0, 0);
            gtk_box_pack_start(content, scrolled, 1, 1, 0);
            gtk_box_pack_start(content, buttons, 0, 0, 0);

            // The window owns the popover, the reference keeps it alive until it is dropped,
            // also when the window is destroyed first.
            let popover = g_object_ref_sink(gtk_popover_new(anchor));
            gtk_popover_set_modal(popover, 0);
            gtk_popover_set_position(popover, GTK_POS_BOTTOM);
            gtk_container_add(popover, content);
            g_signal_connect_data(
                popover,
                c"key-press-event".as_ptr(),
                key_press as *const c_void,
                null(),
                null(),
                0,
            );
            gtk_widget_show_all(content);
            gtk_widget_show(popover);
            Self { popover, state }
        }
    }

    /// Shows changed values, like the other side of a proportional resize.
    pub(super) fn set_values(&self, values: &[f32]) {
        self.state.set_values(values);
    }
}

impl Drop for Popover {
    fn drop(&mut self) {
        // SAFETY: This owns a reference to the popover, which keeps it alive until it is
        // destroyed with its controls.
        unsafe {
            gtk_widget_destroy(self.popover);
            g_object_unref(self.popover);
        }
    }
}
