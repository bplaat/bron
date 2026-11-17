/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The AppKit scroll view: a flipped view that draws the page with bcanvas, below a native
//! NSScrollView that only shows its scroller. The scroll view scrolls an empty document as tall
//! as the page, the view draws the page at its scroll position.

use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::null_mut;

use bcanvas::{CanvasRenderingContext2d, NativeContext};
use bwindow::{
    LogicalPoint, Modifiers, MouseButton, MouseEvent, NativeViewHandle, Window, WindowAttachment,
    WindowEvent, WindowEventSender,
};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{Encode, Encoding, class, define_class, msg_send, sel};

use super::LINE;

// The tracking area reports moves and exits in the visible rect while the window is key.
const TRACKING_OPTIONS: u64 = (1 << 0) | (1 << 1) | (1 << 5) | (1 << 9) | (1 << 10);
const NS_VIEW_WIDTH_SIZABLE: u64 = 2;
const NS_VIEW_HEIGHT_SIZABLE: u64 = 16;
const NS_SCROLL_ELASTICITY_NONE: i64 = 1;
const KEY_HOME: u16 = 115;
const KEY_PAGE_UP: u16 = 116;
const KEY_END: u16 = 119;
const KEY_PAGE_DOWN: u16 = 121;
const KEY_DOWN: u16 = 125;
const KEY_UP: u16 = 126;

#[derive(Clone, Copy)]
#[repr(C)]
struct Point {
    x: f64,
    y: f64,
}

// SAFETY: The layout matches CGPoint.
unsafe impl Encode for Point {
    const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
}

#[derive(Clone, Copy)]
#[repr(C)]
struct Size {
    width: f64,
    height: f64,
}

// SAFETY: The layout matches CGSize.
unsafe impl Encode for Size {
    const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
}

#[derive(Clone, Copy)]
#[repr(C)]
struct Rect {
    origin: Point,
    size: Size,
}

// SAFETY: The layout matches CGRect.
unsafe impl Encode for Rect {
    const ENCODING: Encoding = Encoding::Struct("CGRect", &[Point::ENCODING, Size::ENCODING]);
}

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {
    static NSViewBoundsDidChangeNotification: *mut Object;
    static NSViewFrameDidChangeNotification: *mut Object;
}

define_class!(
    // Leaves everything but its scroller to the page view below it.
    #[unsafe(super(NSScrollView))]
    #[name = "BastiaanScrollView"]
    struct Scroller;

    impl Scroller {
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
    #[name = "BastiaanScrollDocument"]
    struct Document;

    impl Document {
        #[unsafe(method(isFlipped))]
        const fn _is_flipped(&self) -> Bool {
            Bool::YES
        }
    }
);

struct PageIvars {
    sender: WindowEventSender,
    scroll_view: Retained<Object>,
    document: Retained<Object>,
    content_height: Cell<f64>,
    // The Core Graphics context while the view draws itself.
    context: Cell<*mut c_void>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[name = "BastiaanPageView"]
    #[ivars = PageIvars]
    struct PageView;

    impl PageView {
        #[unsafe(method(isFlipped))]
        const fn _is_flipped(&self) -> Bool {
            Bool::YES
        }

        #[unsafe(method(acceptsFirstResponder))]
        const fn _accepts_first_responder(&self) -> Bool {
            Bool::YES
        }

        // Draws through the window's RedrawRequested, or again later when the event loop is
        // busy.
        #[unsafe(method(drawRect:))]
        fn _draw_rect(&self, _: Rect) {
            // SAFETY: AppKit has a current graphics context while a view draws.
            let context: *mut c_void = unsafe {
                let graphics: *mut Object = msg_send![class!(NSGraphicsContext), currentContext];
                msg_send![graphics, CGContext]
            };
            self.ivars().context.set(context);
            let drawn = self.ivars().sender.redraw(|| {});
            self.ivars().context.set(null_mut());
            if !drawn {
                // SAFETY: The view is alive and implements `redraw`.
                unsafe {
                    let _: () = msg_send![self,
                        performSelector: sel!(redraw),
                        withObject: null_mut::<Object>(),
                        afterDelay: 1.0 / 60.0
                    ];
                }
            }
        }

        #[unsafe(method(redraw))]
        fn _redraw(&self) {
            // SAFETY: The view is alive.
            unsafe {
                let _: () = msg_send![self, setNeedsDisplay: Bool::YES];
            }
        }

        // The scroll view scrolled or resized.
        #[unsafe(method(clipViewChanged:))]
        fn _clip_view_changed(&self, _: *mut Object) {
            self.update_document();
            self._redraw();
        }

        #[unsafe(method(setFrameSize:))]
        fn _set_frame_size(&self, size: Size) {
            // SAFETY: NSView implements `setFrameSize:` with this argument type.
            unsafe {
                let _: () = msg_send![super(self), setFrameSize: size];
            }
            self.update_document();
        }

        #[unsafe(method(scrollWheel:))]
        fn _scroll_wheel(&self, event: *mut Object) {
            // SAFETY: The scroll view is live and implements `scrollWheel:`.
            unsafe {
                let _: () = msg_send![&*self.ivars().scroll_view, scrollWheel: event];
            }
        }

        #[unsafe(method(mouseDown:))]
        fn _mouse_down(&self, event: *mut Object) {
            // SAFETY: The view is in a window while it gets events.
            unsafe {
                let window: *mut Object = msg_send![self, window];
                let _: Bool = msg_send![window, makeFirstResponder: self];
            }
            let event = self.mouse_event(event);
            self.ivars().sender.send(WindowEvent::MouseDown(event));
        }

        #[unsafe(method(mouseUp:))]
        fn _mouse_up(&self, event: *mut Object) {
            let event = self.mouse_event(event);
            self.ivars().sender.send(WindowEvent::MouseUp(event));
        }

        #[unsafe(method(mouseMoved:))]
        fn _mouse_moved(&self, event: *mut Object) {
            self.mouse_move(event);
        }

        #[unsafe(method(mouseDragged:))]
        fn _mouse_dragged(&self, event: *mut Object) {
            self.mouse_move(event);
        }

        #[unsafe(method(mouseExited:))]
        fn _mouse_exited(&self, _: *mut Object) {
            self.ivars().sender.send(WindowEvent::MouseLeave);
        }

        #[unsafe(method(keyDown:))]
        fn _key_down(&self, event: *mut Object) {
            // SAFETY: AppKit passes a valid key NSEvent.
            let code: u16 = unsafe { msg_send![event, keyCode] };
            let (scroll, page) = (self.scroll(), self.clip_bounds().size.height);
            let scroll = match code {
                KEY_UP => scroll - f64::from(LINE),
                KEY_DOWN => scroll + f64::from(LINE),
                KEY_PAGE_UP => scroll - page,
                KEY_PAGE_DOWN => scroll + page,
                KEY_HOME => 0.0,
                KEY_END => f64::MAX,
                // SAFETY: NSView implements `keyDown:`, which passes the key on.
                _ => return unsafe { msg_send![super(self), keyDown: event] },
            };
            self.scroll_to(scroll);
        }
    }
);

impl PageView {
    fn clip(&self) -> *mut Object {
        // SAFETY: The scroll view keeps its clip view.
        unsafe { msg_send![&*self.ivars().scroll_view, contentView] }
    }

    fn clip_bounds(&self) -> Rect {
        // SAFETY: The clip view is alive.
        unsafe { msg_send![self.clip(), bounds] }
    }

    fn scroll(&self) -> f64 {
        self.clip_bounds().origin.y.max(0.0)
    }

    fn scroll_to(&self, scroll: f64) {
        let bounds = self.clip_bounds();
        let max = (self.ivars().content_height.get() - bounds.size.height).max(0.0);
        let point = Point {
            x: 0.0,
            y: scroll.clamp(0.0, max),
        };
        // SAFETY: The clip view scrolls the document, the scroll view follows it.
        unsafe {
            let _: () = msg_send![self.clip(), scrollToPoint: point];
            let _: () = msg_send![&*self.ivars().scroll_view, reflectScrolledClipView: self.clip()];
        }
    }

    // Sizes the document to the page, at least as tall as the visible area.
    fn update_document(&self) {
        let bounds = self.clip_bounds();
        let size = Size {
            width: bounds.size.width,
            height: self.ivars().content_height.get().max(bounds.size.height),
        };
        // SAFETY: The document is alive, an unchanged size posts no notification.
        unsafe {
            let frame: Rect = msg_send![&*self.ivars().document, frame];
            if frame.size.width != size.width || frame.size.height != size.height {
                let _: () = msg_send![&*self.ivars().document, setFrameSize: size];
            }
        }
    }

    fn position(&self, event: *mut Object) -> LogicalPoint {
        // SAFETY: AppKit passes a valid mouse NSEvent of the view's window.
        let point: Point = unsafe {
            let point: Point = msg_send![event, locationInWindow];
            msg_send![self, convertPoint: point, fromView: null_mut::<Object>()]
        };
        LogicalPoint::new(point.x as f32, point.y as f32)
    }

    fn mouse_event(&self, event: *mut Object) -> MouseEvent {
        MouseEvent {
            button: MouseButton::Left,
            position: self.position(event),
            modifiers: Modifiers::NONE,
        }
    }

    fn mouse_move(&self, event: *mut Object) {
        self.ivars().sender.send(WindowEvent::MouseMove {
            position: self.position(event),
            movement: LogicalPoint::new(0.0, 0.0),
            modifiers: Modifiers::NONE,
        });
    }
}

/// A native view that scrolls a page vertically.
pub(crate) struct ScrollView {
    view: Retained<PageView>,
    attachment: WindowAttachment,
}

impl ScrollView {
    /// Fills the window with an empty page.
    pub(crate) fn new(window: &Window) -> Self {
        let attachment = window.attach_content().expect("window has no content yet");
        let size = window.size();
        let frame = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: f64::from(size.width),
                height: f64::from(size.height),
            },
        };
        // SAFETY: All objects are valid AppKit instances, the view retains the scroll view as
        // its subview and observes its clip view until it drops.
        let view = unsafe {
            let document: Allocated<Document> = msg_send![Document::class(), alloc];
            let document: Retained<Object> = msg_send![document, initWithFrame: frame];
            let scroll_view: Allocated<Scroller> = msg_send![Scroller::class(), alloc];
            let scroll_view: Retained<Object> = msg_send![scroll_view, initWithFrame: frame];
            let _: () = msg_send![&*scroll_view, setDrawsBackground: Bool::NO];
            let _: () = msg_send![&*scroll_view, setHasVerticalScroller: Bool::YES];
            let _: () = msg_send![&*scroll_view, setAutohidesScrollers: Bool::YES];
            let _: () = msg_send![&*scroll_view,
                setVerticalScrollElasticity: NS_SCROLL_ELASTICITY_NONE
            ];
            let _: () = msg_send![&*scroll_view,
                setAutoresizingMask: NS_VIEW_WIDTH_SIZABLE | NS_VIEW_HEIGHT_SIZABLE
            ];
            let _: () = msg_send![&*scroll_view, setDocumentView: &*document];
            let clip: *mut Object = msg_send![&*scroll_view, contentView];
            let _: () = msg_send![clip, setDrawsBackground: Bool::NO];
            let _: () = msg_send![clip, setPostsBoundsChangedNotifications: Bool::YES];
            let _: () = msg_send![clip, setPostsFrameChangedNotifications: Bool::YES];

            let view: Allocated<PageView> = msg_send![PageView::class(), alloc];
            let view: Retained<PageView> = msg_send![
                super(view.set_ivars(PageIvars {
                    sender: attachment.event_sender(),
                    scroll_view,
                    document,
                    content_height: Cell::new(0.0),
                    context: Cell::new(null_mut()),
                })),
                initWithFrame: frame
            ];
            let _: () = msg_send![&*view, addSubview: &*view.ivars().scroll_view];
            let tracking: Allocated<Object> = msg_send![class!(NSTrackingArea), alloc];
            let tracking: Retained<Object> = msg_send![tracking,
                initWithRect: frame,
                options: TRACKING_OPTIONS,
                owner: &*view,
                userInfo: null_mut::<Object>()
            ];
            let _: () = msg_send![&*view, addTrackingArea: &*tracking];
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
            attachment.set_native_view(NativeViewHandle::AppKit(Retained::as_ptr(&view).cast()));
            let window: *mut Object = msg_send![&*view, window];
            let _: Bool = msg_send![window, makeFirstResponder: &*view];
            view
        };
        // Redraws, also the animation frames, paint through the view
        let redraw_view = view.clone();
        attachment.on_redraw(move || redraw_view._redraw());
        Self { view, attachment }
    }

    /// Paints the visible part of the page during the window's `RedrawRequested`, returns
    /// whether it painted.
    pub(crate) fn draw(&self, draw: impl FnOnce(&mut CanvasRenderingContext2d<'_>)) -> bool {
        let context = self.view.ivars().context.get();
        if context.is_null() {
            return false;
        }
        // SAFETY: The view is drawing into the context, it is flipped like the canvas.
        unsafe {
            let bounds: Rect = msg_send![&*self.view, bounds];
            let window: *mut Object = msg_send![&*self.view, window];
            let scale: f64 = msg_send![window, backingScaleFactor];
            bcanvas::draw_native(
                NativeContext::CoreGraphics(context),
                bounds.size.width as f32,
                bounds.size.height as f32,
                scale as f32,
                draw,
            );
        }
        true
    }

    /// The scroll position in logical pixels.
    pub(crate) fn scroll(&self) -> f32 {
        self.view.scroll() as f32
    }

    /// Sets the height of the page, which the scroll bar scrolls through.
    pub(crate) fn set_content_height(&self, height: f32) {
        self.view.ivars().content_height.set(f64::from(height));
        self.view.update_document();
    }

    /// Paints the page again.
    pub(crate) fn request_redraw(&self) {
        self.view._redraw();
    }

    /// Paints the page again at the next display frame.
    pub(crate) fn request_animation_frame(&self) {
        self.attachment.request_animation_frame();
    }

    /// Shows a pointing hand over the page instead of the arrow.
    pub(crate) fn set_pointer_cursor(&self, pointer: bool) {
        // SAFETY: The cursors are shared AppKit objects.
        unsafe {
            let cursor: *mut Object = if pointer {
                msg_send![class!(NSCursor), pointingHandCursor]
            } else {
                msg_send![class!(NSCursor), arrowCursor]
            };
            let _: () = msg_send![cursor, set];
        }
    }

    /// The scroller follows the appearance of the window by itself.
    pub(crate) const fn set_dark(&self, _: bool) {}
}

impl Drop for ScrollView {
    fn drop(&mut self) {
        // SAFETY: The view stops observing the clip view before it can be freed.
        unsafe {
            let center: *mut Object = msg_send![class!(NSNotificationCenter), defaultCenter];
            let _: () = msg_send![center, removeObserver: &*self.view];
        }
    }
}
