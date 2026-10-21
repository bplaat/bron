/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::Cell;
use std::rc::{Rc, Weak};

use bwindow::ffi::*;
use bwindow::{WindowAttachment, WindowEventSender};

use self::context::{Clear, Jni, Resources};
use crate::CanvasRenderingContext2d;
use crate::path::Transform;
pub(crate) use crate::platforms::android::context::PlatformCanvasContext;
pub(crate) use crate::platforms::android::offscreen::PlatformOffscreenCanvas;

mod context;
mod offscreen;

#[derive(Clone, Copy)]
struct Frame {
    width: f32,
    height: f32,
    scale: f32,
    offset: (f32, f32),
}

// Draws the window surface with a hardware accelerated android.graphics.Canvas on the
// Rust main thread, frames are scheduled with the display choreographer.
struct SurfaceState {
    sender: WindowEventSender,
    resources: Rc<Resources>,
    frame: Cell<Option<Frame>>,
    closed: Cell<bool>,
    painting: Cell<bool>,
    scheduled: Cell<bool>,
    animation_frame: Cell<bool>,
}

impl SurfaceState {
    fn request_redraw(self: &Rc<Self>) {
        if self.closed.get() {
            return;
        }
        // Redraws requested while painting wait for the next display frame.
        if self.painting.get() {
            self.request_animation_frame();
        } else if !self.scheduled.replace(true) {
            let state = Rc::downgrade(self);
            android_post_local(move || paint(&state));
        }
    }

    fn request_animation_frame(self: &Rc<Self>) {
        if self.closed.get() || self.animation_frame.replace(true) {
            return;
        }
        let state = Rc::downgrade(self);
        android_post_frame(move || {
            if let Some(state) = state.upgrade() {
                state.animation_frame.set(false);
            }
            paint(&state);
        });
    }
}

fn paint(state: &Weak<SurfaceState>) {
    let Some(state) = state.upgrade() else {
        return;
    };
    state.scheduled.set(false);
    if state.closed.get() || state.painting.get() || android_surface().is_null() {
        return;
    }
    let (x, y, width, height) = android_content_rect();
    let scale = android_density();
    state.painting.set(true);
    state.frame.set(Some(Frame {
        width: width as f32 / scale,
        height: height as f32 / scale,
        scale,
        offset: (x as f32, y as f32),
    }));
    if !state.sender.redraw(|| state.frame.set(None)) {
        state.frame.set(None);
        state.request_animation_frame();
    }
    state.painting.set(false);
}

pub(crate) struct PlatformCanvas {
    state: Rc<SurfaceState>,
    attachment: WindowAttachment,
}

impl PlatformCanvas {
    pub(crate) fn new(attachment: WindowAttachment) -> Self {
        let state = Rc::new(SurfaceState {
            sender: attachment.event_sender(),
            resources: Resources::new(JniEnv::current()),
            frame: Cell::new(None),
            closed: Cell::new(false),
            painting: Cell::new(false),
            scheduled: Cell::new(false),
            animation_frame: Cell::new(false),
        });
        let redraw = Rc::downgrade(&state);
        attachment.on_redraw(move || {
            if let Some(state) = redraw.upgrade() {
                state.request_redraw();
            }
        });
        let animation_frame = Rc::downgrade(&state);
        attachment.on_animation_frame(move || {
            if let Some(state) = animation_frame.upgrade() {
                state.request_animation_frame();
            }
        });
        let resize = Rc::downgrade(&state);
        attachment.on_resize(move |_, _| {
            if let Some(state) = resize.upgrade() {
                state.request_redraw();
            }
        });
        let close = Rc::downgrade(&state);
        attachment.on_close(move || {
            if let Some(state) = close.upgrade() {
                state.closed.set(true);
            }
        });
        state.request_redraw();
        Self { state, attachment }
    }

    pub(crate) fn request_redraw(&self) {
        self.state.request_redraw();
    }

    pub(crate) fn request_animation_frame(&self) {
        self.state.request_animation_frame();
    }

    // Phones have no pointer cursor.
    #[allow(
        clippy::missing_const_for_fn,
        reason = "keeps Canvas::set_cursor non-const"
    )]
    pub(crate) fn set_cursor(&mut self, _cursor: crate::CursorIcon) {}

    pub(crate) fn draw(
        &mut self,
        draw: impl for<'frame> FnOnce(&mut CanvasRenderingContext2d<'frame>),
    ) -> bool {
        let surface = android_surface();
        if self.attachment.is_closed() || surface.is_null() {
            return false;
        }
        let Some(frame) = self.state.frame.take() else {
            return false;
        };
        let env = JniEnv::current();
        let jni = Jni::get(env);
        let background = self
            .attachment
            .background_color()
            .unwrap_or_else(android_default_background)
            | 0xff00_0000;
        unsafe {
            let canvas = env.call_object(surface, jni.lock_hardware_canvas, &[]);
            // Also fill the system bar areas around the content.
            env.call_void(
                canvas,
                jni.draw_color,
                &[
                    jvalue {
                        i: background as jint,
                    },
                    jvalue {
                        l: jni.mode_src.as_raw(),
                    },
                ],
            );
            let base = Transform([
                frame.scale,
                0.0,
                0.0,
                frame.scale,
                frame.offset.0,
                frame.offset.1,
            ]);
            let mut context = CanvasRenderingContext2d::new(
                PlatformCanvasContext::new(
                    canvas,
                    self.state.resources.clone(),
                    base,
                    Clear::Color(background),
                ),
                frame.width,
                frame.height,
                frame.scale,
            );
            draw(&mut context);
            drop(context);
            env.call_void(surface, jni.unlock_canvas_and_post, &[jvalue { l: canvas }]);
            env.delete_local_ref(canvas);
        }
        true
    }
}

impl Drop for PlatformCanvas {
    fn drop(&mut self) {
        self.attachment.disconnect();
        self.state.closed.set(true);
    }
}
