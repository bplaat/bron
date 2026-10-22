/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::Cell;
use std::rc::{Rc, Weak};

use bwindow::ffi::{
    android_default_background, android_density, android_post_frame, android_post_local,
    android_surface, android_window_size,
};
use bwindow::{NativeContentHandle, WindowAttachment, WindowEventSender};
use jni::sys::{jint, jvalue};

use crate::CanvasRenderingContext2d;
use crate::path::Transform;
use crate::platforms::android::context::{
    Clear, Jni, OBJECT, PlatformCanvasContext, Resources, call_void, with_env,
};

#[derive(Clone, Copy)]
struct Frame {
    width: f32,
    height: f32,
    scale: f32,
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
    if state.closed.get() || state.painting.get() || android_surface().is_none() {
        return;
    }
    let (width, height) = android_window_size();
    let scale = android_density();
    state.painting.set(true);
    state.frame.set(Some(Frame {
        width: width as f32 / scale,
        height: height as f32 / scale,
        scale,
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
        let Some(NativeContentHandle::Android(_)) = (unsafe { attachment.content_handle() }) else {
            panic!("expected an open Android window");
        };
        let state = Rc::new(SurfaceState {
            sender: attachment.event_sender(),
            resources: with_env(Resources::new),
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

    pub(crate) fn draw(
        &mut self,
        draw: impl for<'frame> FnOnce(&mut CanvasRenderingContext2d<'frame>),
    ) -> bool {
        let Some(surface) = android_surface() else {
            return false;
        };
        if self.attachment.is_closed() {
            return false;
        }
        let Some(frame) = self.state.frame.take() else {
            return false;
        };
        let background = self
            .attachment
            .background_color()
            .unwrap_or_else(android_default_background)
            | 0xff00_0000;
        with_env(|env| {
            let jni = Jni::get(env)?;
            let canvas = unsafe {
                env.call_method_unchecked(&*surface, jni.lock_hardware_canvas, OBJECT, &[])
            }?
            .l()?;
            // Start every frame from the window background.
            unsafe {
                call_void(
                    env,
                    &canvas,
                    jni.draw_color,
                    &[
                        jvalue {
                            i: background as jint,
                        },
                        jvalue {
                            l: jni.mode_src.as_raw(),
                        },
                    ],
                )?;
            }
            let base = Transform([frame.scale, 0.0, 0.0, frame.scale, 0.0, 0.0]);
            let mut context = CanvasRenderingContext2d::new(
                PlatformCanvasContext::new(
                    env,
                    &canvas,
                    self.state.resources.clone(),
                    base,
                    Clear::Color(background),
                )?,
                frame.width,
                frame.height,
                frame.scale,
            );
            draw(&mut context);
            drop(context);
            unsafe {
                call_void(
                    env,
                    &surface,
                    jni.unlock_canvas_and_post,
                    &[jvalue { l: canvas.as_raw() }],
                )
            }
        });
        true
    }
}

impl Drop for PlatformCanvas {
    fn drop(&mut self) {
        self.attachment.disconnect();
        self.state.closed.set(true);
    }
}
