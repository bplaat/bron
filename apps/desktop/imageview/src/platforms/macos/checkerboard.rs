/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::c_void;

use bcanvas::NativeContext;
use imageview_core::appkit::Rect;
use imageview_core::draw_checkerboard;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, define_class, msg_send};

use super::headers::*;

define_class!(
    #[unsafe(super(NSView))]
    #[name = "ImageViewCheckerboardView"]
    struct CheckerboardView;

    impl CheckerboardView {
        #[unsafe(method(drawRect:))]
        fn _draw_rect(&self, _: Rect) {
            self.draw();
        }

        #[unsafe(method(isFlipped))]
        const fn _is_flipped(&self) -> Bool {
            Bool::YES
        }

        #[unsafe(method(isOpaque))]
        const fn _is_opaque(&self) -> Bool {
            Bool::YES
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn _view_did_change_effective_appearance(&self) {
            // SAFETY: self is a live NSView and the selector takes a scalar Objective-C BOOL.
            unsafe {
                let this = self as *const Self as *mut Object;
                let _: () = msg_send![this, setNeedsDisplay: Bool::YES];
            }
        }
    }
);

impl CheckerboardView {
    fn draw(&self) {
        // SAFETY: Drawing occurs during AppKit's draw pass with a live view and graphics context.
        unsafe {
            let graphics_context: *mut Object =
                msg_send![class!(NSGraphicsContext), currentContext];
            if graphics_context.is_null() {
                return;
            }
            let context: *mut c_void = msg_send![graphics_context, CGContext];
            if context.is_null() {
                return;
            }
            let this = self as *const Self as *mut Object;
            let bounds: Rect = msg_send![this, bounds];
            let dark = uses_dark_appearance(this);
            let (width, height) = (bounds.size.width as f32, bounds.size.height as f32);
            bcanvas::draw_native(
                NativeContext::CoreGraphics(context),
                width,
                height,
                1.0,
                |canvas| {
                    draw_checkerboard(canvas, width, height, dark);
                },
            );
        }
    }
}

/// Creates an owned opaque checkerboard view that follows the effective appearance.
pub(crate) fn create_checkerboard_view(frame: Rect) -> Retained<Object> {
    // SAFETY: CheckerboardView is a registered NSView subclass initialized with a valid frame.
    unsafe {
        let view: Allocated<Object> = msg_send![CheckerboardView::class(), alloc];
        msg_send![view, initWithFrame: frame]
    }
}

unsafe fn uses_dark_appearance(view: *mut Object) -> bool {
    // SAFETY: view is live, AppKit owns the appearance names, and the temporary array retains them
    // for the synchronous best-match query.
    unsafe {
        let appearance: *mut Object = msg_send![view, effectiveAppearance];
        let names: *mut Object = msg_send![class!(NSMutableArray), arrayWithCapacity: 2usize];
        let _: () = msg_send![names, addObject: NSAppearanceNameAqua];
        let _: () = msg_send![names, addObject: NSAppearanceNameDarkAqua];
        let best: *mut Object = msg_send![appearance, bestMatchFromAppearancesWithNames: names];
        if best.is_null() {
            return false;
        }
        let dark: Bool = msg_send![best, isEqualToString: NSAppearanceNameDarkAqua];
        dark.as_bool()
    }
}
