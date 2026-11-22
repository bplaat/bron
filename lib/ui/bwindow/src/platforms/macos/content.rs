/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The shared content view of a window: a flipped, layer-backed NSView filling the content
//! area, with a layer view for each window layer that renderers add their layers to. It delivers the pointer, scroll, keyboard and
//! cursor interaction of the content. Window children like a web view receive their own
//! input, so the view only reports events that target it and never delivers them twice.

use std::cell::{Cell, RefCell};
use std::ptr::null_mut;

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, define_class, msg_send};

use super::headers::*;
use super::input::{macos_key_event, macos_pointer_event, macos_scroll_event};
use super::scroll::Scroller;
use crate::{
    ButtonState, CursorIcon, LogicalRect, NativeContentHandle, NativeViewHandle,
    NativeWindowHandle, WindowEvent, WindowEventSender,
};

// Mouse entered and exited, mouse moved, active in key window, in visible rect and enabled
// during mouse drag.
const TRACKING_OPTIONS: u64 = (1 << 0) | (1 << 1) | (1 << 5) | (1 << 9) | (1 << 10);

pub(super) struct ContentViewIvars {
    sender: WindowEventSender,
    cursor: Cell<CursorIcon>,
    // The native scroller over the content, which scrolls with the wheel.
    scroller: RefCell<Option<Retained<Scroller>>>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[name = "BwindowContentView"]
    #[ivars = ContentViewIvars]
    pub(super) struct ContentView;

    impl ContentView {
        #[unsafe(method(isFlipped))]
        const fn _is_flipped(&self) -> Bool { Bool::YES }

        #[unsafe(method(acceptsFirstResponder))]
        const fn _accepts_first_responder(&self) -> Bool { Bool::YES }

        #[unsafe(method(resetCursorRects))]
        fn _reset_cursor_rects(&self) {
            let cursor: *mut Object = unsafe {
                match self.ivars().cursor.get() {
                    CursorIcon::Pointer => msg_send![class!(NSCursor), pointingHandCursor],
                    // AppKit has no public busy cursor.
                    CursorIcon::Default | CursorIcon::Progress => msg_send![class!(NSCursor), arrowCursor],
                }
            };
            let bounds: NSRect = unsafe { msg_send![self, bounds] };
            let _: () = unsafe { msg_send![self, addCursorRect:bounds, cursor:cursor] };
        }

        #[unsafe(method(mouseDown:))]
        fn _mouse_down(&self, event: *mut Object) {
            let window: *mut Object = unsafe { msg_send![self, window] };
            let _: Bool = unsafe { msg_send![window, makeFirstResponder:self] };
            self.pointer(event, Some(ButtonState::Pressed), true);
        }
        #[unsafe(method(rightMouseDown:))]
        fn _right_mouse_down(&self, event: *mut Object) { self.pointer(event, Some(ButtonState::Pressed), true); }
        #[unsafe(method(otherMouseDown:))]
        fn _other_mouse_down(&self, event: *mut Object) { self.pointer(event, Some(ButtonState::Pressed), true); }
        #[unsafe(method(mouseMoved:))]
        fn _mouse_moved(&self, event: *mut Object) { self.pointer(event, None, true); }
        // Releases and drags go to the view that got the press, also outside of it.
        #[unsafe(method(mouseUp:))]
        fn _mouse_up(&self, event: *mut Object) { self.pointer(event, Some(ButtonState::Released), false); }
        #[unsafe(method(rightMouseUp:))]
        fn _right_mouse_up(&self, event: *mut Object) { self.pointer(event, Some(ButtonState::Released), false); }
        #[unsafe(method(otherMouseUp:))]
        fn _other_mouse_up(&self, event: *mut Object) { self.pointer(event, Some(ButtonState::Released), false); }
        #[unsafe(method(mouseDragged:))]
        fn _mouse_dragged(&self, event: *mut Object) { self.pointer(event, None, false); }
        #[unsafe(method(rightMouseDragged:))]
        fn _right_mouse_dragged(&self, event: *mut Object) { self.pointer(event, None, false); }
        #[unsafe(method(otherMouseDragged:))]
        fn _other_mouse_dragged(&self, event: *mut Object) { self.pointer(event, None, false); }

        #[unsafe(method(mouseExited:))]
        fn _mouse_exited(&self, _: *mut Object) {
            self.ivars().sender.send(WindowEvent::MouseLeave);
        }

        #[unsafe(method(scrollWheel:))]
        fn _scroll_wheel(&self, event: *mut Object) {
            if self.is_hit(event) {
                match &*self.ivars().scroller.borrow() {
                    Some(scroller) => unsafe {
                        let _: () = msg_send![&**scroller, scrollWheel: event];
                    },
                    None => self.ivars().sender.send(unsafe { macos_scroll_event(event) }),
                }
            }
        }

        #[unsafe(method(keyDown:))]
        fn _key_down(&self, event: *mut Object) {
            if self.is_first_responder() {
                self.ivars().sender.send(WindowEvent::KeyDown(unsafe {
                    macos_key_event(event, ButtonState::Pressed)
                }));
            }
        }

        #[unsafe(method(keyUp:))]
        fn _key_up(&self, event: *mut Object) {
            if self.is_first_responder() {
                self.ivars().sender.send(WindowEvent::KeyUp(unsafe {
                    macos_key_event(event, ButtonState::Released)
                }));
            }
        }
    }
);

impl ContentView {
    pub(super) fn new(frame: NSRect, sender: WindowEventSender) -> Retained<Self> {
        unsafe {
            let view: Allocated<Self> = msg_send![Self::class(), alloc];
            let view: Retained<Self> = msg_send![
                super(view.set_ivars(ContentViewIvars {
                    sender,
                    cursor: Cell::new(CursorIcon::Default),
                    scroller: RefCell::new(None),
                })),
                initWithFrame:frame
            ];
            // Renderers add their layers to the backing layer.
            let _: () = msg_send![&*view, setWantsLayer:Bool::YES];
            let tracking: Allocated<Object> = msg_send![class!(NSTrackingArea), alloc];
            let tracking: Retained<Object> = msg_send![tracking,
                initWithRect:frame, options:TRACKING_OPTIONS,
                owner:&*view, userInfo:null_mut::<Object>()];
            let _: () = msg_send![&*view, addTrackingArea:&*tracking];
            view
        }
    }

    /// Scroll the content natively for content of a size, or stop with none.
    pub(super) fn set_scroll_size(&self, size: Option<NSSize>) {
        let mut scroller = self.ivars().scroller.borrow_mut();
        match size {
            Some(size) => scroller
                .get_or_insert_with(|| {
                    let view = self as *const Self as *mut Object;
                    Scroller::new(view, self.ivars().sender.clone())
                })
                .set_size(size),
            None => {
                if let Some(scroller) = scroller.take() {
                    scroller.remove();
                }
            }
        }
    }

    pub(super) fn scroll_offset(&self) -> NSPoint {
        self.ivars()
            .scroller
            .borrow()
            .as_ref()
            .map_or(NSPoint { x: 0.0, y: 0.0 }, |scroller| scroller.offset())
    }

    pub(super) fn set_scroll_offset(&self, offset: NSPoint) {
        if let Some(scroller) = &*self.ivars().scroller.borrow() {
            scroller.scroll_to(offset);
        }
    }

    /// Show `cursor` over the content.
    pub(super) fn set_cursor(&self, cursor: CursorIcon) {
        if self.ivars().cursor.replace(cursor) != cursor {
            let window: *mut Object = unsafe { msg_send![self, window] };
            let _: () = unsafe { msg_send![window, invalidateCursorRectsForView:self] };
        }
    }

    fn pointer(&self, event: *mut Object, state: Option<ButtonState>, hit: bool) {
        if !hit || self.is_hit(event) {
            let view = self as *const Self as *mut Object;
            self.ivars()
                .sender
                .send(unsafe { macos_pointer_event(view, event, state) });
        }
    }

    // Whether the event is over this view itself and not a child, which passed it on.
    fn is_hit(&self, event: *mut Object) -> bool {
        unsafe {
            // Hit testing takes a point in the coordinates of the superview.
            let point: NSPoint = msg_send![event, locationInWindow];
            let superview: *mut Object = msg_send![self, superview];
            let point: NSPoint = if superview.is_null() {
                point
            } else {
                msg_send![superview, convertPoint:point, fromView:null_mut::<Object>()]
            };
            let hit: *mut Object = msg_send![self, hitTest:point];
            std::ptr::eq(hit, self as *const Self as *const Object)
        }
    }

    fn is_first_responder(&self) -> bool {
        let window: *mut Object = unsafe { msg_send![self, window] };
        let responder: *mut Object = unsafe { msg_send![window, firstResponder] };
        std::ptr::eq(responder, self as *const Self as *const Object)
    }
}

/// Fill the content view with a native view below a titlebar drag view, it becomes the
/// first responder of the window.
pub(crate) unsafe fn embed_native_view(
    window: NativeWindowHandle,
    content: Option<NativeContentHandle>,
    view: NativeViewHandle,
) {
    let (
        NativeWindowHandle::AppKit(window),
        Some(NativeContentHandle::AppKit(content)),
        NativeViewHandle::AppKit(view),
    ) = (window, content, view)
    else {
        panic!("invalid native view handle");
    };
    let (window, content, view) = (
        window.cast::<Object>(),
        content.cast::<Object>(),
        view.cast::<Object>(),
    );
    unsafe {
        let bounds: NSRect = msg_send![content, bounds];
        let _: () = msg_send![view, setFrame:bounds];
        let _: () =
            msg_send![view, setAutoresizingMask:NS_VIEW_WIDTH_SIZABLE | NS_VIEW_HEIGHT_SIZABLE];
        let _: () = msg_send![content, addSubview:view, positioned:NS_WINDOW_BELOW, relativeTo:null_mut::<Object>()];
        let _: Bool = msg_send![window, makeFirstResponder:view];
    }
}

define_class!(
    // Hosts the layers of one window layer, flipped like the content view, which gets all
    // of its input.
    #[unsafe(super(NSView))]
    #[name = "BwindowLayerView"]
    struct LayerView;

    impl LayerView {
        #[unsafe(method(isFlipped))]
        const fn _is_flipped(&self) -> Bool { Bool::YES }

        #[unsafe(method(hitTest:))]
        const fn _hit_test(&self, _: NSPoint) -> *mut Object { null_mut() }
    }
);

fn view(handle: NativeContentHandle) -> *mut Object {
    let NativeContentHandle::AppKit(view) = handle else {
        panic!("expected an AppKit view");
    };
    view.cast()
}

/// Add a layer view to the content view, above or below a neighboring layer view, or else
/// below every other child like a web view. The returned view is owned by the caller.
pub(crate) unsafe fn add_layer_view(
    content: NativeContentHandle,
    neighbor: Option<(NativeContentHandle, bool)>,
) -> NativeContentHandle {
    let content = view(content);
    unsafe {
        let bounds: NSRect = msg_send![content, bounds];
        let layer_view: Allocated<LayerView> = msg_send![LayerView::class(), alloc];
        let layer_view: Retained<LayerView> = msg_send![layer_view, initWithFrame:bounds];
        let _: () = msg_send![&*layer_view, setWantsLayer:Bool::YES];
        let (position, relative) = match neighbor {
            Some((neighbor, true)) => (NS_WINDOW_ABOVE, view(neighbor)),
            Some((neighbor, false)) => (NS_WINDOW_BELOW, view(neighbor)),
            None => (NS_WINDOW_BELOW, null_mut()),
        };
        let _: () =
            msg_send![content, addSubview:&*layer_view, positioned:position, relativeTo:relative];
        NativeContentHandle::AppKit(Retained::into_raw(layer_view).cast())
    }
}

/// Whether layers get native views, which every window composites.
pub(crate) const unsafe fn native_layers(_: NativeContentHandle) -> bool {
    true
}

/// Place a layer view at an area of the content view, or fill it.
pub(crate) unsafe fn set_layer_view_frame(
    _: NativeContentHandle,
    layer_view: NativeContentHandle,
    frame: Option<LogicalRect>,
) {
    let layer_view = view(layer_view);
    unsafe {
        let (frame, mask) = match frame {
            Some(frame) => (
                NSRect {
                    origin: NSPoint {
                        x: frame.x.into(),
                        y: frame.y.into(),
                    },
                    size: NSSize {
                        width: frame.width.into(),
                        height: frame.height.into(),
                    },
                },
                0,
            ),
            None => {
                let superview: *mut Object = msg_send![layer_view, superview];
                (
                    msg_send![superview, bounds],
                    NS_VIEW_WIDTH_SIZABLE | NS_VIEW_HEIGHT_SIZABLE,
                )
            }
        };
        let _: () = msg_send![layer_view, setAutoresizingMask:mask];
        let _: () = msg_send![layer_view, setFrame:frame];
    }
}

pub(crate) unsafe fn set_layer_view_hidden(layer_view: NativeContentHandle, hidden: bool) {
    let _: () = unsafe { msg_send![view(layer_view), setHidden:hidden] };
}

/// Remove a layer view with the layers of its renderer and release it.
pub(crate) unsafe fn remove_layer_view(layer_view: NativeContentHandle) {
    let layer_view = view(layer_view);
    unsafe {
        let _: () = msg_send![layer_view, removeFromSuperview];
        drop(Retained::from_raw(layer_view));
    }
}
