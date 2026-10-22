/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The shared content view of a window: a flipped, layer-backed NSView filling the content
//! area, which renderers add their layers to. It delivers the pointer, scroll, keyboard and
//! cursor interaction of the content. Window children like a web view receive their own
//! input, so the view only reports events that target it and never delivers them twice.

use std::cell::Cell;
use std::ptr::null_mut;

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, define_class, msg_send};

use super::headers::*;
use super::input::{macos_key_event, macos_pointer_event, macos_scroll_event};
use crate::{ButtonState, CursorIcon, WindowEvent, WindowEventSender};

// Mouse entered and exited, mouse moved, active in key window, in visible rect and enabled
// during mouse drag.
const TRACKING_OPTIONS: u64 = (1 << 0) | (1 << 1) | (1 << 5) | (1 << 9) | (1 << 10);

pub(super) struct ContentViewIvars {
    sender: WindowEventSender,
    cursor: Cell<CursorIcon>,
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
                self.ivars().sender.send(unsafe { macos_scroll_event(event) });
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
