/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The popover of an edit tool: its controls in a grid with labels, and Cancel and apply
//! buttons. Every change of a control sends all values to the application, which previews
//! them.

use std::cell::RefCell;
use std::ptr::null_mut;

use bwindow::WindowId;
use imageview_core::edit::{Control, Tool};
use imageview_core::macos::{Rect, Size, ns_string};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject as Object, Bool, Sel};
use objc2::{Encode, Encoding, class, define_class, msg_send, sel};

use super::headers::*;
use super::view::rect;
use crate::AppEvent;

const NS_CONTROL_STATE_VALUE_ON: isize = 1;
const NS_GRID_CELL_PLACEMENT_LEADING: isize = 2;
const NS_POPOVER_BEHAVIOR_APPLICATION_DEFINED: isize = 0;
// The bottom edge of a rectangle in a flipped view.
const NS_RECT_EDGE_MAX_Y: u64 = 3;
const NS_STACK_VIEW_GRAVITY_LEADING: isize = 1;
const NS_STACK_VIEW_GRAVITY_TRAILING: isize = 3;
const SLIDER_WIDTH: f64 = 200.0;
const FIELD_WIDTH: f64 = 72.0;
const VALUE_WIDTH: f64 = 40.0;
const SPACING: f64 = 12.0;
const INSET: f64 = 16.0;

#[repr(C)]
struct EdgeInsets([f64; 4]);

// SAFETY: NSEdgeInsets is a struct of four CGFloats: top, left, bottom and right.
unsafe impl Encode for EdgeInsets {
    const ENCODING: Encoding = Encoding::Struct("NSEdgeInsets", &[f64::ENCODING; 4]);
}

#[repr(C)]
struct Range {
    location: usize,
    length: usize,
}

// SAFETY: NSRange is a struct of two NSUIntegers: location and length.
unsafe impl Encode for Range {
    const ENCODING: Encoding = Encoding::Struct("_NSRange", &[usize::ENCODING; 2]);
}

// A control of the popover with its native view and the label that shows a slider value.
struct Entry {
    control: Control,
    view: Retained<Object>,
    value: Option<Retained<Object>>,
}

pub(super) struct TargetIvars {
    // The window the changes go to.
    window: WindowId,
    entries: RefCell<Vec<Entry>>,
    defaults: Vec<f32>,
}

define_class!(
    // Receives the changes of the controls and buttons of a tool popover.
    #[unsafe(super(NSObject))]
    #[name = "ImageViewToolTarget"]
    #[ivars = TargetIvars]
    pub(super) struct Target;

    impl Target {
        #[unsafe(method(controlChanged:))]
        fn _control_changed(&self, sender: *mut Object) {
            // SAFETY: The sender is one of the controls, tagged with its index.
            let index: isize = unsafe { msg_send![sender, tag] };
            self.changed(index as usize);
        }

        // Number fields change with every typed character.
        #[unsafe(method(controlTextDidChange:))]
        fn _control_text_did_change(&self, notification: *mut Object) {
            // SAFETY: The notification object is the field that changed.
            let index: isize = unsafe {
                let field: *mut Object = msg_send![notification, object];
                msg_send![field, tag]
            };
            self.changed(index as usize);
        }

        #[unsafe(method(reset:))]
        fn _reset(&self, _: *mut Object) {
            self.set_values(&self.ivars().defaults);
            self.changed(usize::MAX);
        }

        #[unsafe(method(apply:))]
        fn _apply(&self, _: *mut Object) {
            super::send(AppEvent::Action("tool.apply"));
        }

        #[unsafe(method(cancel:))]
        fn _cancel(&self, _: *mut Object) {
            super::send(AppEvent::Action("tool.cancel"));
        }
    }
);

impl Target {
    fn changed(&self, index: usize) {
        let values = self.values();
        self.show_values(&values);
        super::send(AppEvent::ToolChanged(self.ivars().window, index, values));
    }

    // The value of every control.
    fn values(&self) -> Vec<f32> {
        self.ivars()
            .entries
            .borrow()
            .iter()
            .map(|entry| {
                // SAFETY: Every view is the control of its entry.
                unsafe {
                    match entry.control {
                        // In the steps of the sliders of the other platforms, which the value
                        // labels show.
                        Control::Slider(parameter) => {
                            let value: f64 = msg_send![&*entry.view, doubleValue];
                            let steps = if parameter.max - parameter.min <= 10.0 {
                                10.0
                            } else {
                                1.0
                            };
                            ((value * steps).round() / steps) as f32
                        }
                        // Only the digits count, whatever separators are typed.
                        Control::Number { .. } => {
                            let text: *mut Object = msg_send![&*entry.view, stringValue];
                            string(text)
                                .chars()
                                .filter(char::is_ascii_digit)
                                .collect::<String>()
                                .parse::<u32>()
                                .map_or(0.0, |value| value as f32)
                        }
                        Control::Choice { .. } => {
                            let index: isize = msg_send![&*entry.view, indexOfSelectedItem];
                            index.max(0) as f32
                        }
                        Control::Toggle { .. } => {
                            let state: isize = msg_send![&*entry.view, state];
                            if state == NS_CONTROL_STATE_VALUE_ON {
                                1.0
                            } else {
                                0.0
                            }
                        }
                    }
                }
            })
            .collect()
    }

    // Shows values in the controls that differ, so a field being typed in keeps its text.
    fn set_values(&self, values: &[f32]) {
        let current = self.values();
        for ((entry, value), current) in self
            .ivars()
            .entries
            .borrow()
            .iter()
            .zip(values)
            .zip(current)
        {
            if *value == current {
                continue;
            }
            // SAFETY: Every view is the control of its entry.
            unsafe {
                match entry.control {
                    Control::Slider(_) => {
                        let _: () = msg_send![&*entry.view, setDoubleValue: f64::from(*value)];
                    }
                    Control::Number { .. } => {
                        // Plain digits, without the separators of the locale.
                        let text = format!("{}", value.round() as u32);
                        let _: () = msg_send![&*entry.view, setStringValue: ns_string(&text)];
                    }
                    Control::Choice { .. } => {
                        let _: () = msg_send![&*entry.view, selectItemAtIndex: *value as isize];
                    }
                    Control::Toggle { .. } => {
                        let _: () = msg_send![&*entry.view, setState: isize::from(*value != 0.0)];
                    }
                }
            }
        }
        self.show_values(values);
    }

    // Shows the slider values next to them.
    fn show_values(&self, values: &[f32]) {
        for (entry, value) in self.ivars().entries.borrow().iter().zip(values) {
            if let (Control::Slider(parameter), Some(label)) = (entry.control, &entry.value) {
                let text = parameter.value_text(*value);
                // SAFETY: The label is a live text field.
                unsafe {
                    let _: () = msg_send![&**label, setStringValue: ns_string(&text)];
                }
            }
        }
    }
}

/// The popover of a tool.
pub(super) struct Popover {
    popover: Retained<Object>,
    target: Retained<Target>,
    // The Reset, Cancel and apply buttons, which send to the target.
    buttons: Vec<Retained<Object>>,
}

impl Popover {
    /// Creates a popover with controls of a tool showing values for `window`, reset goes back
    /// to the defaults.
    pub(super) fn new(
        window: WindowId,
        tool: Tool,
        controls: &[Control],
        values: &[f32],
        defaults: &[f32],
    ) -> Self {
        let defaults = defaults.to_vec();
        // SAFETY: NSObject's `init` returns the initialized target.
        let target: Retained<Target> = unsafe {
            let target: Allocated<Target> = msg_send![Target::class(), alloc];
            msg_send![
                super(target.set_ivars(TargetIvars {
                    window,
                    entries: RefCell::new(Vec::new()),
                    defaults,
                })),
                init
            ]
        };
        let target_ptr = Retained::as_ptr(&target).cast::<Object>();
        // SAFETY: All objects are valid AppKit instances, the popover retains its controller
        // and views, the target outlives them in this struct.
        unsafe {
            let grid: Retained<Object> = msg_send![class!(NSGridView), new];
            let _: () = msg_send![&*grid, setRowSpacing: SPACING - 4.0];
            let _: () = msg_send![&*grid, setColumnSpacing: 8.0];
            let _: () = msg_send![&*grid, setRowAlignment: NS_GRID_ROW_ALIGNMENT_FIRST_BASELINE];
            let mut entries = Vec::new();
            for (index, control) in controls.iter().copied().enumerate() {
                if let Control::Slider(parameter) = control
                    && !parameter.section.is_empty()
                {
                    add_heading(&grid, parameter.section, index > 0);
                }
                let (label, view, value) = control_views(control, index, target_ptr);
                let row: *mut Object = msg_send![class!(NSMutableArray), array];
                for cell in [label, Some(Retained::as_ptr(&view))]
                    .into_iter()
                    .chain([value.as_ref().map(Retained::as_ptr)])
                {
                    let cell =
                        cell.unwrap_or_else(|| msg_send![class!(NSGridCell), emptyContentView]);
                    let _: () = msg_send![row, addObject: cell];
                }
                let _: *mut Object = msg_send![&*grid, addRowWithViews: row];
                entries.push(Entry {
                    control,
                    view,
                    value,
                });
            }
            let column: *mut Object = msg_send![&*grid, columnAtIndex: 0isize];
            let _: () = msg_send![column, setXPlacement: NS_GRID_CELL_PLACEMENT_TRAILING];
            *target.ivars().entries.borrow_mut() = entries;
            target.set_values(values);

            let title: *mut Object =
                msg_send![class!(NSTextField), labelWithString: ns_string(tool.title())];
            let font: *mut Object = msg_send![class!(NSFont), boldSystemFontOfSize: 13.0f64];
            let _: () = msg_send![title, setFont: font];

            let buttons: Retained<Object> = msg_send![class!(NSStackView), new];
            let mut targeting = Vec::new();
            if tool.previews() {
                let reset = text_button("Reset", sel!(reset:), target_ptr);
                let _: () =
                    msg_send![&*buttons, addView: reset, inGravity: NS_STACK_VIEW_GRAVITY_LEADING];
                targeting.extend(Retained::retain(reset));
            }
            let cancel = text_button("Cancel", sel!(cancel:), target_ptr);
            let _: () = msg_send![cancel, setKeyEquivalent: ns_string!("\u{1b}")];
            let apply = text_button(tool.apply_label(), sel!(apply:), target_ptr);
            let _: () = msg_send![apply, setKeyEquivalent: ns_string!("\r")];
            for button in [cancel, apply] {
                let _: () = msg_send![&*buttons, addView: button, inGravity: NS_STACK_VIEW_GRAVITY_TRAILING];
                targeting.extend(Retained::retain(button));
            }

            let views: *mut Object = msg_send![class!(NSMutableArray), array];
            for view in [title, Retained::as_ptr(&grid), Retained::as_ptr(&buttons)] {
                let _: () = msg_send![views, addObject: view];
            }
            let stack: *mut Object = msg_send![class!(NSStackView), stackViewWithViews: views];
            let _: () =
                msg_send![stack, setOrientation: NS_USER_INTERFACE_LAYOUT_ORIENTATION_VERTICAL];
            let _: () = msg_send![stack, setAlignment: NS_LAYOUT_ATTRIBUTE_LEADING];
            let _: () = msg_send![stack, setSpacing: SPACING];
            let _: () = msg_send![stack, setEdgeInsets: EdgeInsets([INSET; 4])];
            // The buttons span the grid, so apply is at its trailing edge.
            let buttons_width: *mut Object = msg_send![&*buttons, widthAnchor];
            let grid_width: *mut Object = msg_send![&*grid, widthAnchor];
            let constraint: *mut Object =
                msg_send![buttons_width, constraintEqualToAnchor: grid_width];
            let _: () = msg_send![constraint, setActive: Bool::YES];
            let _: () = msg_send![stack, layoutSubtreeIfNeeded];
            let size: Size = msg_send![stack, fittingSize];

            let controller: Retained<Object> = msg_send![class!(NSViewController), new];
            let _: () = msg_send![&*controller, setView: stack];
            let _: () = msg_send![&*controller, setPreferredContentSize: size];
            let popover: Retained<Object> = msg_send![class!(NSPopover), new];
            let _: () = msg_send![&*popover, setBehavior: NS_POPOVER_BEHAVIOR_APPLICATION_DEFINED];
            let _: () = msg_send![&*popover, setContentViewController: &*controller];
            Self {
                popover,
                target,
                buttons: targeting,
            }
        }
    }

    /// Shows the popover below a toolbar item, or the top of a view without one.
    pub(super) fn show(&self, item: Option<*mut Object>, view: *mut Object) {
        // SAFETY: The popover, item and view are live AppKit objects.
        unsafe {
            let anchored: Bool =
                msg_send![&*self.popover, respondsToSelector: sel!(showRelativeToToolbarItem:)];
            match item {
                Some(item) if anchored.as_bool() => {
                    let _: () = msg_send![&*self.popover, showRelativeToToolbarItem: item];
                }
                _ => {
                    let bounds: Rect = msg_send![view, bounds];
                    // The view is flipped, so the popover hangs below its top edge.
                    let _: () = msg_send![&*self.popover,
                        showRelativeToRect: rect(bounds.size.width / 2.0, 0.0, 1.0, 1.0),
                        ofView: view,
                        preferredEdge: NS_RECT_EDGE_MAX_Y
                    ];
                }
            }
        }
    }

    /// Shows changed values, like the other side of a proportional resize.
    pub(super) fn set_values(&self, values: &[f32]) {
        self.target.set_values(values);
    }
}

impl Drop for Popover {
    fn drop(&mut self) {
        // SAFETY: The popover and controls are live AppKit objects. Their targets and the
        // delegates of fields are not retained, and the controls can outlive the target in the
        // closing popover, which ends the editing of a field, so they are cleared first.
        unsafe {
            for entry in self.target.ivars().entries.borrow().iter() {
                let _: () = msg_send![&*entry.view, setTarget: null_mut::<Object>()];
                if let Control::Number { .. } = entry.control {
                    let _: () = msg_send![&*entry.view, setDelegate: null_mut::<Object>()];
                }
            }
            for button in &self.buttons {
                let _: () = msg_send![&**button, setTarget: null_mut::<Object>()];
            }
            let _: () = msg_send![&*self.popover, close];
        }
    }
}

// Adds a bold heading over all columns, with room above it after other rows.
unsafe fn add_heading(grid: &Object, text: &str, spaced: bool) {
    // SAFETY: The grid retains its rows and the autoreleased label.
    unsafe {
        let heading: *mut Object = msg_send![class!(NSTextField), labelWithString: ns_string(text)];
        let font: *mut Object = msg_send![class!(NSFont), boldSystemFontOfSize: 0.0f64];
        let _: () = msg_send![heading, setFont: font];
        let row: *mut Object = msg_send![class!(NSMutableArray), array];
        let _: () = msg_send![row, addObject: heading];
        let empty: *mut Object = msg_send![class!(NSGridCell), emptyContentView];
        let _: () = msg_send![row, addObject: empty];
        let _: () = msg_send![row, addObject: empty];
        let row: *mut Object = msg_send![grid, addRowWithViews: row];
        let _: () = msg_send![row, mergeCellsInRange: Range { location: 0, length: 3 }];
        let cell: *mut Object = msg_send![row, cellAtIndex: 0isize];
        let _: () = msg_send![cell, setXPlacement: NS_GRID_CELL_PLACEMENT_LEADING];
        if spaced {
            let _: () = msg_send![row, setTopPadding: 8.0f64];
        }
    }
}

// The label, control and value label of a control, which reports to the target.
unsafe fn control_views(
    control: Control,
    index: usize,
    target: *mut Object,
) -> (
    Option<*mut Object>,
    Retained<Object>,
    Option<Retained<Object>>,
) {
    let label = |text: &str| -> *mut Object {
        // SAFETY: The label is autoreleased until the grid retains it.
        unsafe { msg_send![class!(NSTextField), labelWithString: ns_string(&format!("{text}:"))] }
    };
    let width = |view: &Object, width: f64| {
        // SAFETY: The constraint is retained by the view it is activated on.
        unsafe {
            let anchor: *mut Object = msg_send![view, widthAnchor];
            let constraint: *mut Object = msg_send![anchor, constraintEqualToConstant: width];
            let _: () = msg_send![constraint, setActive: Bool::YES];
        }
    };
    // SAFETY: All objects are valid AppKit instances that report to the live target.
    unsafe {
        let (label, view, value) = match control {
            Control::Slider(parameter) => {
                let slider: *mut Object = msg_send![class!(NSSlider),
                    sliderWithValue: f64::from(parameter.default),
                    minValue: f64::from(parameter.min),
                    maxValue: f64::from(parameter.max),
                    target: target,
                    action: sel!(controlChanged:)
                ];
                let _: () = msg_send![slider, setContinuous: Bool::YES];
                width(&*slider, SLIDER_WIDTH);
                let value: *mut Object =
                    msg_send![class!(NSTextField), labelWithString: ns_string!("")];
                let font: *mut Object = msg_send![class!(NSFont),
                    monospacedDigitSystemFontOfSize: 0.0f64,
                    weight: 0.0f64
                ];
                let _: () = msg_send![value, setFont: font];
                let _: () = msg_send![value, setAlignment: NS_TEXT_ALIGNMENT_RIGHT];
                width(&*value, VALUE_WIDTH);
                (
                    Some(label(parameter.label)),
                    Retained::retain(slider),
                    Retained::retain(value),
                )
            }
            Control::Number { label: text } => {
                let field: *mut Object =
                    msg_send![class!(NSTextField), textFieldWithString: ns_string!("")];
                let _: () = msg_send![field, setDelegate: target];
                width(&*field, FIELD_WIDTH);
                let unit: *mut Object =
                    msg_send![class!(NSTextField), labelWithString: ns_string!("pixels")];
                (
                    Some(label(text)),
                    Retained::retain(field),
                    Retained::retain(unit),
                )
            }
            Control::Choice {
                label: text,
                options,
            } => {
                let popup: Allocated<Object> = msg_send![class!(NSPopUpButton), alloc];
                let popup: Retained<Object> = msg_send![popup,
                    initWithFrame: rect(0.0, 0.0, 0.0, 0.0),
                    pullsDown: Bool::NO
                ];
                for option in options {
                    let _: () = msg_send![&*popup, addItemWithTitle: ns_string(option)];
                }
                let _: () = msg_send![&*popup, setTarget: target];
                let _: () = msg_send![&*popup, setAction: sel!(controlChanged:)];
                (Some(label(text)), Some(popup), None)
            }
            Control::Toggle { label: text } => {
                let checkbox: *mut Object = msg_send![class!(NSButton),
                    checkboxWithTitle: ns_string(text),
                    target: target,
                    action: sel!(controlChanged:)
                ];
                (None, Retained::retain(checkbox), None)
            }
        };
        let view = view.expect("AppKit creates controls");
        let _: () = msg_send![&*view, setTag: index as isize];
        (label, view, value)
    }
}

// An autoreleased push button with a title.
fn text_button(title: &str, action: Sel, target: *mut Object) -> *mut Object {
    // SAFETY: NSButton returns an autoreleased button that sends its action to the target.
    unsafe {
        msg_send![class!(NSButton),
            buttonWithTitle: ns_string(title),
            target: target,
            action: action
        ]
    }
}
