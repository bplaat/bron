/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The AppKit media view: a flipped view that draws the viewport with bcanvas, below a native
//! NSScrollView that only shows the scrollers. The scroll view scrolls an empty document as large
//! as the magnified media, its scroll position is followed by the viewport and the other way
//! around.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::ptr::null_mut;

use bcanvas::NativeContext;
use bwindow::ffi::{file_drag_operation, perform_file_drop, register_dragged_types};
use imageview_core::appkit::{NS_VIEW_HEIGHT_SIZABLE, NS_VIEW_WIDTH_SIZABLE, Point, Rect, Size};
use imageview_core::edit::{Selection, Shape};
use imageview_core::{Media, PRECISE_ZOOM_RATE, WHEEL_ZOOM_RATE};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject as Object, Bool, Sel};
use objc2::{class, define_class, msg_send, sel};

use super::headers::*;
use crate::AppEvent;
use crate::viewport::{Axis, Step, Viewport};

define_class!(
    // Leaves everything but its scrollers to the view below it.
    #[unsafe(super(NSScrollView))]
    #[name = "ImageViewScrollView"]
    struct ScrollView;

    impl ScrollView {
        #[unsafe(method(hitTest:))]
        fn _hit_test(&self, point: Point) -> *mut Object {
            // SAFETY: NSScrollView implements `hitTest:` and keeps its clip and document views.
            unsafe {
                let hit: *mut Object = msg_send![super(self), hitTest: point];
                let clip: *mut Object = msg_send![self, contentView];
                let document: *mut Object = msg_send![self, documentView];
                if hit == clip || hit == document { null_mut() } else { hit }
            }
        }
    }
);

define_class!(
    // The empty document of the scroll view, flipped so it scrolls from the top.
    #[unsafe(super(NSView))]
    #[name = "ImageViewScrollDocument"]
    struct Document;

    impl Document {
        #[unsafe(method(isFlipped))]
        const fn _is_flipped(&self) -> Bool {
            Bool::YES
        }
    }
);

pub(super) struct ViewIvars {
    viewport: RefCell<Viewport>,
    dark: Cell<bool>,
    scroll_view: Retained<Object>,
    document: Retained<Object>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[name = "ImageViewCanvasView"]
    #[ivars = ViewIvars]
    pub(super) struct View;

    impl View {
        #[unsafe(method(draggingEntered:))]
        fn _dragging_entered(&self, sender: *mut Object) -> u64 {
            // SAFETY: AppKit supplies the current dragging info on its UI thread.
            unsafe { file_drag_operation(sender) }
        }

        #[unsafe(method(draggingUpdated:))]
        fn _dragging_updated(&self, sender: *mut Object) -> u64 {
            // SAFETY: AppKit supplies the current dragging info on its UI thread.
            unsafe { file_drag_operation(sender) }
        }

        #[unsafe(method(prepareForDragOperation:))]
        const fn _prepare_for_drag_operation(&self, _: *mut Object) -> Bool {
            Bool::YES
        }

        #[unsafe(method(performDragOperation:))]
        fn _perform_drag_operation(&self, sender: *mut Object) -> Bool {
            // SAFETY: AppKit supplies the completed drag on its UI thread.
            unsafe { perform_file_drop(sender) }
        }

        #[unsafe(method(isFlipped))]
        const fn _is_flipped(&self) -> Bool {
            Bool::YES
        }

        #[unsafe(method(isOpaque))]
        const fn _is_opaque(&self) -> Bool {
            Bool::YES
        }

        #[unsafe(method(acceptsFirstResponder))]
        const fn _accepts_first_responder(&self) -> Bool {
            Bool::YES
        }

        #[unsafe(method(drawRect:))]
        fn _draw_rect(&self, _: Rect) {
            self.draw();
        }

        #[unsafe(method(setFrameSize:))]
        fn _set_frame_size(&self, size: Size) {
            // SAFETY: NSView implements `setFrameSize:` with this argument type.
            unsafe {
                let _: () = msg_send![super(self), setFrameSize: size];
            }
            self.update(|_| {});
        }

        // The scroll view scrolled, by itself or through `update`, or its visible area changed,
        // like when legacy scrollers appear.
        #[unsafe(method(clipViewChanged:))]
        fn _clip_view_changed(&self, _: *mut Object) {
            let bounds = self.clip_bounds();
            let pages = Axis::BOTH.map(|axis| self.ivars().viewport.borrow().scroll_bar(axis).page);
            let resized = pages != [bounds.size.width, bounds.size.height];
            if resized {
                // The viewport keeps the middle in place, which the scroll view then follows.
                self.update(|_| {});
                return;
            }
            self.ivars()
                .viewport
                .borrow_mut()
                .scroll_to((bounds.origin.x, bounds.origin.y));
            // SAFETY: The view is a live NSView.
            unsafe {
                let _: () = msg_send![self, setNeedsDisplay: Bool::YES];
            }
        }

        // Command + scroll magnifies around the pointer, the scroll view scrolls the others.
        #[unsafe(method(scrollWheel:))]
        fn _scroll_wheel(&self, event: *mut Object) {
            // SAFETY: AppKit passes a valid scroll NSEvent.
            let (modifiers, precise, delta) = unsafe {
                let modifiers: u64 = msg_send![event, modifierFlags];
                let precise: Bool = msg_send![event, hasPreciseScrollingDeltas];
                let delta: f64 = msg_send![event, scrollingDeltaY];
                (modifiers, precise.as_bool(), delta)
            };
            if modifiers & NS_EVENT_MODIFIER_FLAG_COMMAND == 0 {
                // SAFETY: The scroll view is live and implements `scrollWheel:`.
                unsafe {
                    let _: () = msg_send![&*self.ivars().scroll_view, scrollWheel: event];
                }
                return;
            }
            let rate = if precise { PRECISE_ZOOM_RATE } else { WHEEL_ZOOM_RATE };
            let pointer = self.pointer(event);
            self.update(|viewport| viewport.zoom_by((delta * rate).exp(), Some(pointer)));
        }

        #[unsafe(method(magnifyWithEvent:))]
        fn _magnify_with_event(&self, event: *mut Object) {
            // SAFETY: AppKit passes a valid magnify NSEvent.
            let magnification: f64 = unsafe { msg_send![event, magnification] };
            let pointer = self.pointer(event);
            self.update(|viewport| viewport.zoom_by(1.0 + magnification, Some(pointer)));
        }

        #[unsafe(method(mouseDown:))]
        fn _mouse_down(&self, event: *mut Object) {
            let pointer = self.pointer(event);
            if self.ivars().viewport.borrow_mut().mark_begin(pointer) {
                self.update(|_| {});
                return;
            }
            if self.ivars().viewport.borrow_mut().drag_begin(pointer) {
                // SAFETY: The cursor is a shared AppKit object, popped on mouse up.
                unsafe {
                    let cursor: *mut Object = msg_send![class!(NSCursor), closedHandCursor];
                    let _: () = msg_send![cursor, push];
                }
            }
        }

        #[unsafe(method(mouseDragged:))]
        fn _mouse_dragged(&self, event: *mut Object) {
            let pointer = self.pointer(event);
            // SAFETY: AppKit passes a valid mouse NSEvent.
            let modifiers: u64 = unsafe { msg_send![event, modifierFlags] };
            let square = modifiers & NS_EVENT_MODIFIER_FLAG_SHIFT != 0;
            let moved = {
                let mut viewport = self.ivars().viewport.borrow_mut();
                viewport.mark_drag(pointer, square) || viewport.drag_to(pointer)
            };
            if moved {
                self.update(|_| {});
            }
        }

        #[unsafe(method(mouseUp:))]
        fn _mouse_up(&self, _: *mut Object) {
            let (marked, cleared) = {
                let mut viewport = self.ivars().viewport.borrow_mut();
                let marked = viewport.mark_end();
                let cleared = viewport.selecting().is_some() && viewport.selection().is_none();
                (marked, marked && cleared)
            };
            // A click without a drag goes back to editing the whole image, the menu follows a
            // drawn selection.
            if cleared {
                super::send(AppEvent::Action("select.whole"));
            } else if marked {
                super::send(AppEvent::Action("select.changed"));
            }
            if marked {
                self.update(|_| {});
            } else if self.ivars().viewport.borrow_mut().drag_end() {
                // SAFETY: The closed hand cursor of the drag is on top of the cursor stack.
                unsafe {
                    let _: () = msg_send![class!(NSCursor), pop];
                }
            }
        }

        // Shows a crosshair while cropping or selecting, or an open hand over media that can
        // be dragged around.
        #[unsafe(method(resetCursorRects))]
        fn _reset_cursor_rects(&self) {
            let viewport = self.ivars().viewport.borrow();
            // SAFETY: The cursors are shared AppKit objects and the rectangle is the bounds.
            unsafe {
                let cursor: *mut Object = if viewport.is_marking() {
                    msg_send![class!(NSCursor), crosshairCursor]
                } else if viewport.is_scrollable() {
                    msg_send![class!(NSCursor), openHandCursor]
                } else {
                    return;
                };
                let bounds: Rect = msg_send![self, bounds];
                let _: () = msg_send![self, addCursorRect: bounds, cursor: cursor];
            }
        }

        #[unsafe(method(keyDown:))]
        fn _key_down(&self, event: *mut Object) {
            // SAFETY: AppKit passes a valid key NSEvent.
            let code: u16 = unsafe { msg_send![event, keyCode] };
            let step = match code {
                // Return applies the open tool and escape cancels it.
                KEY_RETURN | KEY_ENTER => return super::send(AppEvent::Action("tool.apply")),
                KEY_ESCAPE => return super::send(AppEvent::Action("tool.cancel")),
                KEY_UP => Step::LineBack,
                KEY_DOWN => Step::LineForward,
                KEY_PAGE_UP => Step::PageBack,
                KEY_PAGE_DOWN => Step::PageForward,
                KEY_HOME => Step::Start,
                KEY_END => Step::End,
                // SAFETY: NSView implements `keyDown:`, which passes the key on.
                _ => return unsafe { msg_send![super(self), keyDown: event] },
            };
            self.update(|viewport| viewport.step(Axis::Vertical, step));
        }

        // The Copy and Select All items of the Edit menu reach the view when it has focus,
        // and text fields of popovers when they have.
        #[unsafe(method(copy:))]
        fn _copy(&self, _: *mut Object) {
            super::send(AppEvent::Action("edit.copy"));
        }

        #[unsafe(method(selectAll:))]
        fn _select_all(&self, _: *mut Object) {
            super::send(AppEvent::Action("select.all"));
        }

        // Undo and Redo of the Edit menu undo edits, text fields of popovers undo their typing
        // themselves.
        // The items of the Edit menu that reach the view follow the state of their actions.
        #[unsafe(method(validateMenuItem:))]
        fn _validate_menu_item(&self, item: *mut Object) -> Bool {
            // SAFETY: AppKit passes a valid menu item.
            let selector: Sel = unsafe { msg_send![item, action] };
            let action = match selector {
                selector if selector == sel!(undo:) => "edit.undo",
                selector if selector == sel!(redo:) => "edit.redo",
                selector if selector == sel!(copy:) => "edit.copy",
                selector if selector == sel!(selectAll:) => "select.all",
                // NSView validates the other items of its responder chain when it implements
                // the validation, an action without it is enabled.
                _ => {
                    // SAFETY: The super call is only made when NSView implements it.
                    return unsafe {
                        let validates: Bool = msg_send![class!(NSView),
                            instancesRespondToSelector: sel!(validateMenuItem:)
                        ];
                        if validates.as_bool() {
                            msg_send![super(self), validateMenuItem: item]
                        } else {
                            Bool::YES
                        }
                    };
                }
            };
            objc_bool(bwindow::is_menu_item_enabled(action))
        }

        #[unsafe(method(undo:))]
        fn _undo(&self, _: *mut Object) {
            super::send(AppEvent::Action("edit.undo"));
        }

        #[unsafe(method(redo:))]
        fn _redo(&self, _: *mut Object) {
            super::send(AppEvent::Action("edit.redo"));
        }

        #[unsafe(method(advanceFrame))]
        fn _advance_frame(&self) {
            self.advance_frame();
        }
    }
);

impl View {
    /// Creates a view that shows the media zoomed to fit `frame`, or nothing while it loads.
    pub(super) fn new(frame: Rect, media: Option<Media>) -> Retained<Self> {
        // SAFETY: All objects are valid AppKit instances, the view retains the scroll view as
        // its subview and observes its clip view until `stop_observing`.
        unsafe {
            let document: Allocated<Document> = msg_send![Document::class(), alloc];
            let document: Retained<Object> = msg_send![document, initWithFrame: frame];
            let scroll_view: Allocated<ScrollView> = msg_send![ScrollView::class(), alloc];
            let scroll_view: Retained<Object> = msg_send![scroll_view, initWithFrame: frame];
            let _: () = msg_send![&*scroll_view, setDrawsBackground: Bool::NO];
            let _: () = msg_send![&*scroll_view, setHasHorizontalScroller: Bool::YES];
            let _: () = msg_send![&*scroll_view, setHasVerticalScroller: Bool::YES];
            let _: () = msg_send![&*scroll_view, setAutohidesScrollers: Bool::YES];
            let _: () = msg_send![&*scroll_view,
                setAutoresizingMask: NS_VIEW_WIDTH_SIZABLE | NS_VIEW_HEIGHT_SIZABLE
            ];
            let _: () = msg_send![&*scroll_view, setDocumentView: &*document];
            let clip: *mut Object = msg_send![&*scroll_view, contentView];
            let _: () = msg_send![clip, setDrawsBackground: Bool::NO];
            let _: () = msg_send![clip, setPostsBoundsChangedNotifications: Bool::YES];
            let _: () = msg_send![clip, setPostsFrameChangedNotifications: Bool::YES];

            let view: Allocated<Self> = msg_send![Self::class(), alloc];
            let view: Retained<Self> = msg_send![
                super(view.set_ivars(ViewIvars {
                    viewport: RefCell::new(Viewport::new(
                        media,
                        (frame.size.width, frame.size.height)
                    )),
                    dark: Cell::new(false),
                    scroll_view,
                    document,
                })),
                initWithFrame: frame
            ];
            let _: () = msg_send![&*view, addSubview: &*view.ivars().scroll_view];
            register_dragged_types(Retained::as_ptr(&view) as *mut Object);
            let center: *mut Object = msg_send![class!(NSNotificationCenter), defaultCenter];
            for name in [
                NSViewBoundsDidChangeNotification,
                NSViewFrameDidChangeNotification,
            ] {
                let _: () = msg_send![center,
                    addObserver: &*view,
                    selector: sel!(clipViewChanged:),
                    name: name,
                    object: clip
                ];
            }
            view
        }
    }

    /// The shown media.
    pub(super) fn media(&self) -> Media {
        self.ivars().viewport.borrow().media().clone()
    }

    /// Shows a crop rectangle over all of the media, or removes it.
    pub(super) fn set_cropping(&self, cropping: bool) {
        self.update(|viewport| viewport.set_cropping(cropping));
    }

    /// Shows a preview of any size in the place of the media, or the media again.
    pub(super) fn set_preview(&self, preview: Option<Media>) {
        self.update(|viewport| viewport.set_preview(preview));
    }

    /// Places the crop rectangle at a position with a size in pixels.
    pub(super) fn set_crop_rect(&self, rect: (u32, u32, u32, u32)) {
        self.update(|viewport| {
            if let Some(crop) = viewport.crop() {
                crop.set_rect(rect);
            }
        });
    }

    /// The longest side of the view in device pixels.
    pub(super) fn longest_pixels(&self) -> u32 {
        // SAFETY: The view is a live NSView, its window may not be there yet.
        unsafe {
            let bounds: Rect = msg_send![self, bounds];
            let window: *mut Object = msg_send![self, window];
            let scale: f64 = if window.is_null() {
                1.0
            } else {
                msg_send![window, backingScaleFactor]
            };
            (bounds.size.width.max(bounds.size.height) * scale).ceil() as u32
        }
    }

    /// Makes the crop rectangle keep a width divided by height.
    pub(super) fn set_crop_aspect(&self, aspect: Option<f64>) {
        self.update(|viewport| {
            if let Some(crop) = viewport.crop() {
                crop.set_aspect(aspect);
            }
        });
    }

    /// The crop rectangle in pixels, or all of the media without one.
    pub(super) fn crop_rect(&self) -> (u32, u32, u32, u32) {
        let mut viewport = self.ivars().viewport.borrow_mut();
        let (width, height) = viewport.media().size();
        viewport
            .crop()
            .map_or((0, 0, width as u32, height as u32), |crop| crop.rect())
    }

    /// Makes drags draw selections of a shape, or pan the media.
    pub(super) fn set_selecting(&self, shape: Option<Shape>) {
        self.update(|viewport| viewport.set_selecting(shape));
    }

    /// The selected part of the media.
    pub(super) fn selection(&self) -> Option<Selection> {
        self.ivars().viewport.borrow().selection()
    }

    /// Selects everything outside the selection instead.
    pub(super) fn invert_selection(&self) {
        self.update(Viewport::invert_selection);
    }

    /// Follows the light or dark theme.
    pub(super) fn set_dark(&self, dark: bool) {
        self.ivars().dark.set(dark);
        self.update(|_| {});
    }

    // The visible part of the document.
    fn clip_bounds(&self) -> Rect {
        // SAFETY: The scroll view keeps its clip view.
        unsafe {
            let clip: *mut Object = msg_send![&*self.ivars().scroll_view, contentView];
            msg_send![clip, bounds]
        }
    }

    /// Changes the viewport, then scrolls the scroll view along and redraws.
    pub(super) fn update(&self, change: impl FnOnce(&mut Viewport)) {
        let ivars = self.ivars();
        let visible = self.clip_bounds().size;
        let bars = {
            let mut viewport = ivars.viewport.borrow_mut();
            viewport.resize((visible.width, visible.height));
            change(&mut viewport);
            Axis::BOTH.map(|axis| viewport.scroll_bar(axis))
        };
        // SAFETY: The scroll view keeps its clip and document views, scrolling posts the bounds
        // change that the viewport follows.
        unsafe {
            let size = Size {
                width: bars[0].length,
                height: bars[1].length,
            };
            // Resizing the document can show or hide legacy scrollers, which resizes the
            // viewport again, so the scroll position is read after it.
            let _: () = msg_send![&*ivars.document, setFrameSize: size];
            let bars = Axis::BOTH.map(|axis| ivars.viewport.borrow().scroll_bar(axis));
            let clip: *mut Object = msg_send![&*ivars.scroll_view, contentView];
            let origin = Point {
                x: bars[0].position,
                y: bars[1].position,
            };
            let _: () = msg_send![clip, scrollToPoint: origin];
            let _: () = msg_send![&*ivars.scroll_view, reflectScrolledClipView: clip];
            let _: () = msg_send![self, setNeedsDisplay: Bool::YES];
            let window: *mut Object = msg_send![self, window];
            if !window.is_null() {
                let _: () = msg_send![window, invalidateCursorRectsForView: self];
            }
        }
    }

    fn draw(&self) {
        // SAFETY: Drawing occurs during AppKit's draw pass, whose flipped graphics context
        // stays valid for this call.
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
            let bounds: Rect = msg_send![self, bounds];
            let window: *mut Object = msg_send![self, window];
            let scale: f64 = if window.is_null() {
                1.0
            } else {
                msg_send![window, backingScaleFactor]
            };
            let ivars = self.ivars();
            bcanvas::draw_native(
                NativeContext::CoreGraphics(context),
                bounds.size.width as f32,
                bounds.size.height as f32,
                scale as f32,
                |canvas| ivars.viewport.borrow().paint(canvas, ivars.dark.get()),
            );
        }
    }

    // The position of a mouse event in the view.
    fn pointer(&self, event: *mut Object) -> (f64, f64) {
        // SAFETY: AppKit passes a valid NSEvent and the view converts from window coordinates.
        let point: Point = unsafe {
            let location: Point = msg_send![event, locationInWindow];
            msg_send![self, convertPoint: location, fromView: null_mut::<Object>()]
        };
        (point.x, point.y)
    }

    /// Stops following the scroll view and cancels the scheduled animation frame, before the
    /// view is released.
    pub(super) fn stop(&self) {
        // SAFETY: Removing only affects this view's own observations and pending requests.
        unsafe {
            let center: *mut Object = msg_send![class!(NSNotificationCenter), defaultCenter];
            let _: () = msg_send![center, removeObserver: self];
            let _: () = msg_send![class!(NSObject),
                cancelPreviousPerformRequestsWithTarget: self,
                selector: sel!(advanceFrame),
                object: null_mut::<Object>()
            ];
        }
    }

    /// Shows the current animation frame and schedules the next one.
    pub(super) fn advance_frame(&self) {
        let (changed, next) = self.ivars().viewport.borrow_mut().advance_frame();
        // SAFETY: Scheduling retains the view until the selector runs or is canceled.
        unsafe {
            let _: () = msg_send![class!(NSObject),
                cancelPreviousPerformRequestsWithTarget: self,
                selector: sel!(advanceFrame),
                object: null_mut::<Object>()
            ];
            if changed {
                let _: () = msg_send![self, setNeedsDisplay: Bool::YES];
            }
            if let Some(next) = next {
                let _: () = msg_send![self,
                    performSelector: sel!(advanceFrame),
                    withObject: null_mut::<Object>(),
                    afterDelay: next.as_secs_f64()
                ];
            }
        }
    }
}

pub(super) const fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect {
        origin: Point { x, y },
        size: Size { width, height },
    }
}

#[cfg(test)]
mod tests {
    use imageview_core::appkit::ns_string;
    use imageview_core::decode_media;
    use objc2::rc::autoreleasepool;

    use super::*;

    #[test]
    fn stopping_an_animated_view_releases_its_timer_reference() {
        let media = decode_media(include_bytes!(
            "../../../../imageview-core/tests/fixtures/animated.gif"
        ))
        .expect("valid animation");
        autoreleasepool(|_| {
            let view = View::new(rect(0.0, 0.0, 320.0, 240.0), Some(media));
            // SAFETY: The test retains the view during these ownership checks.
            let before: usize = unsafe { msg_send![&*view, retainCount] };
            view.advance_frame();
            // SAFETY: The view remains live with its pending timer.
            let scheduled: usize = unsafe { msg_send![&*view, retainCount] };
            view.stop();
            // SAFETY: The Rust owner keeps the view live after cancellation.
            let stopped: usize = unsafe { msg_send![&*view, retainCount] };
            assert!(scheduled > before, "timer did not retain its target");
            assert_eq!(stopped, before, "timer retained its canceled target");
            drop(view);
        });
    }

    #[test]
    fn media_view_accepts_file_drops() {
        // SAFETY: The view stays retained during the test and uses AppKit selector signatures.
        autoreleasepool(|_| unsafe {
            let view = View::new(rect(0.0, 0.0, 320.0, 240.0), None);
            let types: *mut Object = msg_send![&*view, registeredDraggedTypes];
            let accepts: Bool = msg_send![types, containsObject:ns_string("public.file-url")];
            assert!(accepts.as_bool());
            let handles: Bool = msg_send![&*view, respondsToSelector:sel!(performDragOperation:)];
            assert!(handles.as_bool());
            view.stop();
        });
    }
}
