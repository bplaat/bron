/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The exclusive attachment of a renderer to the window content or a layer.

use super::*;

/// An exclusive renderer attachment to a window layer, whose hooks disconnect when it or
/// its window is dropped.
pub struct WindowAttachment {
    pub(super) host: Rc<ContentHost>,
    pub(super) layer: Rc<LayerSlot>,
}

impl WindowAttachment {
    /// The owning window's identifier.
    pub fn window_id(&self) -> WindowId {
        self.host.id
    }

    /// Whether the native window has closed.
    pub fn is_closed(&self) -> bool {
        self.host.closed.get()
    }

    /// Add another layer above the existing ones of the window, like a renderer that draws
    /// its layers into one native host where the platform has no native layers.
    pub fn add_layer(&self) -> WindowLayer {
        WindowLayer::new(self.host.clone(), self.host.add_layer())
    }

    /// The area of the layer in the window content, or none when it fills the content.
    pub fn frame(&self) -> Option<LogicalRect> {
        self.layer.frame.get()
    }

    /// Whether the layer has a native view of its own, which the system composites with
    /// the other layers. Without one it draws the shared content host of the window.
    pub fn is_native_layer(&self) -> bool {
        self.layer.content_handle(&self.host);
        self.layer.native.get().is_some()
    }

    /// The size of the layer in physical pixels.
    pub fn size(&self) -> (u32, u32) {
        self.layer
            .frame
            .get()
            .map_or(self.host.size.get(), |frame| {
                frame.physical_size(self.host.scale_factor.get())
            })
    }

    /// Whether the layer is hidden.
    pub fn is_hidden(&self) -> bool {
        self.layer.hidden.get()
    }

    /// The position of the layer in the stack of the window, counted from the bottom.
    pub fn stack_position(&self) -> usize {
        self.host
            .layers()
            .iter()
            .position(|layer| Rc::ptr_eq(layer, &self.layer))
            .unwrap_or(usize::MAX)
    }

    /// Register a callback for changes of the frame or visibility of the layer.
    pub fn on_layer_changed(&self, handler: impl Fn() + 'static) {
        assert!(!self.is_closed(), "window is closed");
        let previous = self.layer.change.borrow_mut().replace(Rc::new(handler));
        drop(previous);
    }

    /// Access the native top-level window, for content that adds its own window child
    /// like a web view.
    ///
    /// # Safety
    /// Use the handle only on the main thread while this attachment is open.
    /// Do not destroy the native window or replace its delegate/user data.
    pub unsafe fn window_handle(&self) -> Option<NativeWindowHandle> {
        self.host.handle.get()
    }

    /// Access the native host of the layer that renderers draw into, creating it on first
    /// use: a view of its own on platforms with native layers, else the window content host.
    /// The window delivers its interaction, so renderers only draw and present.
    ///
    /// # Safety
    /// Use the handle only on the main thread while this attachment is open.
    /// Do not destroy the host, replace its delegate/user data, or handle its input.
    pub unsafe fn content_handle(&self) -> Option<NativeContentHandle> {
        let handle = self.layer.content_handle(&self.host);
        // A renderer of the window surface draws the window instead of its background.
        #[cfg(target_os = "android")]
        if matches!(handle, Some(NativeContentHandle::Android(_))) {
            self.host.draws_content.set(true);
        }
        handle
    }

    /// Fill the window content area with a native view instead of the content host, the
    /// window keeps it sized to the content area and focused while the window is active.
    /// The view handles its own input, so the window does not report it.
    ///
    /// # Safety
    /// `view` must be a live native view of the window's platform, a Win32 view must be a
    /// child window of [`Self::window_handle`]. Use it only on the main thread and keep it
    /// alive while this attachment is open.
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    pub unsafe fn set_native_view(&self, view: NativeViewHandle) {
        assert!(!self.is_closed(), "window is closed");
        assert!(
            self.host.native_view.replace(Some(view)).is_none(),
            "window already has a native view"
        );
        // The content host is never created when a native view fills the window.
        let factory = self.host.content_factory.borrow_mut().take();
        drop(factory);
        let window = self.host.handle.get().expect("window has no native handle");
        unsafe { crate::platforms::embed_native_view(window, self.host.content.get(), view) };
    }

    /// Native pixels per logical pixel of the window content.
    pub fn scale_factor(&self) -> f32 {
        self.host.scale_factor.get()
    }

    /// The window's configured backing color.
    pub fn background_color(&self) -> Option<u32> {
        self.host.background.get()
    }

    /// Whether the window accepts file drops.
    pub fn allow_file_drop(&self) -> bool {
        self.host.allow_file_drop
    }

    /// Whether native content can be created for this window.
    pub fn is_ready(&self) -> bool {
        self.host.ready.get() && !self.is_closed()
    }

    /// Run once when native content can be created, immediately if already ready.
    /// Pending callbacks are canceled when the attachment disconnects.
    pub fn on_ready(&self, handler: impl FnOnce() + 'static) {
        assert!(!self.is_closed(), "window is closed");
        if self.host.ready.get() {
            handler();
        } else {
            let previous = self
                .layer
                .ready_handler
                .borrow_mut()
                .replace(Box::new(handler));
            drop(previous);
        }
    }

    /// Register a callback for native client-area size changes in physical pixels.
    pub fn on_resize(&self, handler: impl Fn(u32, u32) + 'static) {
        assert!(!self.is_closed(), "window is closed");
        let previous = self.layer.resize.borrow_mut().replace(Rc::new(handler));
        drop(previous);
    }

    /// Register a callback for display scale changes of the window content.
    pub fn on_scale_factor_changed(&self, handler: impl Fn(f32) + 'static) {
        assert!(!self.is_closed(), "window is closed");
        let previous = self.layer.scale.borrow_mut().replace(Rc::new(handler));
        drop(previous);
    }

    /// Register teardown to run once when the owning window closes.
    pub fn on_close(&self, handler: impl FnOnce() + 'static) {
        assert!(!self.is_closed(), "window is closed");
        let previous = self.layer.close.borrow_mut().replace(Box::new(handler));
        drop(previous);
    }

    /// Register the content backend's coalescing native repaint scheduler, without one
    /// redraws are delivered as `RedrawRequested`. Queued redraws also use this
    /// scheduler if it is registered before delivery, so they cannot bypass a native frame.
    pub fn on_redraw(&self, handler: impl Fn() + 'static) {
        assert!(!self.is_closed(), "window is closed");
        let previous = self.layer.redraw.borrow_mut().replace(Rc::new(handler));
        drop(previous);
    }

    /// Register the native paint pass for content drawing into the client area of the
    /// top-level window. It runs for every WM_PAINT after the window validated the
    /// update region, so it must not invalidate the window unless it wants another frame.
    #[cfg(windows)]
    pub fn windows_on_paint(&self, handler: impl Fn() + 'static) {
        assert!(!self.is_closed(), "window is closed");
        let previous = self.layer.paint.borrow_mut().replace(Rc::new(handler));
        drop(previous);
    }

    /// Redraw the layer at the next display frame of the window, through the repaint
    /// scheduler.
    pub fn request_animation_frame(&self) {
        self.layer.request_animation_frame();
    }

    /// Create a weak sender for native content input and file-drop callbacks, whose
    /// redraws are those of this layer.
    pub fn event_sender(&self) -> WindowEventSender {
        WindowEventSender {
            host: Rc::downgrade(&self.host),
            layer: Rc::downgrade(&self.layer),
        }
    }

    /// Disconnect callbacks before releasing the native content resources.
    pub fn disconnect(&self) {
        let close = self.layer.disconnect();
        drop(close);
    }

    /// Get the application's existing Windows data directory.
    #[cfg(windows)]
    pub fn windows_data_directory(&self) -> std::path::PathBuf {
        crate::platforms::windows_config_dir()
    }
}

impl Drop for WindowAttachment {
    fn drop(&mut self) {
        self.disconnect();
    }
}
