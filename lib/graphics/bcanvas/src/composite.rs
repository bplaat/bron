/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// Window layers of canvases without native layers, like on Windows 7 and XP and Android 8
// and 9. One native
// canvas paints the window: a lone visible layer filling the window draws into its frame
// directly, else every layer records its drawing, which the painter replays in stack order
// at the end of the frame. Moving or hiding a layer only replays the recordings again.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use bwindow::{AttachError, WindowAttachment, WindowId};

use crate::platforms::PlatformCanvas;
use crate::{CanvasRenderingContext2d, Color, OffscreenCanvas};

thread_local! {
    #[allow(clippy::missing_const_for_thread_local, reason = "false positive without native TLS")]
    static COMPOSITORS: RefCell<Vec<(WindowId, Weak<Compositor>)>> =
        const { RefCell::new(Vec::new()) };
}

struct Compositor {
    // The canvas painting the window, which replays the layers at the end of its frames.
    painter: RefCell<PlatformCanvas>,
    layers: RefCell<Vec<Weak<Layer>>>,
}

impl Compositor {
    // The compositor of the window of an attachment, which paints a layer of its own.
    fn get(attachment: &WindowAttachment) -> Result<Rc<Self>, AttachError> {
        let window = attachment.window_id();
        let existing = COMPOSITORS.with_borrow_mut(|compositors| {
            compositors.retain(|(_, compositor)| compositor.strong_count() > 0);
            compositors
                .iter()
                .find(|(id, _)| *id == window)
                .and_then(|(_, compositor)| compositor.upgrade())
        });
        if let Some(compositor) = existing {
            return Ok(compositor);
        }
        let painter = PlatformCanvas::new(attachment.add_layer().attach()?);
        let compositor = Rc::new_cyclic(|weak: &Weak<Self>| {
            let weak = weak.clone();
            painter.on_frame_end(move || {
                if let Some(compositor) = weak.upgrade() {
                    compositor.replay();
                }
            });
            Self {
                painter: RefCell::new(painter),
                layers: RefCell::new(Vec::new()),
            }
        });
        COMPOSITORS
            .with_borrow_mut(|compositors| compositors.push((window, Rc::downgrade(&compositor))));
        Ok(compositor)
    }

    // The live layers from bottom to top.
    fn layers(&self) -> Vec<Rc<Layer>> {
        let mut layers: Vec<_> = self
            .layers
            .borrow()
            .iter()
            .filter_map(Weak::upgrade)
            .collect();
        layers.sort_by_key(|layer| layer.attachment.stack_position());
        layers
    }

    // The layer that draws into the native frame itself: the only visible one, filling the
    // window.
    fn direct(&self) -> Option<Rc<Layer>> {
        let mut visible = self
            .layers()
            .into_iter()
            .filter(|layer| !layer.attachment.is_hidden());
        match (visible.next(), visible.next()) {
            (Some(layer), None) if layer.attachment.frame().is_none() => Some(layer),
            _ => None,
        }
    }

    fn request_redraw(&self) {
        self.painter.borrow().request_redraw();
    }

    // Replay the recordings at the end of a frame that no layer drew into directly.
    fn replay(&self) {
        let layers = self.layers();
        // The window background can change, like with the theme.
        let background = layers
            .first()
            .and_then(|layer| layer.attachment.background_color());
        let mut missing = false;
        let mut painter = self.painter.borrow_mut();
        let _ = painter.draw(|context| {
            let (width, height) = (context.width(), context.height());
            match background {
                Some(color) => {
                    context.set_fill_style(Color::from_rgb(color));
                    context.fill_rect(0.0, 0.0, width, height);
                }
                None => context.clear_rect(0.0, 0.0, width, height),
            }
            for layer in layers.iter().filter(|layer| !layer.attachment.is_hidden()) {
                let recording = layer.recording.borrow();
                let Some(recording) = recording.as_ref() else {
                    // Like a layer that drew directly before, it records at the next frame.
                    layer.dirty.set(true);
                    missing = true;
                    continue;
                };
                let (x, y) = layer
                    .attachment
                    .frame()
                    .map_or((0.0, 0.0), |frame| (frame.x, frame.y));
                // Each layer composites as a group of its own, so its clears and composite
                // operations only change the layer, like a native one.
                context.begin_layer();
                context.draw_canvas(
                    recording,
                    x,
                    y,
                    recording.logical_width(),
                    recording.logical_height(),
                );
                context.end_layer();
            }
        });
        if missing {
            painter.request_animation_frame();
        }
    }
}

/// A canvas layer of a window without native layers.
pub(crate) struct Layer {
    compositor: Rc<Compositor>,
    attachment: WindowAttachment,
    dirty: Cell<bool>,
    recording: RefCell<Option<OffscreenCanvas>>,
}

impl Layer {
    pub(crate) fn new(attachment: WindowAttachment) -> Result<Rc<Self>, AttachError> {
        let compositor = Compositor::get(&attachment)?;
        let layer = Rc::new(Self {
            compositor,
            attachment,
            dirty: Cell::new(true),
            recording: RefCell::new(None),
        });
        let redraw = || {
            let layer = Rc::downgrade(&layer);
            move || {
                if let Some(layer) = layer.upgrade() {
                    layer.request_redraw();
                }
            }
        };
        layer.attachment.on_redraw(redraw());
        let resize = redraw();
        layer.attachment.on_resize(move |_, _| resize());
        let scale = redraw();
        layer.attachment.on_scale_factor_changed(move |_| scale());
        let changed = Rc::downgrade(&layer.compositor);
        layer.attachment.on_layer_changed(move || {
            if let Some(compositor) = changed.upgrade() {
                compositor.request_redraw();
            }
        });
        layer
            .compositor
            .layers
            .borrow_mut()
            .push(Rc::downgrade(&layer));
        layer.compositor.request_redraw();
        Ok(layer)
    }

    pub(crate) fn request_redraw(&self) {
        self.dirty.set(true);
        self.compositor.request_redraw();
    }

    pub(crate) fn request_animation_frame(&self) {
        self.dirty.set(true);
        self.attachment.request_animation_frame();
    }

    pub(crate) fn draw(
        self: &Rc<Self>,
        draw: impl for<'frame> FnOnce(&mut CanvasRenderingContext2d<'frame>),
    ) -> bool {
        if self.attachment.is_closed() {
            return false;
        }
        let compositor = &self.compositor;
        let Some((width, height)) = compositor.painter.borrow().frame_size() else {
            return false;
        };
        // A lone layer draws every native frame itself, without a recording.
        if compositor
            .direct()
            .is_some_and(|direct| Rc::ptr_eq(&direct, self))
        {
            self.recording.borrow_mut().take();
            self.dirty.set(false);
            return compositor.painter.borrow_mut().draw(draw);
        }
        if !self.dirty.replace(false) {
            return false;
        }
        let (width, height) = self
            .attachment
            .frame()
            .map_or((width, height), |frame| (frame.width, frame.height));
        let mut recording = OffscreenCanvas::new_recording(width.max(0.0), height.max(0.0));
        recording.draw(draw);
        *self.recording.borrow_mut() = Some(recording);
        true
    }
}

impl Drop for Layer {
    fn drop(&mut self) {
        self.attachment.disconnect();
        // The other layers draw without this one.
        self.compositor.request_redraw();
    }
}
