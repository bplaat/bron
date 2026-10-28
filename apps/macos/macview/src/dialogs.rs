/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The sheets that ask for the numbers of resize and rotate edits.

use std::cell::{Cell, RefCell};
use std::ptr::null_mut;

use block2::RcBlock;
use macview_appkit::ns_string;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, define_class, msg_send};

use crate::headers::*;

/// The largest width or height a bitmap can be resized to.
pub(crate) const MAXIMUM_DIMENSION: u32 = 30_000;
// The NSGridRowAlignment that lines labels up with the text of their fields.
const GRID_ROW_ALIGNMENT_FIRST_BASELINE: i64 = 2;
// The NSGridCellPlacements that align a column to its leading or trailing edge.
const GRID_CELL_PLACEMENT_LEADING: i64 = 2;
const GRID_CELL_PLACEMENT_TRAILING: i64 = 3;
// The width of a number field, in points.
const FIELD_WIDTH: f64 = 80.0;

struct LinkedFieldsIvars {
    width: Retained<Object>,
    height: Retained<Object>,
    keep_proportions: Retained<Object>,
    // Width divided by height of the image, which linked fields keep.
    ratio: f64,
    updating: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "MacViewLinkedFields"]
    #[ivars = LinkedFieldsIvars]
    struct LinkedFields;

    impl LinkedFields {
        #[unsafe(method(controlTextDidChange:))]
        fn _control_text_did_change(&self, notification: *mut Object) {
            self.text_did_change(notification);
        }
    }
);

impl LinkedFields {
    // Updates the other field to keep the proportions, when they are kept.
    fn text_did_change(&self, notification: *mut Object) {
        let ivars = self.ivars();
        if ivars.updating.get() {
            return;
        }
        // SAFETY: AppKit passes a notification of one of the two live fields.
        unsafe {
            let state: isize = msg_send![&*ivars.keep_proportions, state];
            if state != NS_CONTROL_STATE_VALUE_ON {
                return;
            }
            let field: *mut Object = msg_send![notification, object];
            let value: f64 = msg_send![field, doubleValue];
            let (other, linked) = if std::ptr::eq(field, Retained::as_ptr(&ivars.width)) {
                (&ivars.height, value / ivars.ratio)
            } else {
                (&ivars.width, value * ivars.ratio)
            };
            ivars.updating.set(true);
            let linked = linked.round().clamp(1.0, f64::from(MAXIMUM_DIMENSION)) as isize;
            let _: () = msg_send![&**other, setIntegerValue: linked];
            ivars.updating.set(false);
        }
    }
}

// Returns an autoreleased number field that only takes whole numbers in `minimum..=maximum`.
unsafe fn number_field(value: f64, minimum: f64, maximum: f64, decimals: bool) -> *mut Object {
    // SAFETY: The field and its formatter are autoreleased objects of AppKit.
    unsafe {
        let field: *mut Object = msg_send![class!(NSTextField), textFieldWithString: ns_string("")];
        let formatter: Retained<Object> = msg_send![class!(NSNumberFormatter), new];
        let _: () = msg_send![&*formatter, setAllowsFloats: objc_bool(decimals)];
        let _: () =
            msg_send![&*formatter, setMaximumFractionDigits: if decimals { 2usize } else { 0 }];
        let minimum: *mut Object = msg_send![class!(NSNumber), numberWithDouble: minimum];
        let maximum: *mut Object = msg_send![class!(NSNumber), numberWithDouble: maximum];
        let _: () = msg_send![&*formatter, setMinimum: minimum];
        let _: () = msg_send![&*formatter, setMaximum: maximum];
        let _: () = msg_send![field, setFormatter: formatter.as_ptr()];
        let _: () = msg_send![field, setDoubleValue: value];
        let _: () = msg_send![field, setAlignment: NS_TEXT_ALIGNMENT_RIGHT];
        let width: *mut Object = msg_send![field, widthAnchor];
        let width: *mut Object = msg_send![width, constraintEqualToConstant: FIELD_WIDTH];
        let _: () = msg_send![width, setActive: Bool::YES];
        field
    }
}

/// Returns an autoreleased grid of rows of views, with labels in the first column.
///
/// # Safety
///
/// Every view must be live.
pub(crate) unsafe fn grid(rows: &[&[*mut Object]]) -> *mut Object {
    // SAFETY: The views are live, and the grid retains them.
    unsafe {
        let rows: Vec<*mut Object> = rows
            .iter()
            .map(|row| -> *mut Object { ns_array(row) })
            .collect();
        let rows: *mut Object = ns_array(&rows);
        let grid: *mut Object = msg_send![class!(NSGridView), gridViewWithViews: rows];
        let _: () = msg_send![grid, setRowSpacing: 8.0f64];
        let _: () = msg_send![grid, setColumnSpacing: 8.0f64];
        let _: () = msg_send![grid, setRowAlignment: GRID_ROW_ALIGNMENT_FIRST_BASELINE];
        let first: *mut Object = msg_send![grid, columnAtIndex: 0isize];
        let _: () = msg_send![first, setXPlacement: GRID_CELL_PLACEMENT_TRAILING];
        let columns: isize = msg_send![grid, numberOfColumns];
        if columns > 2 {
            let units: *mut Object = msg_send![grid, columnAtIndex: 2isize];
            let _: () = msg_send![units, setXPlacement: GRID_CELL_PLACEMENT_LEADING];
        }
        let size: macview_appkit::Size = msg_send![grid, fittingSize];
        let _: () = msg_send![grid, setFrameSize: size];
        grid
    }
}

/// Returns an autoreleased label.
///
/// # Safety
///
/// This must run on the main thread.
pub(crate) unsafe fn label(text: &str) -> *mut Object {
    // SAFETY: NSTextField returns an autoreleased label.
    unsafe { msg_send![class!(NSTextField), labelWithString: ns_string(text)] }
}

// Shows an alert sheet on `window` with `accessory` below the message, and calls `confirmed` when
// the first button is clicked.
unsafe fn run_sheet(
    window: *mut Object,
    message: &str,
    information: &str,
    button: &str,
    accessory: *mut Object,
    first_responder: *mut Object,
    confirmed: impl FnOnce() + 'static,
) {
    // SAFETY: The caller supplies a live window. The alert is retained by the completion block
    // until the sheet ends.
    unsafe {
        let alert: Retained<Object> = msg_send![class!(NSAlert), new];
        let _: () = msg_send![&*alert, setMessageText: ns_string(message)];
        let _: () = msg_send![&*alert, setInformativeText: ns_string(information)];
        let _: *mut Object = msg_send![&*alert, addButtonWithTitle: ns_string(button)];
        let _: *mut Object = msg_send![&*alert, addButtonWithTitle: ns_string("Cancel")];
        let _: () = msg_send![&*alert, setAccessoryView: accessory];
        let _: () = msg_send![&*alert, layout];
        let sheet: *mut Object = msg_send![&*alert, window];
        let _: () = msg_send![sheet, setInitialFirstResponder: first_responder];

        let confirmed = Cell::new(Some(confirmed));
        let keep_alive = RefCell::new(Some(alert.clone()));
        let completion = RcBlock::new(move |response: isize| {
            keep_alive.borrow_mut().take();
            if response == NS_ALERT_FIRST_BUTTON_RETURN
                && let Some(confirmed) = confirmed.take()
            {
                confirmed();
            }
        });
        let _: () = msg_send![&*alert,
            beginSheetModalForWindow: window,
            completionHandler: &*completion
        ];
    }
}

/// Asks for a new image size, linked to the current proportions by default.
///
/// # Safety
///
/// `window` must point to a live `NSWindow`.
pub(crate) unsafe fn ask_image_size(
    window: *mut Object,
    title: &str,
    information: &str,
    current: (u32, u32),
    keep_proportions: bool,
    resize: impl Fn(u32, u32) + 'static,
) {
    // SAFETY: Every control is an autoreleased AppKit object retained by the alert, and the
    // linked fields delegate by the completion block until the sheet ends.
    unsafe {
        let maximum = f64::from(MAXIMUM_DIMENSION);
        let width = number_field(f64::from(current.0), 1.0, maximum, false);
        let height = number_field(f64::from(current.1), 1.0, maximum, false);
        let checkbox: *mut Object = msg_send![class!(NSButton),
            checkboxWithTitle: ns_string("Keep proportions"),
            target: null_mut::<Object>(),
            action: objc2::runtime::Sel(std::ptr::null())
        ];
        let state = if keep_proportions {
            NS_CONTROL_STATE_VALUE_ON
        } else {
            NS_CONTROL_STATE_VALUE_OFF
        };
        let _: () = msg_send![checkbox, setState: state];
        let empty: *mut Object = msg_send![class!(NSGridCell), emptyContentView];
        let accessory = grid(&[
            &[label("Width:"), width, label("pixels")],
            &[label("Height:"), height, label("pixels")],
            &[empty, checkbox, empty],
        ]);

        let linked: Allocated<LinkedFields> = msg_send![LinkedFields::class(), alloc];
        let linked: Retained<LinkedFields> = msg_send![
            super(linked.set_ivars(LinkedFieldsIvars {
                width: Retained::retain(width).expect("field is not null"),
                height: Retained::retain(height).expect("field is not null"),
                keep_proportions: Retained::retain(checkbox).expect("checkbox is not null"),
                ratio: f64::from(current.0) / f64::from(current.1.max(1)),
                updating: Cell::new(false),
            })),
            init
        ];
        for field in [width, height] {
            let _: () = msg_send![field, setDelegate: Retained::as_ptr(&linked)];
        }
        run_sheet(
            window,
            title,
            information,
            "Resize",
            accessory,
            width,
            move || {
                let ivars = linked.ivars();
                let width: isize = msg_send![&*ivars.width, integerValue];
                let height: isize = msg_send![&*ivars.height, integerValue];
                let clamp = |value: isize| value.clamp(1, MAXIMUM_DIMENSION as isize) as u32;
                let size = (clamp(width), clamp(height));
                if size != current {
                    resize(size.0, size.1);
                }
            },
        );
    }
}

/// Asks for an angle in degrees to rotate the image by.
///
/// # Safety
///
/// `window` must point to a live `NSWindow`.
pub(crate) unsafe fn ask_rotation(window: *mut Object, rotate: impl Fn(f64) + 'static) {
    // SAFETY: Every control is an autoreleased AppKit object retained by the alert.
    unsafe {
        let angle = number_field(0.0, -360.0, 360.0, true);
        let accessory = grid(&[&[label("Angle:"), angle, label("degrees clockwise")]]);
        let field = Retained::retain(angle).expect("field is not null");
        run_sheet(
            window,
            "Rotate by Angle",
            "The image grows to fit the rotated corners, which become transparent.",
            "Rotate",
            accessory,
            angle,
            move || {
                let degrees: f64 = msg_send![&*field, doubleValue];
                if degrees.rem_euclid(360.0) != 0.0 {
                    rotate(degrees);
                }
            },
        );
    }
}
