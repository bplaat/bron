/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::{Cell, RefCell};
use std::ptr::null_mut;

use macview_appkit::{Image, Size, ns_string};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, define_class, msg_send, sel};

use crate::edit_panel::EditPanel;
use crate::headers::*;
use crate::scroll_view::replace_document_view;
use crate::selection_view::{SelectionView, attach_selection_view, create_selection_view};
use crate::toolbar::{
    allowed_item_identifiers, create_item, create_toolbar, default_item_identifiers,
};

// The factor a single zoom step magnifies or shrinks the media by.
const ZOOM_STEP: f64 = 1.5;

pub(crate) struct WindowControllerIvars {
    size: Cell<Size>,
    overlay: Retained<SelectionView>,
    // The popover that previews an edit, kept alive until the next one replaces it.
    panel: RefCell<Option<Retained<EditPanel>>>,
    spinner: RefCell<Option<Retained<Object>>>,
}

define_class!(
    #[unsafe(super(NSWindowController))]
    #[name = "MacViewWindowController"]
    #[ivars = WindowControllerIvars]
    pub(crate) struct WindowController;

    impl WindowController {
        #[unsafe(method(windowTitleForDocumentDisplayName:))]
        fn _window_title_for_document_display_name(
            &self,
            display_name: *mut Object,
        ) -> *mut Object {
            self.window_title(display_name)
        }

        #[unsafe(method(zoomIn:))]
        fn _zoom_in(&self, _: *mut Object) {
            self.zoom_by(ZOOM_STEP);
        }

        #[unsafe(method(zoomOut:))]
        fn _zoom_out(&self, _: *mut Object) {
            self.zoom_by(1.0 / ZOOM_STEP);
        }

        // The zoom toolbar group: zoom out, then zoom in.
        #[unsafe(method(zoom:))]
        fn _zoom(&self, group: *mut Object) {
            let factor = if selected_index(group) == 0 { 1.0 / ZOOM_STEP } else { ZOOM_STEP };
            self.zoom_by(factor);
        }

        #[unsafe(method(actualSize:))]
        fn _actual_size(&self, _: *mut Object) {
            self.send_to_scroll_view(sel!(setZoom:), 1.0);
        }

        #[unsafe(method(zoomToFit:))]
        fn _zoom_to_fit(&self, _: *mut Object) {
            let scroll_view = self.scroll_view();
            if !scroll_view.is_null() {
                // SAFETY: scroll_view is the scroll view of this window.
                unsafe {
                    let _: () = msg_send![scroll_view, zoomToFit];
                }
            }
        }

        #[unsafe(method(toolbar:itemForItemIdentifier:willBeInsertedIntoToolbar:))]
        fn _toolbar_item(&self, _: *mut Object, identifier: *mut Object, _: Bool) -> *mut Object {
            create_item(identifier)
        }

        #[unsafe(method(toolbarDefaultItemIdentifiers:))]
        fn _toolbar_default_item_identifiers(&self, _: *mut Object) -> *mut Object {
            default_item_identifiers()
        }

        #[unsafe(method(toolbarAllowedItemIdentifiers:))]
        fn _toolbar_allowed_item_identifiers(&self, _: *mut Object) -> *mut Object {
            allowed_item_identifiers()
        }

        #[unsafe(method(validateMenuItem:))]
        fn _validate_menu_item(&self, item: *mut Object) -> Bool {
            objc_bool(self.validate_item(item, true))
        }

        #[unsafe(method(validateToolbarItem:))]
        fn _validate_toolbar_item(&self, item: *mut Object) -> Bool {
            objc_bool(self.validate_item(item, false))
        }

        #[unsafe(method(selectTool:))]
        fn _select_tool(&self, group: *mut Object) {
            self.select_tool_at(selected_index(group));
        }

        #[unsafe(method(usePanTool:))]
        fn _use_pan_tool(&self, _: *mut Object) {
            self.select_tool_at(0);
        }

        #[unsafe(method(useRectangleSelection:))]
        fn _use_rectangle_selection(&self, _: *mut Object) {
            self.select_tool_at(1);
        }

        #[unsafe(method(useEllipseSelection:))]
        fn _use_ellipse_selection(&self, _: *mut Object) {
            self.select_tool_at(2);
        }

        #[unsafe(method(selectAll:))]
        fn _select_all(&self, _: *mut Object) {
            self.select_all();
        }

        #[unsafe(method(deselectAll:))]
        fn _deselect_all(&self, _: *mut Object) {
            self.ivars().overlay.set_selection(None);
        }

        #[unsafe(method(selectAspectRatio:))]
        fn _select_aspect_ratio(&self, item: *mut Object) {
            self.select_aspect_ratio(item);
        }

        #[unsafe(method(cropToSelection:))]
        fn _crop_to_selection(&self, _: *mut Object) {
            self.crop_to_selection();
        }

        // The rotate toolbar group: left, then right.
        #[unsafe(method(rotate:))]
        fn _rotate(&self, group: *mut Object) {
            self.rotate_quarters(if selected_index(group) == 0 { 3 } else { 1 });
        }

        #[unsafe(method(rotateLeft:))]
        fn _rotate_left(&self, _: *mut Object) {
            self.rotate_quarters(3);
        }

        #[unsafe(method(rotateRight:))]
        fn _rotate_right(&self, _: *mut Object) {
            self.rotate_quarters(1);
        }

        #[unsafe(method(rotateHalf:))]
        fn _rotate_half(&self, _: *mut Object) {
            self.rotate_quarters(2);
        }

        #[unsafe(method(rotateByAngle:))]
        fn _rotate_by_angle(&self, _: *mut Object) {
            self.rotate_by_angle();
        }

        #[unsafe(method(straighten:))]
        fn _straighten(&self, _: *mut Object) {
            self.show_panel(crate::edit_panel::PanelKind::Straighten);
        }

        // The flip toolbar group: horizontal, then vertical.
        #[unsafe(method(flip:))]
        fn _flip(&self, group: *mut Object) {
            self.flip(selected_index(group) == 0);
        }

        #[unsafe(method(flipHorizontal:))]
        fn _flip_horizontal(&self, _: *mut Object) {
            self.flip(true);
        }

        #[unsafe(method(flipVertical:))]
        fn _flip_vertical(&self, _: *mut Object) {
            self.flip(false);
        }

        #[unsafe(method(resizeImage:))]
        fn _resize_image(&self, _: *mut Object) {
            self.resize_image(false);
        }

        #[unsafe(method(resizeCanvas:))]
        fn _resize_canvas(&self, _: *mut Object) {
            self.resize_image(true);
        }

        #[unsafe(method(showAdjustments:))]
        fn _show_adjustments(&self, _: *mut Object) {
            self.show_panel(crate::edit_panel::PanelKind::Adjust);
        }

        #[unsafe(method(showEffects:))]
        fn _show_effects(&self, _: *mut Object) {
            self.show_panel(crate::edit_panel::PanelKind::Effects);
        }

        #[unsafe(method(applyPreset:))]
        fn _apply_preset(&self, item: *mut Object) {
            self.apply_preset(item);
        }

        #[unsafe(method(applyGrayscale:))]
        fn _apply_grayscale(&self, _: *mut Object) {
            self.apply_color(true);
        }

        #[unsafe(method(applyInvert:))]
        fn _apply_invert(&self, _: *mut Object) {
            self.apply_color(false);
        }

        #[unsafe(method(toggleCompare:))]
        fn _toggle_compare(&self, _: *mut Object) {
            if let Some(document) = self.document() {
                document.toggle_compare();
            }
        }

        #[unsafe(method(resetToOriginal:))]
        fn _reset_to_original(&self, _: *mut Object) {
            if let Some(document) = self.document() {
                document.reset_to_original();
            }
        }

        #[unsafe(method(copy:))]
        fn _copy(&self, _: *mut Object) {
            if let Some(document) = self.document() {
                document.copy_image(self.ivars().overlay.selection());
            }
        }

        #[unsafe(method(paste:))]
        fn _paste(&self, _: *mut Object) {
            crate::editing::open_pasted_image();
        }
    }
);

impl WindowController {
    fn window_title(&self, display_name: *mut Object) -> *mut Object {
        let size = self.ivars().size.get();
        let suffix = format!(
            " ({}x{})",
            size.width.round() as i64,
            size.height.round() as i64
        );
        // SAFETY: AppKit passes a valid NSString and the appended string is autoreleased, which
        // matches the ownership convention of this method.
        unsafe { msg_send![display_name, stringByAppendingString: ns_string(&suffix)] }
    }

    /// Returns the overlay that draws and edits the selection.
    pub(crate) fn overlay(&self) -> &SelectionView {
        &self.ivars().overlay
    }

    /// Keeps the popover that previews an edit alive while it is shown.
    pub(crate) fn set_panel(&self, panel: Option<Retained<EditPanel>>) {
        self.ivars().panel.replace(panel);
    }

    pub(crate) const fn as_object(&self) -> *mut Object {
        self as *const Self as *mut Object
    }

    /// Returns the window, or null before the controller has one.
    pub(crate) fn window(&self) -> *mut Object {
        // SAFETY: The controller is live.
        unsafe { msg_send![self.as_object(), window] }
    }

    /// Returns the document of the window, while it has one of this application.
    pub(crate) fn document(&self) -> Option<&crate::Document> {
        // SAFETY: The document outlives its window controllers, and the class check makes the
        // cast sound.
        unsafe {
            let document: *mut Object = msg_send![self.as_object(), document];
            if document.is_null() {
                return None;
            }
            let ours: Bool = msg_send![document, isKindOfClass: crate::Document::class()];
            ours.as_bool().then(|| &*document.cast::<crate::Document>())
        }
    }

    /// Returns the scroll view that magnifies the media, or null before the window has one.
    pub(crate) fn scroll_view(&self) -> *mut Object {
        // SAFETY: The window and its view hierarchy are live while the controller is.
        unsafe {
            let window = self.window();
            if window.is_null() {
                return null_mut();
            }
            let content_view: *mut Object = msg_send![window, contentView];
            let subviews: *mut Object = msg_send![content_view, subviews];
            let count: usize = msg_send![subviews, count];
            for index in 0..count {
                let view: *mut Object = msg_send![subviews, objectAtIndex: index];
                let is_scroll_view: Bool = msg_send![view, isKindOfClass: class!(NSScrollView)];
                if is_scroll_view.as_bool() {
                    return view;
                }
            }
            null_mut()
        }
    }

    // Zooms the media of this window by `factor`.
    fn zoom_by(&self, factor: f64) {
        self.send_to_scroll_view(sel!(zoomBy:), factor);
    }

    // Sends a zoom message that takes a single number to the scroll view of this window.
    fn send_to_scroll_view(&self, selector: objc2::runtime::Sel, value: f64) {
        let scroll_view = self.scroll_view();
        if scroll_view.is_null() {
            return;
        }
        // SAFETY: Both zoom messages of the scroll view take one double and return nothing.
        unsafe {
            let send: unsafe extern "C-unwind" fn(*mut Object, objc2::runtime::Sel, f64) =
                std::mem::transmute(objc2::ffi::objc_msgSend as *const std::ffi::c_void);
            send(scroll_view, selector, value);
        }
    }

    // Shows the editing toolbar for bitmaps, and no toolbar for other media.
    fn show_toolbar(&self, editable: bool) {
        // SAFETY: The window retains the toolbar, whose delegate is this controller.
        unsafe {
            let window = self.window();
            let toolbar: *mut Object = msg_send![window, toolbar];
            if editable && toolbar.is_null() {
                let toolbar = create_toolbar(self.as_object());
                let _: () = msg_send![window, setToolbar: toolbar.as_ptr()];
                let _: () = msg_send![window, setToolbarStyle: NS_WINDOW_TOOLBAR_STYLE_UNIFIED];
            } else if !editable && !toolbar.is_null() {
                let _: () = msg_send![window, setToolbar: null_mut::<Object>()];
            }
        }
    }
}

// Returns the selected segment of a toolbar item group.
fn selected_index(group: *mut Object) -> isize {
    // SAFETY: Toolbar groups send their actions with themselves, menu items never reach here.
    unsafe {
        let is_group: Bool = msg_send![group, isKindOfClass: class!(NSToolbarItemGroup)];
        if is_group.as_bool() {
            msg_send![group, selectedIndex]
        } else {
            0
        }
    }
}

/// Returns the window controller of this application behind `controller`, if it is one.
///
/// # Safety
///
/// `controller` must point to a live `NSWindowController`.
pub(crate) unsafe fn ours(controller: *mut Object) -> Option<&'static WindowController> {
    // SAFETY: The caller supplies a live controller, and the class check makes the cast sound.
    // The document keeps its controllers alive while it uses them.
    unsafe {
        let ours: Bool = msg_send![controller, isKindOfClass: WindowController::class()];
        ours.as_bool()
            .then(|| &*(controller.cast::<WindowController>()))
    }
}

/// Replaces media in this controller without resizing its window and updates its size title.
/// `canvas_size` is the size of the bitmap in pixels when it can be edited.
///
/// # Safety
///
/// `controller` must point to a valid `NSWindowController` and `view` to a valid `NSView`, which
/// the scroll view of the window takes ownership of.
pub(crate) unsafe fn show_media(
    controller: *mut Object,
    view: *mut Object,
    size: Size,
    zoom_to_fit: bool,
    canvas_size: Option<Size>,
) {
    // SAFETY: The caller supplies live AppKit objects.
    unsafe {
        let Some(controller_ref) = ours(controller) else {
            return;
        };
        controller_ref.ivars().size.set(size);

        let scroll_view = controller_ref.scroll_view();
        if !scroll_view.is_null() {
            replace_document_view(scroll_view, view, zoom_to_fit);
        }
        controller_ref.install_overlay(view, canvas_size);
        let _: () = msg_send![controller, synchronizeWindowTitleWithDocumentName];
    }
}

impl WindowController {
    // Puts the selection overlay on a new media view, or takes it away for media that cannot be
    // edited, and shows the toolbar that goes with it.
    pub(crate) fn install_overlay(&self, media_view: *mut Object, canvas_size: Option<Size>) {
        let overlay = &self.ivars().overlay;
        match canvas_size {
            // SAFETY: The caller supplies the live media view of this window.
            Some(canvas_size) => unsafe { attach_selection_view(overlay, media_view, canvas_size) },
            None => {
                overlay.set_selection(None);
                overlay.set_tool(crate::selection_view::Tool::Pan);
                // SAFETY: The overlay is a live view.
                unsafe {
                    let _: () = msg_send![&**overlay, removeFromSuperview];
                }
            }
        }
        self.show_toolbar(canvas_size.is_some());
    }
}

/// Shows `image` in the existing media view of `controller`, keeping its zoom and position.
///
/// # Safety
///
/// `controller` must point to a live `NSWindowController`.
pub(crate) unsafe fn update_media_image(controller: *mut Object, image: &Image) {
    // SAFETY: The caller supplies a live controller, whose scroll view owns the media view.
    unsafe {
        let Some(controller) = ours(controller) else {
            return;
        };
        let scroll_view = controller.scroll_view();
        if scroll_view.is_null() {
            return;
        }
        let media_view: *mut Object = msg_send![scroll_view, documentView];
        let image_view: Bool = msg_send![media_view, isKindOfClass: class!(NSImageView)];
        if image_view.as_bool() {
            let _: () = msg_send![media_view, setImage: image.as_ptr()];
        }
    }
}

/// Shows a spinner over the media of `controller` while an edit is applied.
///
/// # Safety
///
/// `controller` must point to a live `NSWindowController`.
pub(crate) unsafe fn set_busy(controller: *mut Object, busy: bool) {
    // SAFETY: The caller supplies a live controller, whose content view retains the spinner.
    unsafe {
        let Some(controller) = ours(controller) else {
            return;
        };
        let mut spinner = controller.ivars().spinner.borrow_mut();
        if !busy {
            if let Some(spinner) = spinner.take() {
                let _: () = msg_send![&*spinner, stopAnimation: null_mut::<Object>()];
                let _: () = msg_send![&*spinner, removeFromSuperview];
            }
            return;
        }
        if spinner.is_some() {
            return;
        }
        let window = controller.window();
        let content_view: *mut Object = msg_send![window, contentView];
        let indicator: Retained<Object> = msg_send![class!(NSProgressIndicator), new];
        let _: () = msg_send![&*indicator, setStyle: NS_PROGRESS_INDICATOR_STYLE_SPINNING];
        let _: () = msg_send![&*indicator, setIndeterminate: Bool::YES];
        let _: () = msg_send![&*indicator, setTranslatesAutoresizingMaskIntoConstraints: Bool::NO];
        let _: () = msg_send![content_view, addSubview: indicator.as_ptr()];
        for (attribute, anchor) in [
            (sel!(centerXAnchor), sel!(centerXAnchor)),
            (sel!(centerYAnchor), sel!(centerYAnchor)),
        ] {
            let send: unsafe extern "C-unwind" fn(*mut Object, objc2::runtime::Sel) -> *mut Object =
                std::mem::transmute(objc2::ffi::objc_msgSend as *const std::ffi::c_void);
            let own = send(indicator.as_ptr(), attribute);
            let other = send(content_view, anchor);
            let constraint: *mut Object = msg_send![own, constraintEqualToAnchor: other];
            let _: () = msg_send![constraint, setActive: Bool::YES];
        }
        let _: () = msg_send![&*indicator, startAnimation: null_mut::<Object>()];
        *spinner = Some(indicator);
    }
}

/// Creates an `NSWindowController` with one owned retain count and a media-size window title.
pub(crate) fn create_window_controller(window: *mut Object, size: Size) -> Retained<Object> {
    // SAFETY: WindowController is a registered NSWindowController subclass and window is a live
    // NSWindow. Its Rust ivars are initialized before AppKit can ask for a window title.
    unsafe {
        let controller: Allocated<WindowController> = msg_send![WindowController::class(), alloc];
        let controller: Retained<WindowController> = msg_send![
            super(controller.set_ivars(WindowControllerIvars {
                size: Cell::new(size),
                overlay: create_selection_view(),
                panel: RefCell::new(None),
                spinner: RefCell::new(None),
            })),
            initWithWindow: window
        ];

        Retained::into_any(controller)
    }
}
