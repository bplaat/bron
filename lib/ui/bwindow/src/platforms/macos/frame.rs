/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The display frames of a window: a Core Video display link of the window's screen,
//! which runs while animation frames are requested and stops one frame after.

use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::null_mut;
use std::rc::Weak;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use objc2::rc::{Allocated, Retained, autoreleasepool};
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, define_class, msg_send, sel};

use super::headers::*;
use crate::NativeWindowHandle;
use crate::content::ContentHost;

type CVDisplayLinkRef = *mut c_void;
type CVDisplayLinkOutputCallback = unsafe extern "C" fn(
    CVDisplayLinkRef,
    *const c_void,
    *const c_void,
    u64,
    *mut u64,
    *mut c_void,
) -> i32;

#[link(name = "CoreVideo", kind = "framework")]
unsafe extern "C" {
    fn CVDisplayLinkCreateWithActiveCGDisplays(display_link: *mut CVDisplayLinkRef) -> i32;
    fn CVDisplayLinkSetOutputCallback(
        display_link: CVDisplayLinkRef,
        callback: CVDisplayLinkOutputCallback,
        user_info: *mut c_void,
    ) -> i32;
    fn CVDisplayLinkSetCurrentCGDisplay(display_link: CVDisplayLinkRef, display_id: u32) -> i32;
    fn CVDisplayLinkStart(display_link: CVDisplayLinkRef) -> i32;
    fn CVDisplayLinkStop(display_link: CVDisplayLinkRef) -> i32;
    fn CVDisplayLinkRelease(display_link: CVDisplayLinkRef);
}

// Shared with the display link thread, which posts one frame at a time to the main thread.
struct Signal {
    target: AtomicPtr<Object>,
    posted: AtomicBool,
}

pub(crate) struct FrameIvars {
    host: Weak<ContentHost>,
    window: *mut Object,
    display_link: CVDisplayLinkRef,
    running: Cell<bool>,
    requested: Cell<bool>,
    screen: Cell<*mut Object>,
    signal: Arc<Signal>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "BwindowFrameSource"]
    #[ivars = FrameIvars]
    pub(crate) struct FrameTarget;

    impl FrameTarget {
        #[unsafe(method(displayFrame:))]
        fn _display_frame(&self, _: *mut Object) {
            let state = self.ivars();
            state.signal.posted.store(false, Ordering::Release);
            if !state.requested.replace(false) {
                self.stop();
            } else if let Some(host) = state.host.upgrade() {
                host.animation_frame();
                // Commit layer changes of the frame now, AppKit otherwise commits them in
                // its own display cycle, which runs at 60 Hz also on faster displays.
                let _: () = unsafe { msg_send![class!(CATransaction), flush] };
            }
        }
    }
);

unsafe extern "C" fn display_link_callback(
    _: CVDisplayLinkRef,
    _: *const c_void,
    _: *const c_void,
    _: u64,
    _: *mut u64,
    user_info: *mut c_void,
) -> i32 {
    let signal = unsafe { &*user_info.cast::<Signal>() };
    if !signal.posted.swap(true, Ordering::AcqRel) {
        let target = signal.target.load(Ordering::Acquire);
        if !target.is_null() {
            autoreleasepool(|_| unsafe {
                let _: () = msg_send![target,
                    performSelectorOnMainThread:sel!(displayFrame:),
                    withObject:null_mut::<Object>(), waitUntilDone:Bool::NO];
            });
        }
    }
    0
}

impl FrameTarget {
    fn stop(&self) {
        let state = self.ivars();
        if state.running.replace(false) {
            unsafe { CVDisplayLinkStop(state.display_link) };
        }
    }

    // Follow the window to the display it is on.
    fn synchronize_display(&self) {
        let state = self.ivars();
        unsafe {
            let screen: *mut Object = msg_send![state.window, screen];
            if screen.is_null() || state.screen.get() == screen {
                return;
            }
            let description: *mut Object = msg_send![screen, deviceDescription];
            let number: *mut Object =
                msg_send![description, objectForKey:ns_string!("NSScreenNumber")];
            if number.is_null() {
                return;
            }
            let display_id: u32 = msg_send![number, unsignedIntValue];
            if CVDisplayLinkSetCurrentCGDisplay(state.display_link, display_id) == 0 {
                state.screen.set(screen);
            }
        }
    }
}

pub(crate) struct FrameSource(Retained<FrameTarget>);

impl FrameSource {
    pub(crate) fn new(handle: NativeWindowHandle, host: Weak<ContentHost>) -> Option<Self> {
        let NativeWindowHandle::AppKit(window) = handle else {
            return None;
        };
        let signal = Arc::new(Signal {
            target: AtomicPtr::new(null_mut()),
            posted: AtomicBool::new(false),
        });
        unsafe {
            let mut display_link = null_mut();
            if CVDisplayLinkCreateWithActiveCGDisplays(&mut display_link) != 0 {
                return None;
            }
            if CVDisplayLinkSetOutputCallback(
                display_link,
                display_link_callback,
                Arc::as_ptr(&signal).cast_mut().cast(),
            ) != 0
            {
                CVDisplayLinkRelease(display_link);
                return None;
            }
            let target: Allocated<FrameTarget> = msg_send![FrameTarget::class(), alloc];
            let target: Retained<FrameTarget> = msg_send![
                super(target.set_ivars(FrameIvars {
                    host,
                    window: window.cast(),
                    display_link,
                    running: Cell::new(false),
                    requested: Cell::new(false),
                    screen: Cell::new(null_mut()),
                    signal: signal.clone(),
                })),
                init
            ];
            signal
                .target
                .store(target.as_ptr().cast(), Ordering::Release);
            Some(Self(target))
        }
    }

    pub(crate) fn request(&self) {
        let state = self.0.ivars();
        state.requested.set(true);
        self.0.synchronize_display();
        if !state.running.replace(true) {
            unsafe { CVDisplayLinkStart(state.display_link) };
        }
    }
}

impl Drop for FrameSource {
    fn drop(&mut self) {
        let state = self.0.ivars();
        self.0.stop();
        // Stopping waits for a running callback, no frame is posted to the target after.
        state.signal.target.store(null_mut(), Ordering::Release);
        unsafe { CVDisplayLinkRelease(state.display_link) };
    }
}
