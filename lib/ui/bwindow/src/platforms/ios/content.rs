/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The shared content view of a window: the root UIView of its view controller, which
//! renderers add their layers to. It delivers native multitouch events with compatibility mouse input.
//! Window children like a web view receive their own touches, so the view only reports
//! touches that start on it and never delivers them twice.

use objc2::rc::{Allocated, Retained};
use objc2::runtime::AnyObject as Object;
use objc2::{define_class, msg_send};

use super::input::ios_touches;
use crate::{TouchEvent, WindowEvent, WindowEventSender};

define_class!(
    #[unsafe(super(UIView))]
    #[name = "BwindowContentView"]
    #[ivars = WindowEventSender]
    pub(super) struct ContentView;

    impl ContentView {
        #[unsafe(method(touchesBegan:withEvent:))]
        fn _touches_began(&self, touches: *mut Object, _: *mut Object) {
            self.touch(touches, WindowEvent::TouchStart);
        }

        #[unsafe(method(touchesMoved:withEvent:))]
        fn _touches_moved(&self, touches: *mut Object, _: *mut Object) {
            self.touch(touches, WindowEvent::TouchMove);
        }

        #[unsafe(method(touchesEnded:withEvent:))]
        fn _touches_ended(&self, touches: *mut Object, _: *mut Object) {
            self.touch(touches, WindowEvent::TouchEnd);
        }

        #[unsafe(method(touchesCancelled:withEvent:))]
        fn _touches_cancelled(&self, touches: *mut Object, _: *mut Object) {
            self.touch(touches, WindowEvent::TouchCancel);
        }
    }
);

impl ContentView {
    pub(super) fn new(sender: WindowEventSender) -> Retained<Self> {
        let view: Allocated<Self> = unsafe { msg_send![Self::class(), alloc] };
        let view: Retained<Self> = unsafe { msg_send![super(view.set_ivars(sender)), init] };
        let _: () = unsafe { msg_send![&*view, setMultipleTouchEnabled:true] };
        view
    }

    fn touch(&self, touches: *mut Object, event: fn(TouchEvent) -> WindowEvent) {
        let view = self as *const Self as *mut Object;
        let changed_touches = unsafe { ios_touches(view, touches) };
        if !changed_touches.is_empty() {
            self.ivars().send(event(TouchEvent {
                changed_touches,
                ..TouchEvent::default()
            }));
        }
    }
}
