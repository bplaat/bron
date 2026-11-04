/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The Save As sheet of a window: a save panel with a format choice and the JPEG quality
//! below it, like Preview, and the size vectors are drawn at as bitmaps.

use std::ffi::{CStr, OsStr, c_char};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::null_mut;

use block2::RcBlock;
use imageview_core::appkit::ns_string;
use imageview_core::edit::{EXPORT_SCALES, is_jpeg};
use objc2::rc::{Allocated, Retained, autoreleasepool};
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, define_class, msg_send, sel};

use super::browse::file_url;
use super::headers::*;

const NS_MODAL_RESPONSE_OK: isize = 1;
const NS_GRID_ROW_ALIGNMENT_FIRST_BASELINE: isize = 2;
const NS_GRID_CELL_PLACEMENT_TRAILING: isize = 3;
const QUALITY_WIDTH: f64 = 160.0;

/// What a save sheet asks for.
pub(crate) struct SaveRequest<'a> {
    /// The folder it starts in.
    pub(crate) directory: Option<&'a Path>,
    /// The file name it starts with.
    pub(crate) name: &'a str,
    /// The JPEG quality it starts with.
    pub(crate) quality: u8,
    /// The formats to choose from, as a name and file name extension.
    pub(crate) formats: &'a [(&'static str, &'static str)],
    /// The natural size of a vector, which is drawn at a chosen scale.
    pub(crate) size: Option<(f64, f64)>,
}

/// What a save sheet chose: the path, the JPEG quality and the scale of a vector.
pub(crate) struct SaveChoice {
    pub(crate) path: PathBuf,
    pub(crate) quality: u8,
    pub(crate) scale: f64,
}

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
            let name = string(name);
            let stem = Path::new(&name)
                .file_stem()
                .map_or_else(|| name.clone(), |stem| stem.to_string_lossy().into_owned());
            let _: () = msg_send![ivars.panel, setNameFieldStringValue: ns_string(&format!("{stem}.{extension}"))];
            let kind: *mut Object =
                msg_send![class!(UTType), typeWithFilenameExtension: ns_string(extension)];
            if !kind.is_null() {
                let kinds: *mut Object = msg_send![class!(NSArray), arrayWithObject: kind];
                let _: () = msg_send![ivars.panel, setAllowedContentTypes: kinds];
            }
            let jpeg = is_jpeg(Path::new(&format!("a.{extension}")));
            let _: () = msg_send![ivars.quality_row, setHidden: objc_bool(!jpeg)];
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

/// Asks where to save an image in a sheet of a window, in which format and with which JPEG
/// quality, and at which size for a vector.
pub(super) fn save_panel(window: *mut Object, request: &SaveRequest<'_>) -> Option<SaveChoice> {
    let SaveRequest {
        directory,
        name,
        quality,
        formats,
        size,
    } = *request;
    autoreleasepool(|_| {
        // SAFETY: All objects are valid AppKit instances. The panel retains its accessory view,
        // which retains the controls that send to the target, which lives until the panel
        // is closed.
        unsafe {
            let panel: *mut Object = msg_send![class!(NSSavePanel), savePanel];
            if let Some(directory) = directory {
                let url = file_url(directory);
                if !url.is_null() {
                    let _: () = msg_send![panel, setDirectoryURL: url];
                }
            }
            let _: () = msg_send![panel, setNameFieldStringValue: ns_string(name)];
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
            let extension = Path::new(name)
                .extension()
                .map(|extension| extension.to_string_lossy().to_ascii_lowercase());
            let selected = formats
                .iter()
                .position(|(_, candidate)| {
                    extension.as_deref() == Some(candidate)
                        || (is_jpeg(Path::new(name))
                            && is_jpeg(Path::new(&format!("a.{candidate}"))))
                })
                .unwrap_or(0);
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
            let constraint: *mut Object =
                msg_send![anchor, constraintEqualToConstant: QUALITY_WIDTH];
            let _: () = msg_send![constraint, setActive: Bool::YES];
            let value: *mut Object = msg_send![class!(NSTextField), labelWithString: ns_string("")];
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
            let _: () =
                msg_send![rows, addObject: row(&[label("Quality:"), quality_slider, value])];
            // A vector is drawn at a scale of its natural size, shown with its pixel size.
            let scales = size.map(|(width, height)| {
                let scales: Allocated<Object> = msg_send![class!(NSPopUpButton), alloc];
                let scales: Retained<Object> = msg_send![scales,
                    initWithFrame: super::view::rect(0.0, 0.0, 0.0, 0.0),
                    pullsDown: Bool::NO
                ];
                for scale in EXPORT_SCALES {
                    let title = format!(
                        "{}% ({} \u{d7} {})",
                        (scale * 100.0).round(),
                        (width * scale).round().max(1.0),
                        (height * scale).round().max(1.0)
                    );
                    let _: () = msg_send![&*scales, addItemWithTitle: ns_string(&title)];
                }
                let natural = EXPORT_SCALES
                    .iter()
                    .position(|scale| *scale == 1.0)
                    .unwrap_or(0);
                let _: () = msg_send![&*scales, selectItemAtIndex: natural as isize];
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

            let response: isize = if window.is_null() {
                msg_send![panel, runModal]
            } else {
                let application: *mut Object = msg_send![class!(NSApplication), sharedApplication];
                let block = RcBlock::new(move |response: isize| {
                    let _: () = msg_send![application, stopModalWithCode: response];
                });
                let _: () =
                    msg_send![panel, beginSheetModalForWindow: window, completionHandler: &*block];
                msg_send![application, runModalForWindow: panel]
            };
            if response != NS_MODAL_RESPONSE_OK {
                return None;
            }
            let url: *mut Object = msg_send![panel, URL];
            let path: *const c_char = msg_send![url, fileSystemRepresentation];
            if path.is_null() {
                return None;
            }
            let path = PathBuf::from(OsStr::from_bytes(CStr::from_ptr(path).to_bytes()));
            let scale = scales.map_or(1.0, |scales| {
                let index: isize = msg_send![&*scales, indexOfSelectedItem];
                usize::try_from(index)
                    .ok()
                    .and_then(|index| EXPORT_SCALES.get(index))
                    .copied()
                    .unwrap_or(1.0)
            });
            Some(SaveChoice {
                path,
                quality: target.quality(),
                scale,
            })
        }
    })
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
