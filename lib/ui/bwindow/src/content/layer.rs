/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The layers of window content and their native views.

use super::*;

/// One layer of window content, drawn by one renderer.
pub(crate) struct LayerSlot {
    pub(super) host: Weak<ContentHost>,
    // The window content below every layer, which renderers draw in the content host.
    pub(super) base: bool,
    // The native host of the layer, created when its renderer asks for it.
    pub(super) native: Cell<Option<NativeContentHandle>>,
    pub(super) frame: Cell<Option<LogicalRect>>,
    pub(super) hidden: Cell<bool>,
    pub(super) attached: Cell<bool>,
    pub(super) ready_handler: RefCell<Option<CloseHandler>>,
    pub(super) resize: RefCell<Option<ResizeHandler>>,
    pub(super) scale: RefCell<Option<ScaleHandler>>,
    pub(super) close: RefCell<Option<CloseHandler>>,
    pub(super) redraw: RefCell<Option<Rc<dyn Fn()>>>,
    #[cfg(windows)]
    pub(super) paint: RefCell<Option<Rc<dyn Fn()>>>,
    pub(super) change: RefCell<Option<Rc<dyn Fn()>>>,
    pub(super) animation_frame_pending: Cell<bool>,
}

impl LayerSlot {
    pub(super) fn changed(&self) {
        let handler = self.change.borrow().clone();
        if let Some(handler) = handler {
            handler();
        }
    }

    pub(super) fn resized(&self, width: u32, height: u32) {
        let handler = self.resize.borrow().clone();
        if let Some(handler) = handler {
            handler(width, height);
        }
    }

    pub(super) fn request_redraw(&self) {
        let redraw = self.redraw.borrow().clone();
        match redraw {
            Some(redraw) => redraw(),
            None => {
                if let Some(host) = self.host.upgrade() {
                    host.deliver_redraw();
                }
            }
        }
    }

    pub(super) fn request_animation_frame(&self) {
        let Some(host) = self.host.upgrade().filter(|host| !host.closed.get()) else {
            return;
        };
        if !self.animation_frame_pending.replace(true) && !host.schedule_frame() {
            self.animation_frame_pending.set(false);
        }
    }

    // The native host of the layer, a view of its own on platforms with native layers.
    pub(super) fn content_handle(
        self: &Rc<Self>,
        host: &ContentHost,
    ) -> Option<NativeContentHandle> {
        let content = host.content_handle()?;
        if self.base || !unsafe { native_layers(content) } {
            return Some(content);
        }
        if let Some(native) = self.native.get() {
            return Some(native);
        }
        // The layers below get their views first, so the views are created bottom to top
        // like platforms without restacking stack them, and each goes above the one below.
        let layers: Vec<_> = host
            .layers()
            .into_iter()
            .filter(|layer| !layer.base)
            .collect();
        let index = layers.iter().position(|layer| Rc::ptr_eq(layer, self))?;
        for layer in &layers[..index] {
            layer.content_handle(host);
        }
        let neighbor = index
            .checked_sub(1)
            .and_then(|below| layers[below].native.get())
            .map(|native| (native, true));
        let native = unsafe { add_layer_view(content, neighbor) };
        self.native.set(Some(native));
        unsafe {
            set_layer_view_frame(content, native, self.frame.get());
            set_layer_view_hidden(native, self.hidden.get());
        }
        Some(native)
    }

    // Take the hooks out before dropping them, their destructors can register hooks again.
    pub(super) fn disconnect(&self) -> Option<CloseHandler> {
        let ready = self.ready_handler.borrow_mut().take();
        drop(ready);
        let redraw = self.redraw.borrow_mut().take();
        drop(redraw);
        #[cfg(windows)]
        {
            let paint = self.paint.borrow_mut().take();
            drop(paint);
        }
        self.animation_frame_pending.set(false);
        let resize = self.resize.borrow_mut().take();
        drop(resize);
        let scale = self.scale.borrow_mut().take();
        drop(scale);
        let change = self.change.borrow_mut().take();
        drop(change);
        self.close.borrow_mut().take()
    }
}

impl Drop for LayerSlot {
    fn drop(&mut self) {
        if let Some(native) = self.native.take() {
            unsafe { remove_layer_view(native) };
        }
    }
}

/// A layer of window content, stacked above the layers added before it. A renderer draws
/// it after taking it with [`Self::attach`], the app moves, sizes and hides it. The layer is
/// removed when its handles and its renderer are dropped.
#[derive(Clone)]
pub struct WindowLayer {
    pub(super) host: Rc<ContentHost>,
    pub(super) layer: Rc<LayerSlot>,
}

impl WindowLayer {
    pub(crate) const fn new(host: Rc<ContentHost>, layer: Rc<LayerSlot>) -> Self {
        Self { host, layer }
    }

    /// Take the layer for a renderer, once.
    pub fn attach(&self) -> Result<WindowAttachment, AttachError> {
        if self.host.closed.get() {
            return Err(AttachError::Closed);
        }
        if self.layer.attached.get() {
            return Err(AttachError::Occupied);
        }
        if self.host.layers().iter().any(|layer| layer.base) {
            return Err(AttachError::Unsupported);
        }
        self.layer.attached.set(true);
        Ok(WindowAttachment {
            host: self.host.clone(),
            layer: self.layer.clone(),
        })
    }

    /// The area of the layer in the window content, or none when it fills the content.
    pub fn frame(&self) -> Option<LogicalRect> {
        self.layer.frame.get()
    }

    /// Place the layer at an area of the window content, which may reach outside it to
    /// scroll, or fill the content with none. Native layers move without being redrawn.
    pub fn set_frame(&self, frame: Option<LogicalRect>) {
        let previous = self.layer.frame.replace(frame);
        if previous == frame || self.host.closed.get() {
            return;
        }
        if let (Some(native), Some(content)) = (self.layer.native.get(), self.host.content.get()) {
            unsafe { set_layer_view_frame(content, native, frame) };
        }
        let size = |frame: Option<LogicalRect>| {
            frame.map_or(self.host.size.get(), |frame| {
                frame.physical_size(self.host.scale_factor.get())
            })
        };
        let (width, height) = size(frame);
        if size(previous) != (width, height) {
            self.layer.resized(width, height);
        }
        self.layer.changed();
    }

    /// Whether the layer is hidden.
    pub fn is_hidden(&self) -> bool {
        self.layer.hidden.get()
    }

    /// Hide or show the layer.
    pub fn set_hidden(&self, hidden: bool) {
        if self.layer.hidden.replace(hidden) != hidden {
            if let Some(native) = self.layer.native.get() {
                unsafe { set_layer_view_hidden(native, hidden) };
            }
            self.layer.changed();
        }
    }

    /// Repaint the layer through its renderer.
    pub fn request_redraw(&self) {
        if !self.host.closed.get() {
            self.layer.request_redraw();
        }
    }

    /// Repaint the layer at the next display frame of the window.
    pub fn request_animation_frame(&self) {
        self.layer.request_animation_frame();
    }
}
