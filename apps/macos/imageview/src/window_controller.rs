/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::Cell;
use std::ptr::null_mut;

use imageview_appkit::{Size, ns_string};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, define_class, msg_send, sel};

use crate::scroll_view::replace_document_view;

// The factor a single zoom step magnifies or shrinks the media by.
const ZOOM_STEP: f64 = 1.5;

struct WindowControllerIvars {
    size: Cell<Size>,
}

define_class!(
    #[unsafe(super(NSWindowController))]
    #[name = "ImageViewWindowController"]
    #[ivars = WindowControllerIvars]
    struct WindowController;

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

    // Returns the scroll view that magnifies the media, or null before the window has one.
    fn scroll_view(&self) -> *mut Object {
        // SAFETY: The window and its view hierarchy are live while the controller is.
        unsafe {
            let this = self as *const Self as *mut Object;
            let window: *mut Object = msg_send![this, window];
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
}

/// Replaces media in this controller without resizing its window and updates its size title.
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
) {
    // SAFETY: The caller supplies live AppKit objects, and the class check makes the cast to the
    // window controller of this application sound.
    unsafe {
        let ours: Bool = msg_send![controller, isKindOfClass: WindowController::class()];
        if !ours.as_bool() {
            return;
        }
        let controller_ref = &*(controller.cast::<WindowController>());
        controller_ref.ivars().size.set(size);

        let scroll_view = controller_ref.scroll_view();
        if !scroll_view.is_null() {
            replace_document_view(scroll_view, view, zoom_to_fit);
        }
        let _: () = msg_send![controller, synchronizeWindowTitleWithDocumentName];
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
            })),
            initWithWindow: window
        ];

        Retained::into_any(controller)
    }
}
