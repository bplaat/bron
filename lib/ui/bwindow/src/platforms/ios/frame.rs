/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The display frames of a window: a display link, which runs while animation frames are
//! requested and pauses one frame after.

use std::cell::Cell;
use std::rc::Weak;

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, define_class, msg_send, sel};

use crate::NativeWindowHandle;
use crate::content::ContentHost;

#[link(name = "Foundation", kind = "framework")]
unsafe extern "C" {
    static NSRunLoopCommonModes: *mut Object;
}

pub(crate) struct FrameIvars {
    host: Weak<ContentHost>,
    requested: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "BwindowFrameSource"]
    #[ivars = FrameIvars]
    struct FrameTarget;

    impl FrameTarget {
        #[unsafe(method(displayFrame:))]
        fn _display_frame(&self, link: *mut Object) {
            if !self.ivars().requested.replace(false) {
                let _: () = unsafe { msg_send![link, setPaused:Bool::YES] };
            } else if let Some(host) = self.ivars().host.upgrade() {
                host.animation_frame();
                // Commit layer changes of the frame now, at the display frame.
                let _: () = unsafe { msg_send![class!(CATransaction), flush] };
            }
        }
    }
);

pub(crate) struct FrameSource {
    target: Retained<FrameTarget>,
    display_link: Retained<Object>,
}

impl FrameSource {
    pub(crate) fn new(_: NativeWindowHandle, host: Weak<ContentHost>) -> Option<Self> {
        unsafe {
            let target: Allocated<FrameTarget> = msg_send![FrameTarget::class(), alloc];
            let target: Retained<FrameTarget> = msg_send![
                super(target.set_ivars(FrameIvars {
                    host,
                    requested: Cell::new(false),
                })),
                init
            ];
            let display_link: Retained<Object> = msg_send![class!(CADisplayLink),
                displayLinkWithTarget:&*target, selector:sel!(displayFrame:)];
            let _: () = msg_send![&display_link, setPaused:Bool::YES];
            let run_loop: *mut Object = msg_send![class!(NSRunLoop), mainRunLoop];
            let _: () =
                msg_send![&display_link, addToRunLoop:run_loop, forMode:NSRunLoopCommonModes];
            Some(Self {
                target,
                display_link,
            })
        }
    }

    pub(crate) fn request(&self) {
        self.target.ivars().requested.set(true);
        let _: () = unsafe { msg_send![&self.display_link, setPaused:Bool::NO] };
    }
}

impl Drop for FrameSource {
    fn drop(&mut self) {
        // The display link retains its target, invalidate it to release both.
        let _: () = unsafe { msg_send![&self.display_link, invalidate] };
    }
}
