/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The controls of the Save As sheet below its save panel, like Preview.

use std::ffi::{CStr, c_char};
use std::ptr::null_mut;

use bwindow::NativeFileDialog;
use imageview_core::appkit::ns_string;
use objc2::rc::{Allocated, Retained, autoreleasepool};
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, define_class, msg_send, sel};

use super::headers::*;
use crate::save::{
    SaveRequest, format_of, has_quality, natural_scale, scale_at, scale_titles, with_extension,
};

const NS_GRID_ROW_ALIGNMENT_FIRST_BASELINE: isize = 2;
const NS_GRID_CELL_PLACEMENT_TRAILING: isize = 3;
const QUALITY_WIDTH: f64 = 160.0;

struct TargetIvars {
    panel: *mut Object,
    formats: Vec<(&'static str, &'static str)>,
    format: Retained<Object>,
    quality: Retained<Object>,
    value: Retained<Object>,
    quality_row: *mut Object,
}

define_class!(
    // Follows the format and quality controls of a save panel.
    #[unsafe(super(NSObject))]
    #[name = "ImageViewSaveTarget"]
    #[ivars = TargetIvars]
    struct Target;

    impl Target {
        #[unsafe(method(formatChanged:))]
        fn _format_changed(&self, _: *mut Object) {
            self.format_changed();
        }

        #[unsafe(method(qualityChanged:))]
        fn _quality_changed(&self, _: *mut Object) {
            self.show_quality();
        }
    }
);

impl Target {
    fn format_index(&self) -> usize {
        // SAFETY: The format control is a live pop-up button.
        let index: isize = unsafe { msg_send![&*self.ivars().format, indexOfSelectedItem] };
        usize::try_from(index)
            .unwrap_or(0)
            .min(self.ivars().formats.len() - 1)
    }

    fn quality(&self) -> u8 {
        // SAFETY: The quality control is a live slider.
        let quality: f64 = unsafe { msg_send![&*self.ivars().quality, doubleValue] };
        quality.round().clamp(1.0, 100.0) as u8
    }

    // Changes the extension of the file name to the chosen format, which the panel then
    // only allows, and shows the quality of JPEG.
    fn format_changed(&self) {
        let ivars = self.ivars();
        let extension = ivars.formats[self.format_index()].1;
        // SAFETY: The panel and quality row are live while the panel is shown, the strings
        // and arrays are autoreleased.
        unsafe {
            let name: *mut Object = msg_send![ivars.panel, nameFieldStringValue];
            let name = with_extension(&string(name), extension);
            let _: () = msg_send![ivars.panel, setNameFieldStringValue: ns_string(&name)];
            let kind: *mut Object =
                msg_send![class!(UTType), typeWithFilenameExtension: ns_string(extension)];
            if !kind.is_null() {
                let kinds: *mut Object = msg_send![class!(NSArray), arrayWithObject: kind];
                let _: () = msg_send![ivars.panel, setAllowedContentTypes: kinds];
            }
            let _: () = msg_send![ivars.quality_row, setHidden: objc_bool(!has_quality(extension))];
        }
    }

    fn show_quality(&self) {
        // SAFETY: The value label is a live text field.
        unsafe {
            let _: () = msg_send![&*self.ivars().value,
                setStringValue: ns_string(&format!("{}%", self.quality()))
            ];
        }
    }
}

/// The controls of a save panel.
pub(crate) struct SaveOptions {
    target: Retained<Target>,
    scales: Option<Retained<Object>>,
}

impl SaveOptions {
    /// Adds the controls to the save panel of a dialog.
    pub(crate) fn new(native: NativeFileDialog, request: &SaveRequest<'_>) -> Option<Self> {
        let NativeFileDialog::AppKit(panel) = native else {
            return None;
        };
        autoreleasepool(|_| {
            // SAFETY: bwindow passes a live NSSavePanel.
            unsafe { add_controls(panel.cast(), request) }
        })
    }

    /// The chosen JPEG quality.
    pub(crate) fn quality(&self) -> u8 {
        self.target.quality()
    }

    /// The chosen scale of a vector.
    pub(crate) fn scale(&self) -> f64 {
        self.scales.as_ref().map_or(1.0, |scales| {
            // SAFETY: The control is a live pop-up button.
            let index: isize = unsafe { msg_send![&**scales, indexOfSelectedItem] };
            usize::try_from(index).map_or(1.0, scale_at)
        })
    }
}

// Adds the format, quality and size controls of a request below a save panel.
unsafe fn add_controls(panel: *mut Object, request: &SaveRequest<'_>) -> Option<SaveOptions> {
    let SaveRequest {
        name,
        quality,
        formats,
        size,
        ..
    } = *request;
    // SAFETY: All objects are valid AppKit instances. The panel retains its accessory view, which
    // retains the controls that send to the target, which lives until the panel is closed.
    unsafe {
        let _: () = msg_send![panel, setCanCreateDirectories: Bool::YES];
        let _: () = msg_send![panel, setExtensionHidden: Bool::NO];

        let format: Allocated<Object> = msg_send![class!(NSPopUpButton), alloc];
        let format: Retained<Object> = msg_send![format,
            initWithFrame: super::view::rect(0.0, 0.0, 0.0, 0.0),
            pullsDown: Bool::NO
        ];
        for (title, _) in formats {
            let _: () = msg_send![&*format, addItemWithTitle: ns_string(title)];
        }
        let selected = format_of(name, formats);
        let _: () = msg_send![&*format, selectItemAtIndex: selected as isize];

        let quality_slider: *mut Object = msg_send![class!(NSSlider),
            sliderWithValue: f64::from(quality),
            minValue: 1.0f64,
            maxValue: 100.0f64,
            target: null_mut::<Object>(),
            action: sel!(qualityChanged:)
        ];
        let _: () = msg_send![quality_slider, setContinuous: Bool::YES];
        let anchor: *mut Object = msg_send![quality_slider, widthAnchor];
        let constraint: *mut Object = msg_send![anchor, constraintEqualToConstant: QUALITY_WIDTH];
        let _: () = msg_send![constraint, setActive: Bool::YES];
        let value: *mut Object = msg_send![class!(NSTextField), labelWithString: ns_string!("")];
        let font: *mut Object = msg_send![class!(NSFont),
            monospacedDigitSystemFontOfSize: 0.0f64,
            weight: 0.0f64
        ];
        let _: () = msg_send![value, setFont: font];

        let label = |text: &str| -> *mut Object {
            msg_send![class!(NSTextField), labelWithString: ns_string(text)]
        };
        let row = |views: &[*mut Object]| -> *mut Object {
            let row: *mut Object = msg_send![class!(NSMutableArray), array];
            for view in views {
                let _: () = msg_send![row, addObject: *view];
            }
            row
        };
        let rows: *mut Object = msg_send![class!(NSMutableArray), array];
        let _: () = msg_send![rows, addObject: row(&[
            label("Format:"),
            Retained::as_ptr(&format),
            msg_send![class!(NSGridCell), emptyContentView]
        ])];
        let _: () = msg_send![rows, addObject: row(&[label("Quality:"), quality_slider, value])];
        // A vector is drawn at a scale of its natural size, shown with its pixel size.
        let scales = size.map(|size| {
            let scales: Allocated<Object> = msg_send![class!(NSPopUpButton), alloc];
            let scales: Retained<Object> = msg_send![scales,
                initWithFrame: super::view::rect(0.0, 0.0, 0.0, 0.0),
                pullsDown: Bool::NO
            ];
            for title in scale_titles(size) {
                let _: () = msg_send![&*scales, addItemWithTitle: ns_string(&title)];
            }
            let _: () = msg_send![&*scales, selectItemAtIndex: natural_scale() as isize];
            scales
        });
        if let Some(scales) = &scales {
            let _: () = msg_send![rows, addObject: row(&[
                label("Size:"),
                Retained::as_ptr(scales),
                msg_send![class!(NSGridCell), emptyContentView]
            ])];
        }
        let grid: *mut Object = msg_send![class!(NSGridView), gridViewWithViews: rows];
        let _: () = msg_send![grid, setRowAlignment: NS_GRID_ROW_ALIGNMENT_FIRST_BASELINE];
        let _: () = msg_send![grid, setColumnSpacing: 8.0f64];
        let _: () = msg_send![grid, setRowSpacing: 8.0f64];
        let column: *mut Object = msg_send![grid, columnAtIndex: 0isize];
        let _: () = msg_send![column, setXPlacement: NS_GRID_CELL_PLACEMENT_TRAILING];
        let quality_row: *mut Object = msg_send![grid, rowAtIndex: 1isize];

        // Leave room around the controls, like other accessory views.
        let accessory: Retained<Object> = msg_send![class!(NSView), new];
        let _: () = msg_send![grid, setTranslatesAutoresizingMaskIntoConstraints: Bool::NO];
        let _: () = msg_send![&*accessory, addSubview: grid];
        for (grid_anchor, accessory_anchor, constant) in [
            (sel!(topAnchor), sel!(topAnchor), 12.0f64),
            (sel!(bottomAnchor), sel!(bottomAnchor), -12.0),
            (sel!(centerXAnchor), sel!(centerXAnchor), 0.0),
        ] {
            let from: *mut Object = msg_send![grid, performSelector: grid_anchor];
            let to: *mut Object = msg_send![&*accessory, performSelector: accessory_anchor];
            let constraint: *mut Object =
                msg_send![from, constraintEqualToAnchor: to, constant: constant];
            let _: () = msg_send![constraint, setActive: Bool::YES];
        }
        let leading: *mut Object = msg_send![grid, leadingAnchor];
        let accessory_leading: *mut Object = msg_send![&*accessory, leadingAnchor];
        let constraint: *mut Object = msg_send![leading, constraintGreaterThanOrEqualToAnchor: accessory_leading, constant: 20.0f64];
        let _: () = msg_send![constraint, setActive: Bool::YES];
        let _: () = msg_send![panel, setAccessoryView: &*accessory];

        let target: Allocated<Target> = msg_send![Target::class(), alloc];
        let target: Retained<Target> = msg_send![
            super(target.set_ivars(TargetIvars {
                panel,
                formats: formats.to_vec(),
                format: format.clone(),
                quality: Retained::retain(quality_slider)?,
                value: Retained::retain(value)?,
                quality_row,
            })),
            init
        ];
        let _: () = msg_send![&*format, setTarget: &*target];
        let _: () = msg_send![&*format, setAction: sel!(formatChanged:)];
        let _: () = msg_send![quality_slider, setTarget: &*target];
        target.format_changed();
        target.show_quality();

        Some(SaveOptions { target, scales })
    }
}

// The contents of an NSString.
unsafe fn string(string: *mut Object) -> String {
    // SAFETY: The caller passes a valid NSString, its UTF-8 representation lives in this pool.
    unsafe {
        let bytes: *const c_char = msg_send![string, UTF8String];
        if bytes.is_null() {
            return String::new();
        }
        CStr::from_ptr(bytes).to_string_lossy().into_owned()
    }
}

#[cfg(test)]
mod tests {
    use objc2::runtime::AnyClass;

    #[test]
    fn appkit_loads_file_types() {
        // AppKit loads UniformTypeIdentifiers, so its file types need no framework link.
        assert!(AnyClass::get(c"UTType").is_some());
    }
}
