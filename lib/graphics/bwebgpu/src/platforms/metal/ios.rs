/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! UIKit Metal presentation with on-demand display-link callbacks.

use objc2::rc::{Allocated, Retained, autoreleasepool};
use objc2::runtime::{AnyObject, Bool};
use objc2::{class, define_class, msg_send, sel};

use super::*;

pub(crate) type Factory = ();

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "BwebgpuIOSRedrawTarget"]
    #[ivars = bwindow::WindowEventSender]
    struct RedrawTarget;

    impl RedrawTarget {
        #[unsafe(method(drawFrame:))]
        fn draw_frame(&self, link: *mut AnyObject) {
            let _: () = unsafe { msg_send![link, setPaused:Bool::YES] };
            self.ivars().send(bwindow::WindowEvent::RedrawRequested);
        }
    }
);

pub(crate) struct Surface {
    view: Object,
    layer: Object,
    redraw: Retained<RedrawTarget>,
    display_link: Object,
}

impl Surface {
    pub(crate) fn new(
        attachment: &bwindow::WindowAttachment,
        _: &mut Option<Factory>,
    ) -> Result<Self> {
        let sender = attachment.event_sender();
        let Some(bwindow::NativeContentHandle::UIKit(view)) =
            (unsafe { attachment.content_handle() })
        else {
            return Err(Error("Metal requires a UIKit content view".into()));
        };
        let is_main_thread: Bool = unsafe { msg_send![class!(NSThread), isMainThread] };
        if !is_main_thread.as_bool() {
            return Err(Error(
                "Metal surface creation requires the main thread".into(),
            ));
        }
        autoreleasepool(|_| unsafe {
            let view = retained(view.cast())?;
            let layer = owned(msg_send![class!(CAMetalLayer), new])?;
            let _: () = msg_send![&*layer, setOpaque:Bool::YES];
            let _: () = msg_send![&*layer, setMaximumDrawableCount:3usize];
            let _: () = msg_send![&*layer, setAllowsNextDrawableTimeout:Bool::YES];
            let _: () = msg_send![&*layer, setContentsScale:f64::from(attachment.scale_factor())];
            // A sublayer at the bottom of the shared content view, below its children.
            let parent: *mut AnyObject = msg_send![&*view, layer];
            let _: () = msg_send![parent, insertSublayer:&*layer, atIndex:0u32];
            layout_layer(&view, &layer);
            let (resize_view, resize_layer) = (view.clone(), layer.clone());
            attachment.on_resize(move |_, _| layout_layer(&resize_view, &resize_layer));
            let scale_layer = layer.clone();
            attachment.on_scale_factor_changed(move |scale| {
                let _: () = msg_send![&*scale_layer, setContentsScale:f64::from(scale)];
            });
            let redraw: Allocated<RedrawTarget> = msg_send![RedrawTarget::class(), alloc];
            let redraw: Retained<RedrawTarget> = msg_send![super(redraw.set_ivars(sender)), init];
            let display_link = retained(
                msg_send![class!(CADisplayLink), displayLinkWithTarget:&*redraw, selector:sel!(drawFrame:)],
            )?;
            let _: () = msg_send![&*display_link, setPaused:Bool::YES];
            let run_loop: *mut AnyObject = msg_send![class!(NSRunLoop), mainRunLoop];
            let _: () =
                msg_send![&*display_link, addToRunLoop:run_loop, forMode:NSRunLoopCommonModes];
            Ok(Self {
                view,
                layer,
                redraw,
                display_link,
            })
        })
    }

    pub(crate) fn request_redraw(&self) {
        self.redraw
            .ivars()
            .send(bwindow::WindowEvent::RedrawRequested);
    }

    pub(crate) fn request_animation_frame(&self) {
        let _: () = unsafe { msg_send![&*self.display_link, setPaused:Bool::NO] };
    }

    pub(crate) fn size(&self) -> (u32, u32) {
        unsafe {
            let bounds: Rect = msg_send![&*self.view, bounds];
            let scale: f64 = msg_send![&*self.view, contentScaleFactor];
            (
                (bounds.size.width * scale).round().max(0.0) as u32,
                (bounds.size.height * scale).round().max(0.0) as u32,
            )
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            let _: () = msg_send![&*self.display_link, invalidate];
            let _: () = msg_send![&*self.layer, removeFromSuperlayer];
        }
    }
}

#[path = "presentation.rs"]
mod presentation;
