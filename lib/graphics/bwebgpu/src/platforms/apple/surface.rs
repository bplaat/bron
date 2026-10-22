/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! CAMetalLayer presentation for bwindow content views.

use std::cell::Cell;
use std::ffi::c_void;
use std::ptr;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use objc2::rc::{Allocated, Retained, autoreleasepool};
use objc2::runtime::{AnyObject, Bool};
use objc2::{class, define_class, msg_send, sel};

use super::*;

pub(crate) type Factory = ();

struct RedrawState {
    sender: bwindow::WindowEventSender,
    display_link: Cell<CVDisplayLinkRef>,
    display_link_running: Cell<bool>,
    screen: Cell<*mut AnyObject>,
    signal: Arc<DisplayLinkSignal>,
}

type RedrawIvars = Rc<RedrawState>;

struct DisplayLinkSignal {
    target: AtomicPtr<AnyObject>,
    posted: AtomicBool,
}

unsafe extern "C" fn display_link_callback(
    _: CVDisplayLinkRef,
    _: *const c_void,
    _: *const c_void,
    _: u64,
    _: *mut u64,
    user_info: *mut c_void,
) -> i32 {
    let signal = unsafe { &*user_info.cast::<DisplayLinkSignal>() };
    if !signal.posted.swap(true, Ordering::AcqRel) {
        let target = signal.target.load(Ordering::Acquire);
        if !target.is_null() {
            autoreleasepool(|_| unsafe {
                let _: () = msg_send![target,
                    performSelectorOnMainThread:sel!(drawFrame:),
                    withObject:ptr::null_mut::<AnyObject>(), waitUntilDone:Bool::NO];
            });
        }
    }
    0
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "BwebgpuRedrawTarget"]
    #[ivars = RedrawIvars]
    struct RedrawTarget;

    impl RedrawTarget {
        #[unsafe(method(drawFrame:))]
        fn draw_frame(&self, _: *mut AnyObject) {
            let state = self.ivars();
            if state.display_link_running.replace(false) {
                unsafe {
                    CVDisplayLinkStop(state.display_link.get());
                }
            }
            state.signal.posted.store(false, Ordering::Release);
            state
                .sender
                .send(bwindow::WindowEvent::RedrawRequested);
        }
    }
);

pub(crate) struct Surface {
    view: Object,
    layer: Object,
    redraw: Retained<RedrawTarget>,
}

impl Surface {
    pub(crate) fn new(
        attachment: &bwindow::WindowAttachment,
        _: &mut Option<Factory>,
    ) -> Result<Self> {
        let sender = attachment.event_sender();
        let Some(bwindow::NativeContentHandle::AppKit(view)) =
            (unsafe { attachment.content_handle() })
        else {
            return Err(Error("Metal requires an AppKit window".into()));
        };
        // AppKit layer operations must run on the main thread.
        let is_main_thread: Bool = unsafe { msg_send![class!(NSThread), isMainThread] };
        if !is_main_thread.as_bool() {
            return Err(Error(
                "Metal surface creation requires the main thread".into(),
            ));
        }
        autoreleasepool(|_| unsafe {
            let view = retained(view.cast())?;
            let signal = Arc::new(DisplayLinkSignal {
                target: AtomicPtr::new(ptr::null_mut()),
                posted: AtomicBool::new(false),
            });
            let redraw: Allocated<RedrawTarget> = msg_send![RedrawTarget::class(), alloc];
            let redraw: Retained<RedrawTarget> = msg_send![
                super(redraw.set_ivars(Rc::new(RedrawState {
                    sender,
                    display_link: Cell::new(ptr::null_mut()),
                    display_link_running: Cell::new(false),
                    screen: Cell::new(ptr::null_mut()),
                    signal: signal.clone(),
                }))),
                init
            ];
            signal
                .target
                .store(redraw.as_ptr().cast(), Ordering::Release);
            let mut display_link = ptr::null_mut();
            let result = CVDisplayLinkCreateWithActiveCGDisplays(&mut display_link);
            if result != 0 {
                return Err(Error(format!(
                    "Could not create Core Video display link: {result}"
                )));
            }
            let result = CVDisplayLinkSetOutputCallback(
                display_link,
                display_link_callback,
                Arc::as_ptr(&signal).cast_mut().cast(),
            );
            if result != 0 {
                CVDisplayLinkRelease(display_link);
                return Err(Error(format!(
                    "Could not configure Core Video display link: {result}"
                )));
            }
            redraw.ivars().display_link.set(display_link);
            // A sublayer at the bottom of the shared content view, below its children.
            let layer = owned(msg_send![class!(CAMetalLayer), new])?;
            let _: () = msg_send![&*layer, setOpaque: Bool::YES];
            let _: () = msg_send![&*layer, setMaximumDrawableCount: 3usize];
            let _: () = msg_send![&*layer, setAllowsNextDrawableTimeout: Bool::YES];
            let _: () = msg_send![&*layer, setContentsScale: f64::from(attachment.scale_factor())];
            let backing: *mut AnyObject = msg_send![&*view, layer];
            let _: () = msg_send![backing, insertSublayer: &*layer, atIndex: 0u32];
            layout_layer(&view, &layer);
            let (resize_view, resize_layer) = (view.clone(), layer.clone());
            attachment.on_resize(move |_, _| layout_layer(&resize_view, &resize_layer));
            let scale_layer = layer.clone();
            attachment.on_scale_factor_changed(move |scale| {
                let _: () = msg_send![&*scale_layer, setContentsScale: f64::from(scale)];
            });
            Ok(Self {
                view,
                layer,
                redraw,
            })
        })
    }

    pub(crate) fn request_redraw(&self) {
        self.redraw
            .ivars()
            .sender
            .send(bwindow::WindowEvent::RedrawRequested);
    }

    pub(crate) fn request_animation_frame(&self) {
        let state = self.redraw.ivars();
        self.synchronize_display_link();
        state.signal.posted.store(false, Ordering::Release);
        if !state.display_link_running.replace(true) {
            let result = unsafe { CVDisplayLinkStart(state.display_link.get()) };
            assert_eq!(
                result, 0,
                "Could not start Core Video display link: {result}"
            );
        }
    }

    pub(crate) fn size(&self) -> (u32, u32) {
        unsafe {
            let bounds: Rect = msg_send![&*self.view, bounds];
            let window: *mut AnyObject = msg_send![&*self.view, window];
            let scale: f64 = if window.is_null() {
                1.0
            } else {
                msg_send![window, backingScaleFactor]
            };
            (
                (bounds.size.width * scale).round().max(0.0) as u32,
                (bounds.size.height * scale).round().max(0.0) as u32,
            )
        }
    }

    fn synchronize_display_link(&self) {
        let state = self.redraw.ivars();
        unsafe {
            let window: *mut AnyObject = msg_send![&*self.view, window];
            if window.is_null() {
                return;
            }
            let screen: *mut AnyObject = msg_send![window, screen];
            if screen.is_null() || state.screen.get() == screen {
                return;
            }
            let description: *mut AnyObject = msg_send![screen, deviceDescription];
            let key = string("NSScreenNumber");
            let number: *mut AnyObject = msg_send![description, objectForKey:&*key];
            if number.is_null() {
                return;
            }
            let display_id: u32 = msg_send![number, unsignedIntValue];
            if CVDisplayLinkSetCurrentCGDisplay(state.display_link.get(), display_id) == 0 {
                state.screen.set(screen);
            }
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            let state = self.redraw.ivars();
            if state.display_link_running.replace(false) {
                CVDisplayLinkStop(state.display_link.get());
            }
            state
                .signal
                .target
                .store(ptr::null_mut(), Ordering::Release);
            let display_link = state.display_link.replace(ptr::null_mut());
            if !display_link.is_null() {
                CVDisplayLinkRelease(display_link);
            }
            let _: () = msg_send![&*self.layer, removeFromSuperlayer];
        }
    }
}

#[path = "presentation.rs"]
mod presentation;
