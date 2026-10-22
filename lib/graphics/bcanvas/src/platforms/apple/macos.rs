/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::Cell;
use std::ptr::null_mut;
use std::rc::Rc;

use bwindow::{NativeContentHandle, WindowAttachment, WindowEventSender};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, define_class, msg_send};

use super::context::{PlatformCanvasContext, TextStyleCache};
use super::headers::*;
use crate::CanvasRenderingContext2d;

#[derive(Clone, Copy)]
struct Frame {
    context: CGContextRef,
    width: f32,
    height: f32,
    scale: f32,
}

// Draws a CALayer at the bottom of the shared window content view with Core Graphics, the
// window delivers the interaction and paces the animation frames.
struct LayerState {
    view: Retained<Object>,
    layer: Retained<Object>,
    sender: WindowEventSender,
    frame: Cell<Option<Frame>>,
    closed: Cell<bool>,
    painting: Cell<bool>,
}

type DelegateIvars = Rc<LayerState>;

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "BcanvasLayerDelegate"]
    #[ivars = DelegateIvars]
    struct LayerDelegate;

    impl LayerDelegate {
        #[unsafe(method(drawLayer:inContext:))]
        fn _draw_layer(&self, _: *mut Object, context: CGContextRef) { self.paint(context); }

        // Resize and redisplay the layer at once, without implicit animations.
        #[unsafe(method(actionForLayer:forKey:))]
        fn _action_for_layer(&self, _: *mut Object, _: *mut Object) -> *mut Object {
            unsafe { msg_send![class!(NSNull), null] }
        }
    }
);

impl LayerDelegate {
    fn request_redraw(&self) {
        let state = self.ivars();
        if state.closed.get() {
            return;
        }
        // Redraws requested while painting wait for the next display frame.
        if state.painting.get() {
            state.sender.request_animation_frame();
        } else {
            let _: () = unsafe { msg_send![&*state.layer, setNeedsDisplay] };
        }
    }

    // Fill the layer with the content view and match its display scale.
    fn layout(&self, scale: f32) {
        let state = self.ivars();
        unsafe {
            let bounds: CGRect = msg_send![&*state.view, bounds];
            let _: () = msg_send![class!(CATransaction), begin];
            let _: () = msg_send![class!(CATransaction), setDisableActions:Bool::YES];
            let _: () = msg_send![&*state.layer, setFrame:bounds];
            let _: () = msg_send![&*state.layer, setContentsScale:f64::from(scale)];
            let _: () = msg_send![class!(CATransaction), commit];
        }
        self.request_redraw();
    }

    fn close(&self) {
        let state = self.ivars();
        if state.closed.replace(true) {
            return;
        }
        state.frame.set(None);
        unsafe {
            // The layer delegate is non-owning, so clear it before the delegate is released.
            let _: () = msg_send![&*state.layer, setDelegate:null_mut::<Object>()];
            let _: () = msg_send![&*state.layer, removeFromSuperlayer];
        }
    }

    fn paint(&self, context: CGContextRef) {
        if self.ivars().closed.get() || context.is_null() {
            return;
        }
        // Application code can drop its canvas or close the window during paint.
        let retained: Retained<Self> = unsafe { msg_send![self, retain] };
        let state = retained.ivars();
        if state.painting.get() {
            self.request_redraw();
            return;
        }
        unsafe {
            let bounds: CGRect = msg_send![&*state.layer, bounds];
            let scale: f64 = msg_send![&*state.layer, contentsScale];
            // Canvases draw top-down. The layer of the flipped content view shows its
            // contents flipped already, otherwise flip the bottom-left origin context.
            let flipped: Bool = msg_send![&*state.layer, contentsAreFlipped];
            if !flipped.as_bool() {
                CGContextTranslateCTM(context, 0.0, bounds.size.height);
                CGContextScaleCTM(context, 1.0, -1.0);
            }
            state.painting.set(true);
            state.frame.set(Some(Frame {
                context,
                width: bounds.size.width as f32,
                height: bounds.size.height as f32,
                scale: scale as f32,
            }));
            if !state.sender.redraw(|| state.frame.set(None)) {
                state.frame.set(None);
                self.request_redraw();
            }
            state.painting.set(false);
        }
    }
}

pub(crate) struct PlatformCanvas {
    delegate: Retained<LayerDelegate>,
    attachment: WindowAttachment,
    text_styles: TextStyleCache,
}

impl PlatformCanvas {
    pub(crate) fn new(attachment: WindowAttachment) -> Self {
        let Some(NativeContentHandle::AppKit(view)) = (unsafe { attachment.content_handle() })
        else {
            panic!("expected an open AppKit window");
        };
        unsafe {
            let view = Retained::retain(view.cast::<Object>()).expect("content view");
            let layer: Retained<Object> = msg_send![class!(CALayer), new];
            let delegate: Allocated<LayerDelegate> = msg_send![LayerDelegate::class(), alloc];
            let delegate: Retained<LayerDelegate> = msg_send![
                super(delegate.set_ivars(Rc::new(LayerState {
                    view: view.clone(),
                    layer: layer.clone(),
                    sender: attachment.event_sender(),
                    frame: Cell::new(None),
                    closed: Cell::new(false),
                    painting: Cell::new(false),
                }))),
                init
            ];

            // Transparent canvas pixels show the window background.
            let _: () = msg_send![&*layer, setOpaque:Bool::NO];
            let _: () = msg_send![&*layer, setNeedsDisplayOnBoundsChange:Bool::YES];
            let _: () = msg_send![&*layer, setDelegate:&*delegate];
            let backing: *mut Object = msg_send![&*view, layer];
            let _: () = msg_send![backing, insertSublayer:&*layer, atIndex:0u32];
            delegate.layout(attachment.scale_factor());

            let resize = delegate.clone();
            attachment.on_resize(move |_, _| {
                let scale: f64 = msg_send![&*resize.ivars().layer, contentsScale];
                resize.layout(scale as f32);
            });
            let scale = delegate.clone();
            attachment.on_scale_factor_changed(move |factor| scale.layout(factor));
            let redraw = delegate.clone();
            attachment.on_redraw(move || redraw.request_redraw());
            let close = delegate.clone();
            attachment.on_close(move || close.close());
            Self {
                delegate,
                attachment,
                text_styles: TextStyleCache::default(),
            }
        }
    }

    pub(crate) fn request_redraw(&self) {
        self.delegate.request_redraw();
    }

    pub(crate) fn request_animation_frame(&self) {
        self.attachment.request_animation_frame();
    }

    pub(crate) fn draw(
        &mut self,
        draw: impl for<'frame> FnOnce(&mut CanvasRenderingContext2d<'frame>),
    ) -> bool {
        if self.attachment.is_closed() {
            return false;
        }
        let Some(frame) = self.delegate.ivars().frame.take() else {
            return false;
        };
        let mut context = CanvasRenderingContext2d::new(
            unsafe { PlatformCanvasContext::new(frame.context, self.text_styles.clone()) },
            frame.width,
            frame.height,
            frame.scale,
        );
        draw(&mut context);
        true
    }
}

impl Drop for PlatformCanvas {
    fn drop(&mut self) {
        self.attachment.disconnect();
        self.delegate.close();
    }
}
