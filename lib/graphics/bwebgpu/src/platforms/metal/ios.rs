/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! CAMetalLayer presentation for bwindow UIKit content views.

use objc2::rc::autoreleasepool;
use objc2::runtime::{AnyObject, Bool};
use objc2::{class, msg_send};

use super::*;

pub(crate) type Factory = ();

pub(crate) struct Surface {
    view: Object,
    layer: Object,
}

impl Surface {
    pub(crate) fn new(
        attachment: &bwindow::WindowAttachment,
        _: &mut Option<Factory>,
    ) -> Result<Self> {
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
            Ok(Self { view, layer })
        })
    }

    pub(crate) fn size(&self) -> (u32, u32) {
        unsafe {
            let bounds: Rect = msg_send![&*self.view, bounds];
            let scale: f64 = msg_send![&*self.layer, contentsScale];
            (
                (bounds.size.width * scale).round().max(0.0) as u32,
                (bounds.size.height * scale).round().max(0.0) as u32,
            )
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        let _: () = unsafe { msg_send![&*self.layer, removeFromSuperlayer] };
    }
}

#[path = "presentation.rs"]
mod presentation;
