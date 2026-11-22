/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// Draws a native window layer: the DirectComposition visual of the layer, presented by a
// composition swap chain drawn with Direct2D and blended by the desktop compositor. The
// window delivers the interaction and paces the animation frames.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use bwindow::{NativeContentHandle, WindowAttachment, WindowEventSender};

use crate::CanvasRenderingContext2d;
use crate::platforms::windows::PlatformCanvasContext;
use crate::platforms::windows::direct2d::CompositionTarget;

struct LayerData {
    device: *mut std::ffi::c_void,
    visual: *mut std::ffi::c_void,
    attachment: WindowAttachment,
    sender: WindowEventSender,
    target: RefCell<Option<CompositionTarget>>,
    closed: Cell<bool>,
    painting: Cell<bool>,
    frame: RefCell<Option<CanvasRenderingContext2d<'static>>>,
}

impl LayerData {
    // Paint now when the loop can deliver the redraw, or else at the next display frame.
    fn paint(&self) {
        if self.closed.get() || self.attachment.is_hidden() {
            return;
        }
        if self.painting.replace(true) {
            self.sender.request_animation_frame();
            return;
        }
        let (width, height) = self.attachment.size();
        let scale = self.attachment.scale_factor();
        let mut target = self.target.borrow_mut();
        if target.is_none() {
            *target = unsafe { CompositionTarget::new(self.device, self.visual, width, height) };
        }
        let platform = target
            .as_mut()
            .and_then(|target| target.begin(width, height, scale));
        drop(target);
        let Some(platform) = platform else {
            // A lost device is created again at the next frame.
            self.target.borrow_mut().take();
            self.painting.set(false);
            self.sender.request_animation_frame();
            return;
        };
        *self.frame.borrow_mut() = Some(CanvasRenderingContext2d::new(
            PlatformCanvasContext::Direct2D(Box::new(platform)),
            width.max(1) as f32 / scale,
            height.max(1) as f32 / scale,
            scale,
        ));
        if !self.sender.redraw(|| self.end(true)) {
            self.end(false);
            self.sender.request_animation_frame();
        }
        self.painting.set(false);
    }

    fn end(&self, present: bool) {
        self.frame.borrow_mut().take();
        let mut target = self.target.borrow_mut();
        if let Some(current) = target.as_mut()
            && !current.end(present)
        {
            target.take();
        }
    }

    fn close(&self) {
        self.closed.set(true);
        self.frame.borrow_mut().take();
        self.target.borrow_mut().take();
    }
}

pub(crate) struct PlatformLayerCanvas {
    data: Rc<LayerData>,
}

impl PlatformLayerCanvas {
    pub(crate) fn new(attachment: WindowAttachment) -> Self {
        let Some(NativeContentHandle::DirectComposition { device, visual }) =
            (unsafe { attachment.content_handle() })
        else {
            panic!("expected a DirectComposition layer");
        };
        let data = Rc::new(LayerData {
            device,
            visual,
            sender: attachment.event_sender(),
            attachment,
            target: RefCell::new(None),
            closed: Cell::new(false),
            painting: Cell::new(false),
            frame: RefCell::new(None),
        });
        let redraw = Rc::downgrade(&data);
        data.attachment.on_redraw(move || {
            if let Some(data) = redraw.upgrade() {
                data.paint();
            }
        });
        let resize = Rc::downgrade(&data);
        data.attachment.on_resize(move |_, _| {
            if let Some(data) = resize.upgrade() {
                data.sender.request_animation_frame();
            }
        });
        let close = Rc::downgrade(&data);
        data.attachment.on_close(move || {
            if let Some(data) = close.upgrade() {
                data.close();
            }
        });
        data.sender.request_animation_frame();
        Self { data }
    }

    pub(crate) fn request_redraw(&self) {
        self.data.paint();
    }

    pub(crate) fn request_animation_frame(&self) {
        self.data.sender.request_animation_frame();
    }

    pub(crate) fn draw(
        &mut self,
        draw: impl for<'frame> FnOnce(&mut CanvasRenderingContext2d<'frame>),
    ) -> bool {
        if self.data.attachment.is_closed() {
            return false;
        }
        let frame = self.data.frame.borrow_mut().take();
        let Some(mut frame) = frame else { return false };
        draw(&mut frame);
        true
    }
}

impl Drop for PlatformLayerCanvas {
    fn drop(&mut self) {
        self.data.attachment.disconnect();
        self.data.close();
    }
}
