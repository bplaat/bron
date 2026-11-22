/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The native content host of a window, with its stack of layers.

use super::*;

pub(crate) struct ContentHost {
    pub(super) id: WindowId,
    pub(super) handle: Cell<Option<NativeWindowHandle>>,
    pub(super) content: Cell<Option<NativeContentHandle>>,
    pub(super) content_factory: RefCell<Option<ContentFactory>>,
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    pub(super) native_view: Cell<Option<NativeViewHandle>>,
    pub(super) scale_factor: Cell<f32>,
    // The content size in physical pixels, which layers without a frame have.
    pub(super) size: Cell<(u32, u32)>,
    // The layers from bottom to top.
    pub(super) layers: RefCell<Vec<Weak<LayerSlot>>>,
    // A renderer took the content host, so it draws the whole window surface.
    #[cfg(target_os = "android")]
    pub(super) draws_content: Cell<bool>,
    pub(super) ready: Cell<bool>,
    pub(super) closed: Cell<bool>,
    pub(super) background: Cell<Option<u32>>,
    pub(super) theme: Cell<Option<crate::Theme>>,
    pub(super) allow_file_drop: bool,
    pub(super) frames: RefCell<Option<crate::platforms::FrameSource>>,
    // An animation frame of a window without layers.
    pub(super) animation_frame_pending: Cell<bool>,
    pub(super) pointer_locked: Cell<bool>,
    pub(super) pointer_position: Cell<Option<LogicalPoint>>,
    pub(super) touch_mouse: RefCell<crate::touch::TouchMouse>,
}

impl ContentHost {
    pub(crate) fn new(id: WindowId, background: Option<u32>, allow_file_drop: bool) -> Rc<Self> {
        Rc::new(Self {
            id,
            handle: Cell::new(None),
            content: Cell::new(None),
            content_factory: RefCell::new(None),
            #[cfg(not(any(target_os = "ios", target_os = "android")))]
            native_view: Cell::new(None),
            scale_factor: Cell::new(1.0),
            size: Cell::new((0, 0)),
            layers: RefCell::new(Vec::new()),
            #[cfg(target_os = "android")]
            draws_content: Cell::new(false),
            ready: Cell::new(!cfg!(target_os = "ios")),
            closed: Cell::new(false),
            background: Cell::new(background),
            theme: Cell::new(None),
            allow_file_drop,
            frames: RefCell::new(None),
            animation_frame_pending: Cell::new(false),
            pointer_locked: Cell::new(false),
            pointer_position: Cell::new(None),
            touch_mouse: RefCell::new(crate::touch::TouchMouse::default()),
        })
    }

    #[cfg(any(target_os = "ios", test))]
    pub(crate) fn ready(&self) {
        if self.closed.get() || self.ready.replace(true) {
            return;
        }
        for layer in self.layers() {
            let handler = layer.ready_handler.borrow_mut().take();
            if let Some(handler) = handler {
                handler();
            }
        }
    }

    /// The live layers from bottom to top.
    pub(super) fn layers(&self) -> Vec<Rc<LayerSlot>> {
        let mut layers = self.layers.borrow_mut();
        layers.retain(|layer| layer.strong_count() > 0);
        layers.iter().filter_map(Weak::upgrade).collect()
    }

    /// Add a layer above the existing ones.
    pub(crate) fn add_layer(self: &Rc<Self>) -> Rc<LayerSlot> {
        let layer = self.slot(false);
        self.layers.borrow_mut().push(Rc::downgrade(&layer));
        layer
    }

    /// Take the window content below every layer for a renderer.
    pub(crate) fn attach_content(self: &Rc<Self>) -> Result<WindowAttachment, AttachError> {
        if self.closed.get() {
            return Err(AttachError::Closed);
        }
        let layers = self.layers();
        if layers.iter().any(|layer| layer.base) {
            return Err(AttachError::Occupied);
        }
        if layers.iter().any(|layer| layer.attached.get()) {
            return Err(AttachError::Unsupported);
        }
        let layer = self.slot(true);
        layer.attached.set(true);
        self.layers.borrow_mut().insert(0, Rc::downgrade(&layer));
        Ok(WindowAttachment {
            host: self.clone(),
            layer,
        })
    }

    pub(super) fn slot(self: &Rc<Self>, base: bool) -> Rc<LayerSlot> {
        Rc::new(LayerSlot {
            host: Rc::downgrade(self),
            base,
            native: Cell::new(None),
            frame: Cell::new(None),
            hidden: Cell::new(false),
            attached: Cell::new(false),
            ready_handler: RefCell::new(None),
            resize: RefCell::new(None),
            scale: RefCell::new(None),
            close: RefCell::new(None),
            redraw: RefCell::new(None),
            #[cfg(windows)]
            paint: RefCell::new(None),
            change: RefCell::new(None),
            animation_frame_pending: Cell::new(false),
        })
    }

    pub(crate) fn set_handle(&self, handle: NativeWindowHandle) {
        self.handle.set(Some(handle));
    }

    /// Set the content host that exists for the whole window lifetime.
    #[cfg(any(target_vendor = "apple", target_os = "android", windows))]
    pub(crate) fn set_content(&self, content: NativeContentHandle) {
        self.content.set(Some(content));
    }

    /// Create the content host on first use, for hosts that alternative window children
    /// like a web view must not get.
    #[cfg_attr(
        any(target_vendor = "apple", target_os = "android", windows),
        allow(dead_code, reason = "only GTK creates its content host on demand")
    )]
    pub(crate) fn set_content_factory(
        &self,
        factory: impl FnOnce() -> Option<NativeContentHandle> + 'static,
    ) {
        *self.content_factory.borrow_mut() = Some(Box::new(factory));
    }

    pub(super) fn content_handle(&self) -> Option<NativeContentHandle> {
        if self.closed.get() {
            return None;
        }
        if self.content.get().is_none() {
            let factory = self.content_factory.borrow_mut().take();
            if let Some(factory) = factory {
                self.content.set(factory());
            }
        }
        self.content.get()
    }

    /// The native child view of the window, which gets the focus of the window.
    #[cfg(windows)]
    pub(crate) const fn native_view(&self) -> Option<*mut c_void> {
        match self.native_view.get() {
            Some(NativeViewHandle::Win32(view)) => Some(view),
            _ => None,
        }
    }

    pub(crate) const fn id(&self) -> WindowId {
        self.id
    }

    pub(crate) const fn is_closed(&self) -> bool {
        self.closed.get()
    }

    pub(crate) const fn pointer_locked(&self) -> bool {
        self.pointer_locked.get()
    }

    pub(crate) fn set_pointer_locked(self: &Rc<Self>, locked: bool) {
        if self.pointer_locked.replace(locked) == locked {
            return;
        }
        if !locked {
            self.pointer_position.set(None);
        }
        self.event_sender().send(WindowEvent::PointerLockChange);
    }

    pub(crate) fn set_background(&self, color: u32) {
        self.background.set(Some(color));
    }

    /// The configured background color.
    #[cfg(target_os = "android")]
    pub(crate) const fn background(&self) -> Option<u32> {
        self.background.get()
    }

    /// Whether a renderer draws the window surface, content in its own child window like a
    /// web view leaves painting the background to the window.
    #[cfg(target_os = "android")]
    pub(crate) const fn draws_content(&self) -> bool {
        self.draws_content.get()
    }

    pub(crate) fn theme_changed(self: &Rc<Self>, theme: crate::Theme) {
        if !self.closed.get() && self.theme.replace(Some(theme)) != Some(theme) {
            self.event_sender().send(WindowEvent::ThemeChanged(theme));
        }
    }

    /// Take the window content, like a content backend.
    #[cfg(test)]
    pub(crate) fn attach(self: &Rc<Self>) -> Result<WindowAttachment, AttachError> {
        self.attach_content()
    }

    pub(crate) fn event_sender(self: &Rc<Self>) -> WindowEventSender {
        WindowEventSender {
            host: Rc::downgrade(self),
            layer: Weak::new(),
        }
    }

    pub(crate) fn resize(&self, width: u32, height: u32) {
        if self.closed.get() {
            return;
        }
        self.size.set((width, height));
        // AppKit, UIKit and GTK lay out the native view themselves.
        #[cfg(windows)]
        if let Some(view) = self.native_view() {
            crate::platforms::resize_native_view(view, width, height);
        }
        for layer in self.layers() {
            // Native layer views follow the content, which AppKit and UIKit do themselves.
            if let (Some(native), Some(content)) = (layer.native.get(), self.content.get()) {
                unsafe { set_layer_view_frame(content, native, layer.frame.get()) };
            }
            if layer.frame.get().is_none() {
                layer.resized(width, height);
            }
        }
    }

    /// Report a display scale change, returning whether the value changed.
    pub(crate) fn scale_factor_changed(&self, scale_factor: f32) -> bool {
        if self.closed.get() || self.scale_factor.replace(scale_factor) == scale_factor {
            return false;
        }
        for layer in self.layers() {
            let handler = layer.scale.borrow().clone();
            if let Some(handler) = handler {
                handler(scale_factor);
            }
            // Framed layers have a new size in physical pixels.
            if let Some(frame) = layer.frame.get() {
                let (width, height) = frame.physical_size(scale_factor);
                layer.resized(width, height);
            }
        }
        true
    }

    /// Run the native paint passes of the layers from bottom to top, returns false when no
    /// content paints.
    #[cfg(windows)]
    pub(crate) fn paint(&self) -> bool {
        if self.closed.get() {
            return false;
        }
        let mut painted = false;
        for layer in self.layers() {
            let paint = layer.paint.borrow().clone();
            if let Some(paint) = paint {
                paint();
                painted = true;
            }
        }
        painted
    }

    /// Whether content paints the client area itself.
    #[cfg(windows)]
    pub(crate) fn has_paint(&self) -> bool {
        self.layers()
            .iter()
            .any(|layer| layer.paint.borrow().is_some())
    }

    pub(crate) fn close(&self) {
        if self.closed.replace(true) {
            return;
        }
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        if self.pointer_locked.replace(false) {
            crate::platforms::release_pointer_lock();
        }
        self.pointer_position.set(None);
        self.handle.set(None);
        self.content.set(None);
        let factory = self.content_factory.borrow_mut().take();
        drop(factory);
        let frames = self.frames.borrow_mut().take();
        drop(frames);
        for layer in self.layers() {
            if let Some(close) = layer.disconnect() {
                close();
            }
        }
    }

    /// Repaint every layer through its repaint scheduler, or deliver the redraw directly.
    pub(crate) fn request_redraw(self: &Rc<Self>) {
        if self.closed.get() {
            return;
        }
        let layers = self.layers();
        let redraws: Vec<_> = layers
            .iter()
            .filter_map(|layer| layer.redraw.borrow().clone())
            .collect();
        // Layers without a repaint scheduler draw in a delivered redraw.
        if redraws.len() < layers.len() || layers.is_empty() {
            self.deliver_redraw();
        }
        for redraw in redraws {
            redraw();
        }
    }

    // Deliver a redraw without a repaint scheduler.
    pub(super) fn deliver_redraw(self: &Rc<Self>) {
        #[cfg(windows)]
        if let Some(NativeWindowHandle::Win32(hwnd)) = self.handle.get() {
            // Posted animation messages precede hardware input. Render through
            // WM_PAINT so a slow continuous renderer cannot starve that input.
            use crate::platforms::windows::headers::{FALSE, InvalidateRect};
            unsafe { InvalidateRect(hwnd, std::ptr::null_mut(), FALSE) };
            return;
        }
        self.event_sender().send(WindowEvent::RedrawRequested);
    }

    /// Redraw every layer at the next display frame of the window. The window paces the
    /// frames, so content only needs a repaint scheduler.
    pub(crate) fn request_animation_frame(self: &Rc<Self>) {
        if self.closed.get() {
            return;
        }
        let layers = self.layers();
        if layers.is_empty() {
            if !self.animation_frame_pending.replace(true) && !self.schedule_frame() {
                self.animation_frame_pending.set(false);
            }
        } else {
            for layer in layers {
                layer.request_animation_frame();
            }
        }
    }

    // Ask the frame source for the next display frame, false without one.
    pub(super) fn schedule_frame(self: &Rc<Self>) -> bool {
        let mut frames = self.frames.borrow_mut();
        if frames.is_none() {
            *frames = self
                .handle
                .get()
                .and_then(|handle| crate::platforms::FrameSource::new(handle, Rc::downgrade(self)));
        }
        frames.as_ref().is_some_and(|frames| {
            frames.request();
            true
        })
    }

    /// Deliver the requested animation frames at a display frame, any redraw delivered
    /// before already satisfied them.
    pub(crate) fn animation_frame(self: &Rc<Self>) {
        if self.closed.get() {
            return;
        }
        if self.animation_frame_pending.replace(false) {
            self.deliver_redraw();
        }
        for layer in self.layers() {
            if layer.animation_frame_pending.replace(false) {
                layer.request_redraw();
            }
        }
    }

    /// Fill in the pointer movement since the previous event. While locked the position
    /// stays put and native relative motion is passed through.
    pub(super) fn track_pointer(&self, event: WindowEvent) -> WindowEvent {
        match event {
            WindowEvent::MouseMove {
                mut position,
                movement,
                modifiers,
            } => {
                let previous = self.pointer_position.get();
                let movement = if self.pointer_locked.get() {
                    if let Some(previous) = previous {
                        position = previous;
                    }
                    movement
                } else {
                    previous.map_or(LogicalPoint::default(), |previous| {
                        LogicalPoint::new(position.x - previous.x, position.y - previous.y)
                    })
                };
                self.pointer_position.set(Some(position));
                WindowEvent::MouseMove {
                    position,
                    movement,
                    modifiers,
                }
            }
            WindowEvent::MouseDown(ref mouse) | WindowEvent::MouseUp(ref mouse) => {
                self.pointer_position.set(Some(mouse.position));
                event
            }
            WindowEvent::MouseLeave | WindowEvent::Blur => {
                self.pointer_position.set(None);
                event
            }
            _ => event,
        }
    }
}
