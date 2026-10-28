/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Editing the pixels of a document: the edited bitmap, undo, live previews, and saving.

use std::borrow::Cow;
use std::ffi::{CStr, c_char, c_void};
use std::ptr::null_mut;
use std::sync::Arc;

use block2::RcBlock;
use image::{Bitmap, EncodeOptions, Format};
use macview_appkit::{Image, Point, Rect, Size, dispatch_async, dispatch_async_main, ns_string};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject as Object, Bool, Sel};
use objc2::{class, msg_send, sel};

use crate::dialogs::{grid, label};
use crate::edit::{self, Adjustments, Canvas, Selection};
use crate::headers::*;
use crate::window_controller::{set_busy, update_media_image};
use crate::{Document, MainQueueObject, content_version, set_error};

// The longest side of the bitmap that live previews are rendered at.
const PREVIEW_SIZE: u32 = 2048;
// The number of edits that can be undone, each of which keeps a copy of the bitmap.
const UNDO_LEVELS: usize = 20;
// The defaults key and fallback of the quality JPEG images are saved at.
const JPEG_QUALITY_KEY: &str = "JPEGQuality";
const DEFAULT_JPEG_QUALITY: u8 = 90;

/// The types MacView saves, with their file extension, encoder and name, in save panel order.
const WRITABLE_TYPES: [(&str, &str, Format, &str); 5] = [
    ("public.png", "png", Format::Png, "PNG"),
    ("public.jpeg", "jpg", Format::Jpeg, "JPEG"),
    ("com.microsoft.bmp", "bmp", Format::Bmp, "BMP"),
    ("com.compuserve.gif", "gif", Format::Gif, "GIF"),
    ("org.qoiformat.qoi", "qoi", Format::Qoi, "QOI"),
];

/// An edit that is previewed live before it is applied to the full bitmap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Operation {
    /// Light, color and effect adjustments, limited to the selection when there is one.
    Adjust {
        adjustments: Adjustments,
        selection: Option<Selection>,
    },
    /// Rotates by a small angle and crops the tilted edges away.
    Straighten { degrees: f64 },
}

impl Operation {
    // Returns whether the operation leaves the bitmap as it is.
    fn is_identity(&self) -> bool {
        match self {
            Self::Adjust { adjustments, .. } => adjustments.is_identity(),
            Self::Straighten { degrees } => degrees.abs() < 0.01,
        }
    }

    // Runs the operation on a bitmap that is `scale` times the size of the full image.
    fn run(&self, canvas: &Canvas, scale: f64) -> Canvas {
        match self {
            Self::Adjust {
                adjustments,
                selection,
            } => {
                let selection = selection.map(|selection| selection.scaled(scale));
                edit::apply(canvas, adjustments, selection.as_ref(), scale)
            }
            Self::Straighten { degrees } => edit::rotate(canvas, *degrees, true),
        }
    }
}

// A live preview of an operation on a smaller copy of the bitmap.
struct Preview {
    // Tells renders of an earlier preview apart, which are dropped when they finish.
    session: u64,
    proxy: Arc<Canvas>,
    scale: f64,
    // The last requested operation, and whether it still has to be rendered.
    operation: Option<Operation>,
    pending: bool,
    rendering: bool,
    image: Option<Image>,
}

/// The editing state of a document.
#[derive(Default)]
pub(crate) struct EditState {
    // The edited bitmap, which is made from the media on the first edit.
    canvas: Option<Arc<Canvas>>,
    // The bitmap as it was opened, to compare with and reset to.
    original: Option<Arc<Canvas>>,
    original_image: Option<Image>,
    preview: Option<Preview>,
    preview_sessions: u64,
    comparing: bool,
    busy: bool,
    // The save panel that runs with the format menu and quality slider of MacView.
    save_panel: Option<SavePanel>,
}

// A running save panel and the controls MacView adds to it.
struct SavePanel {
    panel: Retained<Object>,
    quality: Retained<Object>,
    slider: Retained<Object>,
    // The index of the chosen type in the writable types.
    format: usize,
}

impl Document {
    fn edit_state(&self) -> std::cell::RefMut<'_, EditState> {
        self.ivars().edit.borrow_mut()
    }

    const fn as_object(&self) -> *mut Object {
        self as *const Self as *mut Object
    }

    pub(crate) fn retained(&self) -> MainQueueObject {
        // SAFETY: self is a live document.
        MainQueueObject(
            unsafe { Retained::retain(self.as_object()) }.expect("cannot retain a null document"),
        )
    }

    /// Returns whether the pixels of the media can be edited.
    pub(crate) fn is_editable(&self) -> bool {
        self.ivars()
            .media
            .borrow()
            .as_ref()
            .is_some_and(Image::is_editable)
    }

    /// Returns the size of the edited bitmap in pixels, or `None` when it cannot be edited.
    pub(crate) fn canvas_size(&self) -> Option<Size> {
        if !self.is_editable() {
            return None;
        }
        if let Some(canvas) = &self.edit_state().canvas {
            return Some(Size {
                width: f64::from(canvas.width),
                height: f64::from(canvas.height),
            });
        }
        self.ivars()
            .media
            .borrow()
            .as_ref()
            .and_then(Image::pixel_size)
    }

    /// Returns whether an edit is being applied to the full bitmap.
    pub(crate) fn is_busy(&self) -> bool {
        self.edit_state().busy
    }

    /// Returns whether a live preview is shown.
    pub(crate) fn is_previewing(&self) -> bool {
        self.edit_state().preview.is_some()
    }

    /// Returns whether the original is shown instead of the edited bitmap.
    pub(crate) fn is_comparing(&self) -> bool {
        self.edit_state().comparing
    }

    /// Returns whether new edits can start now.
    pub(crate) fn can_edit(&self) -> bool {
        let state = self.edit_state();
        !state.busy && state.preview.is_none() && !state.comparing && self.is_editable()
    }

    /// Returns whether the bitmap differs from the one that was opened.
    pub(crate) fn has_edits(&self) -> bool {
        let state = self.edit_state();
        match (&state.canvas, &state.original) {
            (Some(canvas), Some(original)) => !Arc::ptr_eq(canvas, original),
            _ => false,
        }
    }

    /// Returns the edited bitmap, making it from the media on first use.
    pub(crate) fn canvas(&self) -> Option<Arc<Canvas>> {
        if let Some(canvas) = &self.edit_state().canvas {
            return Some(canvas.clone());
        }
        if !self.is_editable() {
            return None;
        }
        let (width, height, pixels) = self.ivars().media.borrow().as_ref()?.to_pixels()?;
        let canvas = Arc::new(Canvas::new(width, height, pixels));
        let mut state = self.edit_state();
        state.canvas = Some(canvas.clone());
        state.original = Some(canvas.clone());
        Some(canvas)
    }

    /// Starts editing a new document with `canvas` as its original.
    pub(crate) fn install_canvas(&self, canvas: Canvas) -> bool {
        let Some(image) = Image::from_pixels(canvas.width, canvas.height, &canvas.pixels) else {
            return false;
        };
        let canvas = Arc::new(canvas);
        self.ivars().media.replace(Some(image));
        let mut state = self.edit_state();
        state.canvas = Some(canvas.clone());
        state.original = Some(canvas);
        true
    }

    /// Forgets all edits, after the document shows another file or reverts to the saved one.
    pub(crate) fn reset_editing(&self) {
        {
            let mut state = self.edit_state();
            *state = EditState {
                preview_sessions: state.preview_sessions,
                ..EditState::default()
            };
        }
        // SAFETY: The document owns its undo manager.
        unsafe {
            let undo_manager: *mut Object = msg_send![self.as_object(), undoManager];
            let _: () = msg_send![undo_manager, removeAllActions];
            let _: () = msg_send![self.as_object(), updateChangeCount: NS_CHANGE_CLEARED];
        }
    }

    /// Returns the image the windows show: the original, a preview, or the edited media.
    pub(crate) fn with_displayed_image<R>(&self, show: impl FnOnce(&Image) -> R) -> Option<R> {
        let state = self.ivars().edit.borrow();
        if state.comparing
            && let Some(image) = &state.original_image
        {
            return Some(show(image));
        }
        if let Some(image) = state
            .preview
            .as_ref()
            .and_then(|preview| preview.image.as_ref())
        {
            return Some(show(image));
        }
        drop(state);
        self.ivars().media.borrow().as_ref().map(show)
    }

    // Shows the displayed image in every window, without moving or zooming the media.
    fn show_displayed_image(&self) {
        // SAFETY: The document owns its window controllers.
        unsafe {
            let controllers: *mut Object = msg_send![self.as_object(), windowControllers];
            let count: usize = msg_send![controllers, count];
            for index in 0..count {
                let controller: *mut Object = msg_send![controllers, objectAtIndex: index];
                self.with_displayed_image(|image| update_media_image(controller, image));
            }
        }
    }

    fn set_busy(&self, busy: bool) {
        self.edit_state().busy = busy;
        // SAFETY: The document owns its window controllers.
        unsafe {
            let controllers: *mut Object = msg_send![self.as_object(), windowControllers];
            let count: usize = msg_send![controllers, count];
            for index in 0..count {
                let controller: *mut Object = msg_send![controllers, objectAtIndex: index];
                set_busy(controller, busy);
            }
        }
    }

    /// Applies `edit` to the full bitmap on a worker, then installs the result as an undoable
    /// action named `action`.
    pub(crate) fn edit<F>(&self, action: &'static str, edit: F)
    where
        F: FnOnce(&Canvas) -> Canvas + Send + 'static,
    {
        if !self.can_edit() {
            return;
        }
        let Some(canvas) = self.canvas() else {
            return;
        };
        self.set_busy(true);
        let retained = self.retained();
        dispatch_async(move || {
            let result = Arc::new(edit(&canvas));
            dispatch_async_main(move || {
                // SAFETY: retained keeps the document alive until this continuation ends.
                let document = unsafe { &*retained.as_ptr_on_main().cast::<Document>() };
                document.set_busy(false);
                document.set_canvas(result, action);
            });
        });
    }

    /// Replaces the bitmap with `canvas` as an undoable action named `action`.
    pub(crate) fn set_canvas(&self, canvas: Arc<Canvas>, action: &'static str) {
        let Some(previous) = self.edit_state().canvas.replace(canvas.clone()) else {
            return;
        };
        let Some(image) = Image::from_pixels(canvas.width, canvas.height, &canvas.pixels) else {
            self.edit_state().canvas = Some(previous);
            return;
        };
        let resized = (previous.width, previous.height) != (canvas.width, canvas.height);
        // Undoing puts the previous bitmap back, which registers the redo the same way.
        let undo = RcBlock::new(move |target: *mut Object| {
            // SAFETY: The undo manager only calls this with its target, the live document.
            let document = unsafe { &*target.cast::<Document>() };
            document.set_canvas(previous.clone(), action);
        });
        // SAFETY: The document owns its undo manager, which copies the handler block.
        unsafe {
            let this = self.as_object();
            let undo_manager: *mut Object = msg_send![this, undoManager];
            let _: () = msg_send![undo_manager, setLevelsOfUndo: UNDO_LEVELS];
            // Edits finish on the main queue outside of any event, where groups by event would stay
            // open and merge edits, so every edit is its own group. Undoing and redoing register
            // inside the group the undo manager opens for them.
            let _: () = msg_send![undo_manager, setGroupsByEvent: Bool::NO];
            let level: isize = msg_send![undo_manager, groupingLevel];
            if level == 0 {
                let _: () = msg_send![undo_manager, beginUndoGrouping];
            }
            let _: () = msg_send![undo_manager, registerUndoWithTarget: this, handler: &*undo];
            let _: () = msg_send![undo_manager, setActionName: ns_string(action)];
            if level == 0 {
                let _: () = msg_send![undo_manager, endUndoGrouping];
            }
        }
        self.ivars().media.replace(Some(image));
        {
            let mut state = self.edit_state();
            state.comparing = false;
            state.original_image = None;
        }
        self.refresh_windows(resized);
    }

    /// Puts the bitmap back the way it was opened.
    pub(crate) fn reset_to_original(&self) {
        let original = self.edit_state().original.clone();
        if let Some(original) = original
            && self.has_edits()
            && self.can_edit()
        {
            self.set_canvas(original, "Reset to Original");
        }
    }

    /// Switches between showing the original and the edited bitmap.
    pub(crate) fn toggle_compare(&self) {
        {
            let mut state = self.edit_state();
            if state.comparing {
                state.comparing = false;
                state.original_image = None;
            } else {
                let Some(original) = state.original.clone() else {
                    return;
                };
                let Some(canvas) = state.canvas.clone() else {
                    return;
                };
                // Show the original at the size of the edited bitmap, so the view stays put.
                state.original_image = Image::from_pixels_with_size(
                    original.width,
                    original.height,
                    &original.pixels,
                    Size {
                        width: f64::from(canvas.width),
                        height: f64::from(canvas.height),
                    },
                );
                state.comparing = state.original_image.is_some();
            }
        }
        self.show_displayed_image();
    }

    /// Starts a live preview session.
    pub(crate) fn begin_preview(&self) -> bool {
        if !self.can_edit() {
            return false;
        }
        let Some(canvas) = self.canvas() else {
            return false;
        };
        let (proxy, scale) = canvas.proxy(PREVIEW_SIZE);
        let mut state = self.edit_state();
        state.preview_sessions += 1;
        state.preview = Some(Preview {
            session: state.preview_sessions,
            proxy: Arc::new(proxy),
            scale,
            operation: None,
            pending: false,
            rendering: false,
            image: None,
        });
        true
    }

    /// Previews `operation`, rendering only the newest one while earlier renders run.
    pub(crate) fn update_preview(&self, operation: Operation) {
        {
            let mut state = self.edit_state();
            let Some(preview) = state.preview.as_mut() else {
                return;
            };
            preview.operation = Some(operation);
            preview.pending = true;
        }
        self.render_preview();
    }

    fn render_preview(&self) {
        let job = {
            let mut state = self.edit_state();
            let Some(preview) = state.preview.as_mut() else {
                return;
            };
            if preview.rendering || !preview.pending {
                return;
            }
            let Some(operation) = preview.operation else {
                return;
            };
            preview.pending = false;
            preview.rendering = true;
            (
                preview.session,
                preview.proxy.clone(),
                preview.scale,
                operation,
            )
        };
        let (session, proxy, scale, operation) = job;
        let retained = self.retained();
        dispatch_async(move || {
            let result = (!operation.is_identity()).then(|| operation.run(&proxy, scale));
            dispatch_async_main(move || {
                // SAFETY: retained keeps the document alive until this continuation ends.
                let document = unsafe { &*retained.as_ptr_on_main().cast::<Document>() };
                document.finish_preview(session, result);
            });
        });
    }

    fn finish_preview(&self, session: u64, result: Option<Canvas>) {
        let Some(size) = self.canvas_size() else {
            return;
        };
        {
            let mut state = self.edit_state();
            let Some(preview) = state
                .preview
                .as_mut()
                .filter(|preview| preview.session == session)
            else {
                return;
            };
            preview.rendering = false;
            // An identity operation shows the bitmap itself.
            preview.image = result.and_then(|result| {
                Image::from_pixels_with_size(result.width, result.height, &result.pixels, size)
            });
        }
        self.show_displayed_image();
        self.render_preview();
    }

    /// Ends the preview, leaving the bitmap as it was.
    pub(crate) fn cancel_preview(&self) {
        if self.edit_state().preview.take().is_some() {
            self.show_displayed_image();
        }
    }

    /// Ends the preview by applying its operation to the full bitmap as `action`.
    pub(crate) fn commit_preview(&self, action: &'static str) {
        let preview = self.edit_state().preview.take();
        let Some(operation) = preview
            .and_then(|preview| preview.operation)
            .filter(|operation| !operation.is_identity())
        else {
            self.show_displayed_image();
            return;
        };
        self.edit(action, move |canvas| operation.run(canvas, 1.0));
    }

    /// Copies the selected part of the image, or all of it, to the pasteboard.
    pub(crate) fn copy_image(&self, selection: Option<Selection>) {
        let cropped = match (selection, self.canvas()) {
            (Some(selection), Some(canvas)) => {
                let cropped = edit::crop(&canvas, &selection);
                Image::from_pixels(cropped.width, cropped.height, &cropped.pixels)
            }
            _ => None,
        };
        let media = self.ivars().media.borrow();
        let Some(image) = cropped.as_ref().or(media.as_ref()) else {
            return;
        };
        // SAFETY: The general pasteboard is a shared object of AppKit that copies the image.
        unsafe {
            let pasteboard: *mut Object = msg_send![class!(NSPasteboard), generalPasteboard];
            let _: isize = msg_send![pasteboard, clearContents];
            let objects: *mut Object = msg_send![class!(NSArray), arrayWithObject: image.as_ptr()];
            let _: Bool = msg_send![pasteboard, writeObjects: objects];
        }
    }

    /// Returns the types the document can be saved as.
    pub(crate) fn writable_types(&self) -> *mut Object {
        let types = WRITABLE_TYPES.map(|(identifier, _, _, _)| identifier);
        ns_string_array(if self.is_editable() { &types } else { &[] })
    }

    /// Returns whether the document can be saved in the type it was opened as.
    pub(crate) fn is_writable_type(&self, type_name: *mut Object) -> bool {
        self.is_editable() && writable_format(type_name).is_some()
    }

    /// Returns the file extension of a type MacView saves, or null to let AppKit pick one.
    pub(crate) fn file_name_extension(type_name: *mut Object) -> *mut Object {
        let identifier = rust_string(type_name);
        WRITABLE_TYPES
            .iter()
            .find(|(writable, _, _, _)| *writable == identifier)
            .map_or(null_mut(), |(_, extension, _, _)| ns_string(extension))
    }

    /// Encodes the bitmap as `type_name`, or reports why it cannot.
    pub(crate) fn data_of_type(
        &self,
        type_name: *mut Object,
        error_out: *mut c_void,
    ) -> *mut Object {
        self.edit_state().save_panel = None;
        let Some(format) = writable_format(type_name) else {
            set_error(error_out, "MacView cannot save images in this format.");
            return null_mut();
        };
        let Some(canvas) = self.canvas() else {
            set_error(error_out, "This image cannot be saved.");
            return null_mut();
        };
        // JPEG has no transparency, so transparent parts become white like in other editors.
        let canvas = if format == Format::Jpeg && !canvas.is_opaque() {
            Cow::Owned(canvas.flattened([255, 255, 255]))
        } else {
            Cow::Borrowed(&*canvas)
        };
        let options = EncodeOptions {
            jpeg_quality: jpeg_quality(),
            ..EncodeOptions::default()
        };
        let encoded = Bitmap::new(canvas.width, canvas.height, canvas.pixels.clone())
            .and_then(|bitmap| image::encode_with_options(&bitmap, format, options));
        match encoded {
            Ok(bytes) => {
                // The file watcher sees the saved file as unchanged and leaves the edits alone.
                self.ivars()
                    .content_version
                    .set(Some(content_version(&bytes)));
                // SAFETY: NSData copies the encoded bytes.
                unsafe {
                    msg_send![class!(NSData),
                        dataWithBytes: bytes.as_ptr().cast::<c_void>(),
                        length: bytes.len()
                    ]
                }
            }
            Err(error) => {
                set_error(
                    error_out,
                    &format!("The image could not be saved: {error}."),
                );
                null_mut()
            }
        }
    }

    /// Shows a format menu and a JPEG quality slider below a save panel.
    pub(crate) fn add_save_accessory(&self, panel: *mut Object) {
        // SAFETY: AppKit passes a live save panel, which retains its accessory views. The menu
        // keeps a weak target, the document, which outlives the panel it runs.
        unsafe {
            let popup: Allocated<Object> = msg_send![class!(NSPopUpButton), alloc];
            let popup: Retained<Object> = msg_send![popup,
                initWithFrame: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size { width: 0.0, height: 0.0 },
                },
                pullsDown: Bool::NO
            ];
            for (_, _, _, name) in WRITABLE_TYPES {
                let _: () = msg_send![&*popup, addItemWithTitle: ns_string(name)];
            }
            let _: () = msg_send![&*popup, setTarget: self.as_object()];
            let _: () = msg_send![&*popup, setAction: sel!(saveFormatChanged:)];

            let slider: *mut Object = msg_send![class!(NSSlider),
                sliderWithValue: f64::from(jpeg_quality()),
                minValue: 1.0f64,
                maxValue: 100.0f64,
                target: null_mut::<Object>(),
                action: Sel(std::ptr::null())
            ];
            let value: *mut Object = msg_send![class!(NSTextField), labelWithString: ns_string("")];
            let font: *mut Object = msg_send![class!(NSFont),
                monospacedDigitSystemFontOfSize: 0.0f64,
                weight: 0.0f64
            ];
            let _: () = msg_send![value, setFont: font];
            // Bind both to the defaults, which then hold the quality without any glue code.
            let defaults: *mut Object = msg_send![
                class!(NSUserDefaultsController),
                sharedUserDefaultsController
            ];
            let key_path = ns_string(&format!("values.{JPEG_QUALITY_KEY}"));
            for view in [slider, value] {
                let _: () = msg_send![view,
                    bind: ns_string("value"),
                    toObject: defaults,
                    withKeyPath: key_path,
                    options: null_mut::<Object>()
                ];
            }
            let width: *mut Object = msg_send![slider, widthAnchor];
            let width: *mut Object = msg_send![width, constraintEqualToConstant: 200.0f64];
            let _: () = msg_send![width, setActive: Bool::YES];
            let quality = stack_view(
                &[slider, value],
                NS_USER_INTERFACE_LAYOUT_ORIENTATION_HORIZONTAL,
            );

            let grid = grid(&[
                &[label("Format:"), popup.as_ptr()],
                &[label("Quality:"), quality],
            ]);
            let accessory = stack_view(&[grid], NS_USER_INTERFACE_LAYOUT_ORIENTATION_VERTICAL);
            let _: () = msg_send![accessory, setEdgeInsets: EdgeInsets::uniform(12.0)];
            let size: Size = msg_send![accessory, fittingSize];
            let _: () = msg_send![accessory, setFrameSize: size];
            let _: () = msg_send![panel, setAccessoryView: accessory];

            let file_type: *mut Object = msg_send![self.as_object(), fileType];
            let format = writable_format(file_type)
                .and_then(|format| {
                    WRITABLE_TYPES
                        .iter()
                        .position(|(_, _, writable, _)| *writable == format)
                })
                .unwrap_or(0);
            let _: () = msg_send![&*popup, selectItemAtIndex: format as isize];
            self.edit_state().save_panel = Some(SavePanel {
                panel: Retained::retain(panel).expect("panel is not null"),
                quality: Retained::retain(quality).expect("quality row is not null"),
                slider: Retained::retain(slider).expect("slider is not null"),
                format,
            });
            self.select_save_format(format);
        }
    }

    /// Saves in the format chosen in the format menu of the save panel.
    pub(crate) fn save_format_changed(&self, popup: *mut Object) {
        // SAFETY: The sender is the format menu of the save panel.
        let index: isize = unsafe { msg_send![popup, indexOfSelectedItem] };
        if let Ok(index) = usize::try_from(index) {
            self.select_save_format(index);
        }
    }

    // Limits the save panel to the format at `index`, which only has a quality for JPEG.
    fn select_save_format(&self, index: usize) {
        let Some(&(identifier, extension, format, _)) = WRITABLE_TYPES.get(index) else {
            return;
        };
        let mut state = self.edit_state();
        let Some(save_panel) = state.save_panel.as_mut() else {
            return;
        };
        save_panel.format = index;
        // SAFETY: The panel and its accessory views are live while it runs. A type MacView declares
        // itself may be unknown to a build without a bundle, so fall back to its extension.
        unsafe {
            let mut kind: *mut Object =
                msg_send![class!(UTType), typeWithIdentifier: ns_string(identifier)];
            if kind.is_null() {
                kind = msg_send![class!(UTType), typeWithFilenameExtension: ns_string(extension)];
            }
            let _: () = msg_send![&*save_panel.panel, setAllowedContentTypes: ns_array(&[kind])];
            let jpeg = format == Format::Jpeg;
            let _: () = msg_send![&*save_panel.slider, setEnabled: objc_bool(jpeg)];
            let alpha = if jpeg { 1.0f64 } else { 0.4 };
            let _: () = msg_send![&*save_panel.quality, setAlphaValue: alpha];
        }
    }

    /// Returns the type chosen in the last save panel, or null when it had no format menu.
    pub(crate) fn save_panel_type(&self) -> *mut Object {
        self.edit_state()
            .save_panel
            .as_ref()
            .map_or(null_mut(), |save_panel| {
                ns_string(WRITABLE_TYPES[save_panel.format].0)
            })
    }
}

// Returns an autoreleased stack view of `views` with standard spacing.
unsafe fn stack_view(views: &[*mut Object], orientation: i64) -> *mut Object {
    // SAFETY: The views are live, and the stack view retains them.
    unsafe {
        let array: *mut Object = ns_array(views);
        let stack: *mut Object = msg_send![class!(NSStackView), stackViewWithViews: array];
        let _: () = msg_send![stack, setOrientation: orientation];
        let _: () = msg_send![stack, setSpacing: 8.0f64];
        stack
    }
}

// Returns the encoder of a type MacView saves.
fn writable_format(type_name: *mut Object) -> Option<Format> {
    let identifier = rust_string(type_name);
    if identifier == "org.jpeg.jfif" {
        return Some(Format::Jpeg);
    }
    WRITABLE_TYPES
        .iter()
        .find(|(writable, _, _, _)| *writable == identifier)
        .map(|(_, _, format, _)| *format)
}

// Returns the JPEG quality from the defaults.
fn jpeg_quality() -> u8 {
    // SAFETY: The standard user defaults are a shared object of Foundation.
    let quality: isize = unsafe {
        let defaults: *mut Object = msg_send![class!(NSUserDefaults), standardUserDefaults];
        msg_send![defaults, integerForKey: ns_string(JPEG_QUALITY_KEY)]
    };
    u8::try_from(quality)
        .ok()
        .filter(|quality| (1..=100).contains(quality))
        .unwrap_or(DEFAULT_JPEG_QUALITY)
}

/// Copies an `NSString` into a Rust string, returning an empty string for null.
pub(crate) fn rust_string(string: *mut Object) -> String {
    if string.is_null() {
        return String::new();
    }
    // SAFETY: NSString keeps its UTF-8 representation alive for the current autorelease pool.
    unsafe {
        let bytes: *const c_char = msg_send![string, UTF8String];
        if bytes.is_null() {
            return String::new();
        }
        CStr::from_ptr(bytes).to_string_lossy().into_owned()
    }
}

/// Returns whether the pasteboard holds an image.
pub(crate) fn can_paste_image() -> bool {
    // SAFETY: The general pasteboard is a shared object of AppKit.
    unsafe {
        let pasteboard: *mut Object = msg_send![class!(NSPasteboard), generalPasteboard];
        let can: Bool = msg_send![class!(NSImage), canInitWithPasteboard: pasteboard];
        can.as_bool()
    }
}

/// Opens the image on the pasteboard as a new, unsaved document.
pub(crate) fn open_pasted_image() {
    // SAFETY: The pasteboard and document controller are shared objects of AppKit. The new
    // document is retained by the controller once added.
    unsafe {
        let pasteboard: *mut Object = msg_send![class!(NSPasteboard), generalPasteboard];
        let image: Allocated<Object> = msg_send![class!(NSImage), alloc];
        let image: Option<Retained<Object>> = msg_send![image, initWithPasteboard: pasteboard];
        let Some(image) = image else {
            return;
        };
        let Some((width, height, pixels)) = Image::from_native(image).to_pixels() else {
            return;
        };
        let controller: *mut Object =
            msg_send![class!(NSDocumentController), sharedDocumentController];
        let document: *mut Object = msg_send![controller,
            makeUntitledDocumentOfType: ns_string!("public.png"),
            error: null_mut::<c_void>()
        ];
        if document.is_null() {
            return;
        }
        let ours: Bool = msg_send![document, isKindOfClass: Document::class()];
        if !ours.as_bool()
            || !(*document.cast::<Document>()).install_canvas(Canvas::new(width, height, pixels))
        {
            return;
        }
        let _: () = msg_send![controller, addDocument: document];
        let _: () = msg_send![document, makeWindowControllers];
        let _: () = msg_send![document, showWindows];
        // A pasted image only exists in memory, so closing it asks to save it.
        let _: () = msg_send![document, updateChangeCount: NS_CHANGE_DONE];
    }
}

#[cfg(test)]
mod tests {
    use objc2::rc::autoreleasepool;

    use super::*;

    #[test]
    fn writable_types_map_to_their_encoders() {
        autoreleasepool(|_| {
            assert_eq!(writable_format(ns_string("public.png")), Some(Format::Png));
            assert_eq!(
                writable_format(ns_string("org.jpeg.jfif")),
                Some(Format::Jpeg)
            );
            assert_eq!(
                writable_format(ns_string("org.qoiformat.qoi")),
                Some(Format::Qoi)
            );
            assert_eq!(writable_format(ns_string("public.svg-image")), None);
            assert_eq!(writable_format(null_mut()), None);
        });
    }

    #[test]
    fn writable_types_have_their_own_extensions() {
        autoreleasepool(|_| {
            let extension = Document::file_name_extension(ns_string("public.jpeg"));
            assert_eq!(rust_string(extension), "jpg");
            assert!(Document::file_name_extension(ns_string("public.tiff")).is_null());
        });
    }

    #[test]
    fn identity_operations_leave_the_bitmap_alone() {
        let adjust = Operation::Adjust {
            adjustments: Adjustments::default(),
            selection: None,
        };
        assert!(adjust.is_identity());
        assert!(Operation::Straighten { degrees: 0.0 }.is_identity());
        assert!(!Operation::Straighten { degrees: 2.0 }.is_identity());
    }

    #[test]
    fn previews_run_operations_at_their_scale() {
        let canvas = Canvas::new(4, 2, vec![255; 4 * 2 * 4]);
        let straightened = Operation::Straighten { degrees: 10.0 }.run(&canvas, 1.0);
        // Straightening keeps the shape of the bitmap.
        assert!(straightened.width <= 4 && straightened.height <= 2);
        let inverted = Operation::Adjust {
            adjustments: Adjustments {
                invert: true,
                ..Adjustments::default()
            },
            selection: None,
        }
        .run(&canvas, 0.5);
        assert!(
            inverted
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == [0, 0, 0, 255])
        );
    }
}
