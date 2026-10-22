/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The shared content view of a window: the root UIView of its view controller, which
//! renderers add their layers to. It delivers touches as left mouse button pointer events.
//! Window children like a web view receive their own touches, so the view only reports
//! touches that start on it and never delivers them twice.

use objc2::rc::{Allocated, Retained};
use objc2::runtime::AnyObject as Object;
use objc2::{define_class, msg_send};

use super::input::ios_touch_event;
use crate::{ButtonState, WindowEvent, WindowEventSender};

define_class!(
    #[unsafe(super(UIView))]
    #[name = "BwindowContentView"]
    #[ivars = WindowEventSender]
    pub(super) struct ContentView;

    impl ContentView {
        #[unsafe(method(touchesBegan:withEvent:))]
        fn _touches_began(&self, touches: *mut Object, _: *mut Object) {
            self.touch(touches, Some(ButtonState::Pressed));
        }

        #[unsafe(method(touchesMoved:withEvent:))]
        fn _touches_moved(&self, touches: *mut Object, _: *mut Object) {
            self.touch(touches, None);
        }

        #[unsafe(method(touchesEnded:withEvent:))]
        fn _touches_ended(&self, touches: *mut Object, _: *mut Object) {
            if self.touch(touches, Some(ButtonState::Released)) {
                self.ivars().send(WindowEvent::MouseLeave);
            }
        }

        #[unsafe(method(touchesCancelled:withEvent:))]
        fn _touches_cancelled(&self, touches: *mut Object, _: *mut Object) {
            if self.touch(touches, Some(ButtonState::Released)) {
                self.ivars().send(WindowEvent::MouseLeave);
            }
        }
    }
);

impl ContentView {
    pub(super) fn new(sender: WindowEventSender) -> Retained<Self> {
        let view: Allocated<Self> = unsafe { msg_send![Self::class(), alloc] };
        unsafe { msg_send![super(view.set_ivars(sender)), init] }
    }

    // Report a touch that started on this view itself, returns whether it did.
    fn touch(&self, touches: *mut Object, state: Option<ButtonState>) -> bool {
        let view = self as *const Self as *mut Object;
        let touch: *mut Object = unsafe { msg_send![touches, anyObject] };
        let target: *mut Object = unsafe { msg_send![touch, view] };
        if target != view {
            return false;
        }
        self.ivars()
            .send(unsafe { ios_touch_event(view, touches, state) });
        true
    }
}
