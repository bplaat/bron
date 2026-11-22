/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Delivering events of a window or one of its layers.

use super::*;

/// Sends native content events without keeping its window alive. A sender of a layer
/// redraws that layer, others the whole window.
#[derive(Clone)]
pub struct WindowEventSender {
    pub(super) host: Weak<ContentHost>,
    pub(super) layer: Weak<LayerSlot>,
}

impl WindowEventSender {
    /// Redraw at the next display frame of the window, through the repaint scheduler.
    pub fn request_animation_frame(&self) {
        if let Some(layer) = self.layer.upgrade() {
            layer.request_animation_frame();
        } else if let Some(host) = self.host.upgrade() {
            host.request_animation_frame();
        }
    }

    /// Deliver a native paint event synchronously, without queuing a borrowed frame.
    ///
    /// Returns false if the loop is busy, not running, or the window is closed.
    /// In that case the backend must schedule a later repaint. On success,
    /// `finish` ends the native frame before queued input/user events are delivered.
    pub fn redraw(&self, finish: impl FnOnce()) -> bool {
        if let Some(host) = self.host.upgrade()
            && !host.closed.get()
        {
            // A delivered redraw satisfies a pending animation frame, content that could not
            // deliver requests a new one.
            match self.layer.upgrade() {
                Some(layer) => layer.animation_frame_pending.set(false),
                None => host.animation_frame_pending.set(false),
            }
            return crate::dispatch::try_paint(
                crate::Event::Window(host.id, WindowEvent::RedrawRequested),
                finish,
            );
        }
        false
    }

    /// Deliver an event while the owning window is open; discard late events.
    pub fn send(&self, event: WindowEvent) {
        if let Some(host) = self.host.upgrade()
            && !host.closed.get()
        {
            if matches!(
                &event,
                WindowEvent::TouchStart(_)
                    | WindowEvent::TouchMove(_)
                    | WindowEvent::TouchEnd(_)
                    | WindowEvent::TouchCancel(_)
            ) {
                let touch = match &event {
                    WindowEvent::TouchStart(touch) => WindowEvent::TouchStart(touch.clone()),
                    WindowEvent::TouchMove(touch) => WindowEvent::TouchMove(touch.clone()),
                    WindowEvent::TouchEnd(touch) => WindowEvent::TouchEnd(touch.clone()),
                    WindowEvent::TouchCancel(touch) => WindowEvent::TouchCancel(touch.clone()),
                    _ => unreachable!(),
                };
                let sender = self.clone();
                crate::dispatch::send_then(crate::Event::Window(host.id, event), move || {
                    if let Some(host) = sender.host.upgrade()
                        && !host.closed.get()
                    {
                        let events = host.touch_mouse.borrow_mut().events(&touch);
                        for event in events {
                            sender.send(event);
                        }
                    }
                });
                return;
            }
            let event = host.track_pointer(event);
            let event = crate::Event::Window(host.id, event);
            if matches!(
                &event,
                crate::Event::Window(_, WindowEvent::RedrawRequested)
            ) {
                let (weak, layer) = (Rc::downgrade(&host), self.layer.clone());
                crate::dispatch::send_if(event, move || {
                    let Some(host) = weak.upgrade().filter(|host| !host.closed.get()) else {
                        return false;
                    };
                    // Content may have attached since this event was queued. Only its
                    // native paint callbacks can provide a valid borrowed frame.
                    let layers = layer
                        .upgrade()
                        .map_or_else(|| host.layers(), |layer| vec![layer]);
                    let redraws: Vec<_> = layers
                        .iter()
                        .filter_map(|layer| layer.redraw.borrow().clone())
                        .collect();
                    let unscheduled = redraws.len() < layers.len() || layers.is_empty();
                    for redraw in redraws {
                        redraw();
                    }
                    if !unscheduled {
                        return false;
                    }
                    host.animation_frame_pending.set(false);
                    for layer in layers {
                        if layer.redraw.borrow().is_none() {
                            layer.animation_frame_pending.set(false);
                        }
                    }
                    true
                });
            } else {
                crate::dispatch::send(event);
            }
        }
    }
}
