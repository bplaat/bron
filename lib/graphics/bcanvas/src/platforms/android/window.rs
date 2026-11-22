/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use bwindow::ffi::{
    android_default_background, android_density, android_post_local, android_surface,
    android_window_size,
};
use bwindow::{NativeContentHandle, WindowAttachment, WindowEventSender};
use jni::objects::{Global, JObject};
use jni::sys::{jint, jvalue};

use crate::CanvasRenderingContext2d;
use crate::path::Transform;
use crate::platforms::android::context::{
    Clear, Jni, OBJECT, PlatformCanvasContext, Resources, call_void, with_env,
};

type Surface = Rc<Global<JObject<'static>>>;

#[derive(Clone, Copy)]
struct Frame {
    width: f32,
    height: f32,
    scale: f32,
}

// Draws the window surface, or the surface of a native window layer, with a hardware
// accelerated android.graphics.Canvas on the Rust main thread, the window paces the
// animation frames.
struct SurfaceState {
    sender: WindowEventSender,
    // The surface of a native layer with its size in pixels, else the window surface.
    layer: Option<(Surface, Cell<(u32, u32)>)>,
    resources: Rc<Resources>,
    frame: Cell<Option<Frame>>,
    closed: Cell<bool>,
    painting: Cell<bool>,
    scheduled: Cell<bool>,
    frame_end: RefCell<Option<Rc<dyn Fn()>>>,
}

impl SurfaceState {
    fn surface(&self) -> Option<Surface> {
        match &self.layer {
            Some((surface, _)) => Some(surface.clone()),
            None => android_surface(),
        }
    }

    fn request_redraw(self: &Rc<Self>) {
        if self.closed.get() {
            return;
        }
        // Redraws requested while painting wait for the next display frame.
        if self.painting.get() {
            self.sender.request_animation_frame();
        } else if !self.scheduled.replace(true) {
            let state = Rc::downgrade(self);
            android_post_local(move || paint(&state));
        }
    }
}

fn paint(state: &Weak<SurfaceState>) {
    let Some(state) = state.upgrade() else {
        return;
    };
    state.scheduled.set(false);
    if state.closed.get() || state.painting.get() || state.surface().is_none() {
        return;
    }
    let (width, height) = state
        .layer
        .as_ref()
        .map_or_else(android_window_size, |(_, size)| size.get());
    let scale = android_density();
    state.painting.set(true);
    state.frame.set(Some(Frame {
        width: width as f32 / scale,
        height: height as f32 / scale,
        scale,
    }));
    if !state.sender.redraw(|| {
        let hook = state.frame_end.borrow().clone();
        if let Some(hook) = hook
            && state.frame.get().is_some()
        {
            hook();
        }
        state.frame.set(None);
    }) {
        state.frame.set(None);
        state.sender.request_animation_frame();
    }
    state.painting.set(false);
}

pub(crate) struct PlatformCanvas {
    state: Rc<SurfaceState>,
    attachment: WindowAttachment,
}

impl PlatformCanvas {
    pub(crate) fn new(attachment: WindowAttachment) -> Self {
        let layer = match unsafe { attachment.content_handle() } {
            Some(NativeContentHandle::Android(_)) => None,
            Some(NativeContentHandle::AndroidSurface(surface)) => Some((
                Rc::new(with_env(|env| {
                    let surface = unsafe { JObject::from_raw(env, surface.cast()) };
                    env.new_global_ref(&surface)
                })),
                Cell::new(attachment.size()),
            )),
            _ => panic!("expected an open Android window"),
        };
        let state = Rc::new(SurfaceState {
            sender: attachment.event_sender(),
            layer,
            resources: with_env(Resources::new),
            frame: Cell::new(None),
            closed: Cell::new(false),
            painting: Cell::new(false),
            scheduled: Cell::new(false),
            frame_end: RefCell::new(None),
        });
        let redraw = Rc::downgrade(&state);
        attachment.on_redraw(move || {
            if let Some(state) = redraw.upgrade() {
                state.request_redraw();
            }
        });
        let resize = Rc::downgrade(&state);
        attachment.on_resize(move |width, height| {
            if let Some(state) = resize.upgrade() {
                if let Some((_, size)) = &state.layer {
                    size.set((width, height));
                }
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
        self.attachment.request_animation_frame();
    }

    /// The logical size of the open native frame.
    pub(crate) fn frame_size(&self) -> Option<(f32, f32)> {
        self.state
            .frame
            .get()
            .map(|frame| (frame.width, frame.height))
    }

    /// Run `hook` at the end of every native frame that nothing drew yet, which it can
    /// still draw with `draw`.
    pub(crate) fn on_frame_end(&self, hook: impl Fn() + 'static) {
        *self.state.frame_end.borrow_mut() = Some(Rc::new(hook));
    }

    pub(crate) fn draw(
        &mut self,
        draw: impl for<'frame> FnOnce(&mut CanvasRenderingContext2d<'frame>),
    ) -> bool {
        let Some(surface) = self.state.surface() else {
            return false;
        };
        if self.attachment.is_closed() {
            return false;
        }
        let Some(frame) = self.state.frame.take() else {
            return false;
        };
        // The window starts every frame from its background, a layer from transparency.
        let clear = if self.state.layer.is_some() {
            Clear::Transparent
        } else {
            Clear::Color(
                self.attachment
                    .background_color()
                    .unwrap_or_else(android_default_background)
                    | 0xff00_0000,
            )
        };
        with_env(|env| {
            let jni = Jni::get(env)?;
            let canvas = unsafe {
                env.call_method_unchecked(&*surface, jni.lock_hardware_canvas, OBJECT, &[])
            }?
            .l()?;
            let (color, mode) = match clear {
                Clear::Color(color) => (color, &jni.mode_src),
                Clear::Transparent => (0, &jni.mode_clear),
            };
            unsafe {
                call_void(
                    env,
                    &canvas,
                    jni.draw_color,
                    &[jvalue { i: color as jint }, jvalue { l: mode.as_raw() }],
                )?;
            }
            let base = Transform([frame.scale, 0.0, 0.0, frame.scale, 0.0, 0.0]);
            let mut context = CanvasRenderingContext2d::new(
                PlatformCanvasContext::new(
                    env,
                    &canvas,
                    self.state.resources.clone(),
                    base,
                    clear,
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
