/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::null_mut;

use imageview_appkit::{NS_VIEW_HEIGHT_SIZABLE, NS_VIEW_WIDTH_SIZABLE, Point, Rect, Size};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, define_class, msg_send, sel};

use crate::headers::*;

// The magnification the media may be shrunk and enlarged to.
const MINIMUM_MAGNIFICATION: f64 = 0.02;
const MAXIMUM_MAGNIFICATION: f64 = 64.0;

// Command-scroll zoom exponent per wheel line or trackpad point.
const WHEEL_ZOOM_RATE: f64 = 0.15;
const TRACKPAD_ZOOM_RATE: f64 = 0.005;

// Scrolls toward `point` within the media edges and updates the scrollers to the constrained point.
fn scroll_to(scroll_view: *mut Object, clip_view: *mut Object, point: Point) {
    // SAFETY: Both are live AppKit views, and constraining a bounds rectangle of the visible area
    // returns the rectangle the clip view allows.
    unsafe {
        let bounds: Rect = msg_send![clip_view, bounds];
        let constrained: Rect = msg_send![clip_view,
            constrainBoundsRect: Rect {
                origin: point,
                size: bounds.size,
            }
        ];
        let _: () = msg_send![clip_view, scrollToPoint: constrained.origin];
        let _: () = msg_send![scroll_view, reflectScrolledClipView: clip_view];
    }
}

struct ClipViewIvars {
    // Whether the media is being dragged around with the mouse.
    panning: Cell<bool>,
    // Where the pointer was at the previous step of the drag, in window coordinates.
    pointer: Cell<Point>,
}

define_class!(
    #[unsafe(super(NSClipView))]
    #[name = "ImageViewClipView"]
    #[ivars = ClipViewIvars]
    struct ClipView;

    impl ClipView {
        // Flipped so the top of the media stays in place while resizing.
        #[unsafe(method(isFlipped))]
        const fn _is_flipped(&self) -> Bool {
            Bool::YES
        }

        #[unsafe(method(constrainBoundsRect:))]
        fn _constrain_bounds_rect(&self, proposed: Rect) -> Rect {
            self.constrain_bounds_rect(proposed)
        }

        // Handle the mouse here instead of in the media view, so every format drags, zooms and
        // scrolls the same way.
        #[unsafe(method(hitTest:))]
        fn _hit_test(&self, point: Point) -> *mut Object {
            self.hit_test(point)
        }

        #[unsafe(method(resetCursorRects))]
        fn _reset_cursor_rects(&self) {
            self.reset_cursor_rects();
        }

        #[unsafe(method(mouseDown:))]
        fn _mouse_down(&self, event: *mut Object) {
            self.mouse_down(event);
        }

        #[unsafe(method(mouseDragged:))]
        fn _mouse_dragged(&self, event: *mut Object) {
            self.mouse_dragged(event);
        }

        #[unsafe(method(mouseUp:))]
        fn _mouse_up(&self, event: *mut Object) {
            self.mouse_up(event);
        }
    }
);

impl ClipView {
    // Centers media that is smaller than the visible area instead of pinning it to a corner.
    fn constrain_bounds_rect(&self, proposed: Rect) -> Rect {
        // SAFETY: NSClipView implements `constrainBoundsRect:` with this argument and return type,
        // and the document view is the one the scroll view installed.
        unsafe {
            let this = self as *const Self as *mut Object;
            let mut rect: Rect = msg_send![super(self), constrainBoundsRect: proposed];

            let document: *mut Object = msg_send![this, documentView];
            if document.is_null() {
                return rect;
            }
            let frame: Rect = msg_send![document, frame];
            if rect.size.width > frame.size.width {
                rect.origin.x = (frame.size.width - rect.size.width) / 2.0;
            }
            if rect.size.height > frame.size.height {
                rect.origin.y = (frame.size.height - rect.size.height) / 2.0;
            }
            rect
        }
    }

    // Returns the clip view for every point of the visible area, and nothing outside of it.
    fn hit_test(&self, point: Point) -> *mut Object {
        // SAFETY: The clip view is a live view whose frame is in the coordinates the point is in.
        unsafe {
            let this = self as *const Self as *mut Object;
            let frame: Rect = msg_send![this, frame];
            let inside = point.x >= frame.origin.x
                && point.x <= frame.origin.x + frame.size.width
                && point.y >= frame.origin.y
                && point.y <= frame.origin.y + frame.size.height;
            if inside { this } else { null_mut() }
        }
    }

    // Returns the scale between media coordinates and window points.
    fn magnification(&self) -> f64 {
        // SAFETY: The clip view is a live view, whose bounds are its frame divided by the
        // magnification of the scroll view around it.
        unsafe {
            let this = self as *const Self as *mut Object;
            let frame: Rect = msg_send![this, frame];
            let bounds: Rect = msg_send![this, bounds];
            if bounds.size.width > 0.0 {
                frame.size.width / bounds.size.width
            } else {
                1.0
            }
        }
    }

    // Returns whether the media is larger than the visible area, and can be dragged around.
    fn is_scrollable(&self) -> bool {
        // SAFETY: The scroll view keeps the document view alive while it is installed.
        unsafe {
            let this = self as *const Self as *mut Object;
            let document: *mut Object = msg_send![this, documentView];
            if document.is_null() {
                return false;
            }
            let media: Rect = msg_send![document, frame];
            let bounds: Rect = msg_send![this, bounds];
            let magnification = self.magnification();
            // Ignore sub-point overflow from rounding.
            (media.size.width - bounds.size.width) * magnification > 1.0
                || (media.size.height - bounds.size.height) * magnification > 1.0
        }
    }

    // Shows an open hand over media that can be dragged around.
    fn reset_cursor_rects(&self) {
        if !self.is_scrollable() {
            return;
        }
        // SAFETY: The cursors are shared objects of AppKit, and the rectangle is in the
        // coordinates of the clip view itself.
        unsafe {
            let this = self as *const Self as *mut Object;
            let bounds: Rect = msg_send![this, bounds];
            let cursor: *mut Object = msg_send![class!(NSCursor), openHandCursor];
            let _: () = msg_send![this, addCursorRect: bounds, cursor: cursor];
        }
    }

    // Takes hold of media that is larger than the window, to drag it around with.
    fn mouse_down(&self, event: *mut Object) {
        if !self.is_scrollable() {
            self.pass_on(sel!(mouseDown:), event);
            return;
        }
        // SAFETY: AppKit passes a valid NSEvent, and the pushed cursor is popped on mouse up.
        unsafe {
            let location: Point = msg_send![event, locationInWindow];
            self.ivars().pointer.set(location);
            self.ivars().panning.set(true);
            let cursor: *mut Object = msg_send![class!(NSCursor), closedHandCursor];
            let _: () = msg_send![cursor, push];
        }
    }

    // Moves the media along with the pointer.
    fn mouse_dragged(&self, event: *mut Object) {
        if !self.ivars().panning.get() {
            self.pass_on(sel!(mouseDragged:), event);
            return;
        }
        // SAFETY: AppKit passes a valid NSEvent, and the clip view stays inside the scroll view
        // for as long as a drag of it lasts.
        unsafe {
            let this = self as *const Self as *mut Object;
            let scroll_view: *mut Object = msg_send![this, enclosingScrollView];
            if scroll_view.is_null() {
                return;
            }
            let location: Point = msg_send![event, locationInWindow];
            let previous = self.ivars().pointer.replace(location);
            let bounds: Rect = msg_send![this, bounds];
            let magnification = self.magnification();
            // Move relative to the current position, so dragging past an edge does not build up
            // distance. The clip view is flipped, so the y delta is inverted.
            scroll_to(
                scroll_view,
                this,
                Point {
                    x: bounds.origin.x - (location.x - previous.x) / magnification,
                    y: bounds.origin.y + (location.y - previous.y) / magnification,
                },
            );
        }
    }

    // Lets go of the media at the end of a drag.
    fn mouse_up(&self, event: *mut Object) {
        if !self.ivars().panning.replace(false) {
            self.pass_on(sel!(mouseUp:), event);
            return;
        }
        // SAFETY: The closed hand cursor of the drag is on top of the cursor stack.
        unsafe {
            let _: () = msg_send![class!(NSCursor), pop];
        }
    }

    // Hands a mouse event that is not a drag of the media to the rest of the responder chain.
    fn pass_on(&self, selector: objc2::runtime::Sel, event: *mut Object) {
        // SAFETY: The clip view has a next responder while it is inside a window, and all mouse
        // messages take one event and return nothing.
        unsafe {
            let this = self as *const Self as *mut Object;
            let responder: *mut Object = msg_send![this, nextResponder];
            if responder.is_null() {
                return;
            }
            let send: unsafe extern "C-unwind" fn(*mut Object, objc2::runtime::Sel, *mut Object) =
                std::mem::transmute(objc2::ffi::objc_msgSend as *const c_void);
            send(responder, selector, event);
        }
    }
}

define_class!(
    #[unsafe(super(NSScrollView))]
    #[name = "ImageViewScrollView"]
    struct ScrollView;

    impl ScrollView {
        // Shows the media at the given magnification, in the middle of the window.
        #[unsafe(method(setZoom:))]
        fn _set_zoom(&self, magnification: f64) {
            self.magnify(magnification);
            self.scroll_to_center();
        }

        // Magnifies by a step, keeping the middle of the visible area in place.
        #[unsafe(method(zoomBy:))]
        fn _zoom_by(&self, factor: f64) {
            // SAFETY: The scroll view magnifies itself.
            let magnification: f64 =
                unsafe { msg_send![self as *const Self as *mut Object, magnification] };
            self.magnify_around_center(magnification * factor);
        }

        // Shows all of the media, in the middle of the window.
        #[unsafe(method(zoomToFit))]
        fn _zoom_to_fit(&self) {
            self.magnify(self.fit_magnification());
            self.scroll_to_center();
        }

        #[unsafe(method(scrollWheel:))]
        fn _scroll_wheel(&self, event: *mut Object) {
            self.scroll_wheel(event);
        }

        // Always use overlay scrollers, AppKit switches to legacy scrollers when a mouse is attached.
        #[unsafe(method(setScrollerStyle:))]
        fn _set_scroller_style(&self, _style: i64) {
            self.set_scroller_style(NS_SCROLLER_STYLE_OVERLAY);
        }
    }
);

impl ScrollView {
    // Returns the magnification that shows all of the media.
    fn fit_magnification(&self) -> f64 {
        // SAFETY: The scroll view keeps its document view alive, and its content size is the
        // visible area in points.
        unsafe {
            let this = self as *const Self as *mut Object;
            let document: *mut Object = msg_send![this, documentView];
            if document.is_null() {
                return 1.0;
            }
            let media: Rect = msg_send![document, frame];
            if media.size.width <= 0.0 || media.size.height <= 0.0 {
                return 1.0;
            }
            let content: Size = msg_send![this, contentSize];
            (content.width.max(1.0) / media.size.width)
                .min(content.height.max(1.0) / media.size.height)
                .clamp(MINIMUM_MAGNIFICATION, MAXIMUM_MAGNIFICATION)
        }
    }

    // Sets the scroller style, past the override of the setter.
    fn set_scroller_style(&self, style: i64) {
        // SAFETY: NSScrollView implements `setScrollerStyle:` and its argument is an NSInteger.
        unsafe {
            let _: () = msg_send![super(self), setScrollerStyle: style];
        }
    }

    // Magnifies the media, leaving the scroll position to the caller.
    fn magnify(&self, magnification: f64) {
        // SAFETY: The scroll view clamps the magnification to its own range.
        unsafe {
            let this = self as *const Self as *mut Object;
            let _: () = msg_send![this, setMagnification: magnification];
        }
    }

    // Magnifies the media, keeping the middle of the visible area in place.
    fn magnify_around_center(&self, magnification: f64) {
        // SAFETY: The scroll view keeps its clip view alive.
        unsafe {
            let this = self as *const Self as *mut Object;
            let clip_view: *mut Object = msg_send![this, contentView];
            let bounds: Rect = msg_send![clip_view, bounds];
            self.magnify_around(
                magnification,
                Point {
                    x: bounds.origin.x + bounds.size.width / 2.0,
                    y: bounds.origin.y + bounds.size.height / 2.0,
                },
            );
        }
    }

    // Magnifies the media while keeping `anchor` (in media coordinates) under the same window point.
    fn magnify_around(&self, magnification: f64, anchor: Point) {
        // SAFETY: The scroll view keeps its clip view alive.
        unsafe {
            let this = self as *const Self as *mut Object;
            let clip_view: *mut Object = msg_send![this, contentView];
            let before: f64 = msg_send![this, magnification];
            let bounds: Rect = msg_send![clip_view, bounds];
            self.magnify(magnification);
            let after: f64 = msg_send![this, magnification];
            // Keep the visible area at the same scaled distance from the anchor.
            let scale = if after > 0.0 { before / after } else { 1.0 };
            scroll_to(
                this,
                clip_view,
                Point {
                    x: anchor.x + (bounds.origin.x - anchor.x) * scale,
                    y: anchor.y + (bounds.origin.y - anchor.y) * scale,
                },
            );
        }
    }

    // Command + scroll wheel magnifies around the pointer, like a trackpad pinch; other
    // scrolls pan the media.
    fn scroll_wheel(&self, event: *mut Object) {
        // SAFETY: AppKit passes a valid NSEvent and NSScrollView implements `scrollWheel:` with
        // this argument type.
        unsafe {
            let this = self as *const Self as *mut Object;
            let modifiers: u64 = msg_send![event, modifierFlags];
            let delta: f64 = msg_send![event, scrollingDeltaY];
            if modifiers & NS_EVENT_MODIFIER_FLAG_COMMAND == 0 || delta == 0.0 {
                let _: () = msg_send![super(self), scrollWheel: event];
                return;
            }

            let precise: Bool = msg_send![event, hasPreciseScrollingDeltas];
            let rate = if precise.as_bool() {
                TRACKPAD_ZOOM_RATE
            } else {
                WHEEL_ZOOM_RATE
            };
            let magnification: f64 = msg_send![this, magnification];
            let clip_view: *mut Object = msg_send![this, contentView];
            let location: Point = msg_send![event, locationInWindow];
            let pointer: Point = msg_send![clip_view,
                convertPoint: location,
                fromView: null_mut::<Object>()
            ];
            self.magnify_around(magnification * (delta * rate).exp(), pointer);
        }
    }

    // Scrolls back to the middle of the media, so that it cannot stay stuck in a corner.
    fn scroll_to_center(&self) {
        // SAFETY: The scroll view keeps its document and clip views alive.
        unsafe {
            let this = self as *const Self as *mut Object;
            let document: *mut Object = msg_send![this, documentView];
            let clip_view: *mut Object = msg_send![this, contentView];
            if document.is_null() {
                return;
            }
            let media: Rect = msg_send![document, frame];
            let bounds: Rect = msg_send![clip_view, bounds];
            scroll_to(
                this,
                clip_view,
                Point {
                    x: (media.size.width - bounds.size.width) / 2.0,
                    y: (media.size.height - bounds.size.height) / 2.0,
                },
            );
        }
    }
}

/// Creates an owned native scroll view that scrolls and magnifies `document` inside `frame`.
pub(crate) fn create_scroll_view(frame: Rect, document: *mut Object) -> Retained<Object> {
    // SAFETY: All objects are valid AppKit instances, and the clip view is released after the
    // scroll view retains it.
    unsafe {
        let scroll_view: Allocated<Object> = msg_send![ScrollView::class(), alloc];
        let scroll_view: Retained<Object> = msg_send![scroll_view, initWithFrame: frame];

        let clip_view: Allocated<ClipView> = msg_send![ClipView::class(), alloc];
        let clip_view: Retained<ClipView> = msg_send![
            super(clip_view.set_ivars(ClipViewIvars {
                panning: Cell::new(false),
                pointer: Cell::new(Point { x: 0.0, y: 0.0 }),
            })),
            initWithFrame: frame
        ];
        // The checkerboard behind the scroll view is the background of the window.
        let _: () = msg_send![&*clip_view, setDrawsBackground: Bool::NO];
        let _: () = msg_send![&*scroll_view, setContentView: clip_view.as_ptr()];

        let _: () = msg_send![&*scroll_view, setDrawsBackground: Bool::NO];
        let _: () = msg_send![&*scroll_view, setBorderType: NS_NO_BORDER];
        let _: () = msg_send![&*scroll_view, setHasHorizontalScroller: Bool::YES];
        let _: () = msg_send![&*scroll_view, setHasVerticalScroller: Bool::YES];
        let _: () = msg_send![&*scroll_view, setAutohidesScrollers: Bool::YES];
        let _: () = msg_send![&*scroll_view, setScrollerStyle: NS_SCROLLER_STYLE_OVERLAY];
        let _: () = msg_send![&*scroll_view, setAllowsMagnification: Bool::YES];
        let _: () = msg_send![&*scroll_view, setMinMagnification: MINIMUM_MAGNIFICATION];
        let _: () = msg_send![&*scroll_view, setMaxMagnification: MAXIMUM_MAGNIFICATION];
        let _: () = msg_send![&*scroll_view, setDocumentView: document];
        let _: () = msg_send![&*scroll_view,
            setAutoresizingMask: NS_VIEW_WIDTH_SIZABLE | NS_VIEW_HEIGHT_SIZABLE
        ];
        scroll_view
    }
}

/// Replaces the media, preserving zoom on refresh or optionally fitting it to the window.
///
/// # Safety
///
/// `scroll_view` must point to a live `NSScrollView` and `document` to a live `NSView` that the
/// scroll view can retain.
pub(crate) unsafe fn replace_document_view(
    scroll_view: *mut Object,
    document: *mut Object,
    zoom_to_fit: bool,
) {
    // SAFETY: The caller supplies live AppKit views and NSScrollView implements these selectors.
    unsafe {
        let magnification: f64 = msg_send![scroll_view, magnification];
        let _: () = msg_send![scroll_view, setDocumentView: document];
        if zoom_to_fit {
            let _: () = msg_send![scroll_view, zoomToFit];
        } else {
            let _: () = msg_send![scroll_view, setMagnification: magnification];
        }
    }
}

#[cfg(test)]
mod tests {
    use objc2::rc::{Allocated, Retained, autoreleasepool};
    use objc2::{class, msg_send};

    use super::*;

    #[test]
    fn replacing_document_view_can_keep_the_zoom() {
        // SAFETY: All views stay retained for the test and every selector matches its AppKit ABI.
        autoreleasepool(|_| unsafe {
            let frame = Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 320.0,
                    height: 240.0,
                },
            };
            let first: Allocated<Object> = msg_send![class!(NSView), alloc];
            let first: Retained<Object> = msg_send![first, initWithFrame: frame];
            let second: Allocated<Object> = msg_send![class!(NSView), alloc];
            let second: Retained<Object> = msg_send![second, initWithFrame: frame];
            let scroll_view = create_scroll_view(frame, first.as_ptr());
            let _: () = msg_send![&*scroll_view, setMagnification: 2.5f64];

            replace_document_view(scroll_view.as_ptr(), second.as_ptr(), false);

            let magnification: f64 = msg_send![&*scroll_view, magnification];
            assert_eq!(magnification, 2.5);
        });
    }
}
