/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Canvases that draw a window or a window layer.

#[cfg(any(windows, target_os = "android"))]
use std::rc::Rc;

use bwindow::WindowLayer;

use crate::CanvasRenderingContext2d;
use crate::platforms::PlatformCanvas;

/// What a canvas draws: the whole window content, or a transparent layer of it.
#[derive(Clone, Copy)]
pub enum CanvasTarget<'a> {
    /// The whole window content, like a single canvas app.
    Window(&'a bwindow::Window),
    /// A layer above the window content and the layers below it.
    Layer(&'a WindowLayer),
}

impl<'a> From<&'a bwindow::Window> for CanvasTarget<'a> {
    fn from(window: &'a bwindow::Window) -> Self {
        Self::Window(window)
    }
}

impl<'a> From<&'a WindowLayer> for CanvasTarget<'a> {
    fn from(layer: &'a WindowLayer) -> Self {
        Self::Layer(layer)
    }
}

/// Builder for one native 2D canvas drawing a window or a window layer.
pub struct CanvasBuilder<'a> {
    target: CanvasTarget<'a>,
}

impl<'a> CanvasBuilder<'a> {
    /// Create a canvas builder for a window, or a layer of it to stack canvases.
    pub fn new(target: impl Into<CanvasTarget<'a>>) -> Self {
        Self {
            target: target.into(),
        }
    }

    /// Build the canvas, panicking if the window is closed or its target has a renderer.
    pub fn build(self) -> Canvas {
        self.try_build().expect("cannot attach canvas")
    }

    /// Build the canvas with a recoverable attachment error.
    pub fn try_build(self) -> Result<Canvas, bwindow::AttachError> {
        let attachment = match self.target {
            CanvasTarget::Window(window) => {
                let attachment = window.attach_content()?;
                return Ok(Canvas {
                    inner: CanvasInner::Native(PlatformCanvas::new(attachment)),
                });
            }
            CanvasTarget::Layer(layer) => layer.attach()?,
        };
        #[cfg(target_vendor = "apple")]
        let inner = CanvasInner::Native(PlatformCanvas::new(attachment));
        #[cfg(not(target_vendor = "apple"))]
        let inner = match unsafe { attachment.content_handle() } {
            #[cfg(target_os = "android")]
            Some(bwindow::NativeContentHandle::AndroidSurface(_)) => {
                CanvasInner::Native(PlatformCanvas::new(attachment))
            }
            #[cfg(not(any(target_os = "android", windows)))]
            Some(bwindow::NativeContentHandle::GdkLayer(_)) => {
                CanvasInner::Layer(crate::platforms::PlatformLayerCanvas::new(attachment))
            }
            #[cfg(windows)]
            Some(bwindow::NativeContentHandle::DirectComposition { .. }) => {
                CanvasInner::Layer(crate::platforms::PlatformLayerCanvas::new(attachment))
            }
            // Without native layers, like on Windows 7 and XP and Android 8 and 9.
            #[cfg(any(windows, target_os = "android"))]
            _ => CanvasInner::Composited(crate::composite::Layer::new(attachment)?),
            // Like X11 without the Composite extension.
            #[cfg(not(any(windows, target_os = "android")))]
            _ => return Err(bwindow::AttachError::Unsupported),
        };
        Ok(Canvas { inner })
    }
}

/// A native 2D canvas. All access is confined to the main thread.
pub struct Canvas {
    inner: CanvasInner,
}

enum CanvasInner {
    // The window content or a native layer, drawn by the platform canvas.
    Native(PlatformCanvas),
    // A native layer of a Wayland or DirectComposition window.
    #[cfg(not(any(target_vendor = "apple", target_os = "android")))]
    Layer(crate::platforms::PlatformLayerCanvas),
    // Without native layers, canvases are layers of a compositor that paints the window.
    #[cfg(any(windows, target_os = "android"))]
    Composited(Rc<crate::composite::Layer>),
}

macro_rules! inner {
    ($canvas:expr, $inner:ident => $body:expr) => {
        match $canvas {
            CanvasInner::Native($inner) => $body,
            #[cfg(not(any(target_vendor = "apple", target_os = "android")))]
            CanvasInner::Layer($inner) => $body,
            #[cfg(any(windows, target_os = "android"))]
            CanvasInner::Composited($inner) => $body,
        }
    };
}

impl Canvas {
    /// Schedule a repaint. Repeated requests coalesce.
    pub fn request_redraw(&self) {
        inner!(&self.inner, inner => inner.request_redraw());
    }

    /// Schedule one repaint at the next native display frame.
    pub fn request_animation_frame(&self) {
        inner!(&self.inner, inner => inner.request_animation_frame());
    }

    /// Paint once during this canvas window's RedrawRequested callback.
    ///
    /// Returns false outside a native paint callback, after close, or if this frame
    /// was already drawn. The closure is not called in those cases. Native frame
    /// scheduling and recovery are handled by the library; callers must not assume
    /// that every redraw produced a completed draw. The context cannot escape the closure.
    ///
    /// Ignoring whether a frame was drawn is diagnosed by Rust:
    ///
    /// ```compile_fail
    /// #![deny(unused_must_use)]
    /// fn paint(canvas: &mut bcanvas::Canvas) {
    ///     canvas.draw(|_| {});
    /// }
    /// ```
    #[must_use = "only update frame-dependent state when the draw closure ran"]
    pub fn draw(
        &mut self,
        draw: impl for<'frame> FnOnce(&mut CanvasRenderingContext2d<'frame>),
    ) -> bool {
        inner!(&mut self.inner, inner => inner.draw(draw))
    }
}
