/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Native window layers from Android 10: a child `SurfaceControl` for each layer, stacked
//! above the window surface and cropped to the window, which the system compositor blends.
//! Renderers draw its `Surface`. The parent is the root surface control of the window from
//! Android 13, before that the surface of a transparent `SurfaceView` over the window. Older
//! versions share the window surface, which renderers composite themselves.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use jni::objects::{Global, JObject};
use jni::{Env, JValue, jni_sig, jni_str};

use super::activity::{android_activity, android_run_on_ui_thread, android_with_env, sdk_version};
use super::event_loop::{android_density, android_post_frame, android_window_size};
use crate::{LogicalRect, NativeContentHandle};

// Public SurfaceControl APIs are from Android 10, its position and crop from Android 12 and
// the root surface control of a window from Android 13.
const MIN_SDK: i32 = 29;
const POSITION_SDK: i32 = 31;
const ROOT_SDK: i32 = 33;
// android.graphics.PixelFormat.TRANSLUCENT.
const TRANSLUCENT: i32 = -3;

struct LayerState {
    control: Global<JObject<'static>>,
    surface: Global<JObject<'static>>,
    z: i32,
    // The surface control the layer is a child of, none before the window shows.
    parent: Option<Global<JObject<'static>>>,
    frame: Option<LogicalRect>,
    hidden: bool,
    outside: bool,
    retrying: bool,
}

impl LayerState {
    const fn visible(&self) -> bool {
        self.parent.is_some() && !self.hidden && !self.outside
    }
}

thread_local! {
    #[allow(clippy::missing_const_for_thread_local, reason = "false positive without native TLS")]
    static LAYERS: RefCell<HashMap<usize, LayerState>> = RefCell::new(HashMap::new());
    #[allow(clippy::missing_const_for_thread_local, reason = "false positive without native TLS")]
    static NEXT_Z: Cell<i32> = const { Cell::new(1) };
}

// The SurfaceView whose surface parents the layers before Android 13.
static HOST: OnceLock<Global<JObject<'static>>> = OnceLock::new();
static HOST_REQUESTED: AtomicBool = AtomicBool::new(false);

fn key(layer: NativeContentHandle) -> usize {
    let NativeContentHandle::AndroidSurface(surface) = layer else {
        panic!("expected an Android layer surface");
    };
    surface as usize
}

/// Whether layers get native child surfaces.
pub(crate) unsafe fn native_layers(_: NativeContentHandle) -> bool {
    sdk_version() >= MIN_SDK
}

// A new transaction of surface controls.
fn transaction<'local>(env: &mut Env<'local>) -> jni::errors::Result<JObject<'local>> {
    let class = env.find_class(jni_str!("android/view/SurfaceControl$Transaction"))?;
    env.new_object(&class, jni_sig!("()V"), &[])
}

fn apply(env: &mut Env, transaction: &JObject) -> jni::errors::Result<()> {
    env.call_method(transaction, jni_str!("apply"), jni_sig!("()V"), &[])?
        .v()
}

// Add a transparent SurfaceView over the window, on the UI thread. Its surface is stacked
// above the window surface and stays empty, the layers are its children.
fn add_host_view() {
    android_with_env(|env| {
        let activity = android_activity();
        let class = env.find_class(jni_str!("android/view/SurfaceView"))?;
        let view = env.new_object(
            &class,
            jni_sig!("(Landroid/content/Context;)V"),
            &[activity.into()],
        )?;
        env.call_method(
            &view,
            jni_str!("setZOrderOnTop"),
            jni_sig!("(Z)V"),
            &[JValue::Bool(true)],
        )?;
        let holder = env
            .call_method(
                &view,
                jni_str!("getHolder"),
                jni_sig!("()Landroid/view/SurfaceHolder;"),
                &[],
            )?
            .l()?;
        env.call_method(
            &holder,
            jni_str!("setFormat"),
            jni_sig!("(I)V"),
            &[JValue::Int(TRANSLUCENT)],
        )?;
        let class = env.find_class(jni_str!("android/view/ViewGroup$LayoutParams"))?;
        let params = env.new_object(
            &class,
            jni_sig!("(II)V"),
            &[JValue::Int(-1), JValue::Int(-1)],
        )?;
        env.call_method(
            activity,
            jni_str!("addContentView"),
            jni_sig!("(Landroid/view/View;Landroid/view/ViewGroup$LayoutParams;)V"),
            &[(&view).into(), (&params).into()],
        )?;
        let _ = HOST.set(env.new_global_ref(&view)?);
        Ok(())
    });
}

/// Add a child surface above the window surface and the layers added before it.
pub(crate) unsafe fn add_layer_view(
    _: NativeContentHandle,
    _: Option<(NativeContentHandle, bool)>,
) -> NativeContentHandle {
    if sdk_version() < ROOT_SDK && !HOST_REQUESTED.swap(true, Ordering::AcqRel) {
        android_run_on_ui_thread(add_host_view);
    }
    let z = NEXT_Z.replace(NEXT_Z.get() + 1);
    let (control, surface) = android_with_env(|env| {
        let class = env.find_class(jni_str!("android/view/SurfaceControl$Builder"))?;
        let builder = env.new_object(&class, jni_sig!("()V"), &[])?;
        let name = env.new_string("bwindow layer")?;
        let builder_sig = jni_sig!("(Ljava/lang/String;)Landroid/view/SurfaceControl$Builder;");
        env.call_method(
            &builder,
            jni_str!("setName"),
            builder_sig,
            &[(&name).into()],
        )?;
        env.call_method(
            &builder,
            jni_str!("setFormat"),
            jni_sig!("(I)Landroid/view/SurfaceControl$Builder;"),
            &[JValue::Int(TRANSLUCENT)],
        )?;
        let control = env
            .call_method(
                &builder,
                jni_str!("build"),
                jni_sig!("()Landroid/view/SurfaceControl;"),
                &[],
            )?
            .l()?;
        let class = env.find_class(jni_str!("android/view/Surface"))?;
        let surface = env.new_object(
            &class,
            jni_sig!("(Landroid/view/SurfaceControl;)V"),
            &[(&control).into()],
        )?;
        Ok((env.new_global_ref(&control)?, env.new_global_ref(&surface)?))
    });
    let handle = NativeContentHandle::AndroidSurface(surface.as_raw().cast());
    LAYERS.with_borrow_mut(|layers| {
        layers.insert(
            key(handle),
            LayerState {
                control,
                surface,
                z,
                parent: None,
                frame: None,
                hidden: false,
                outside: false,
                retrying: false,
            },
        );
    });
    handle
}

// The surface control that parents the layers: the root of the window from Android 13,
// before that the surface of the host SurfaceView. None until the window shows.
fn parent<'local>(env: &mut Env<'local>) -> jni::errors::Result<Option<JObject<'local>>> {
    let parent = if sdk_version() >= ROOT_SDK {
        let window = env
            .call_method(
                android_activity(),
                jni_str!("getWindow"),
                jni_sig!("()Landroid/view/Window;"),
                &[],
            )?
            .l()?;
        env.call_method(
            &window,
            jni_str!("getRootSurfaceControl"),
            jni_sig!("()Landroid/view/AttachedSurfaceControl;"),
            &[],
        )?
        .l()?
    } else {
        let Some(host) = HOST.get() else {
            return Ok(None);
        };
        let control = env
            .call_method(
                host.as_obj(),
                jni_str!("getSurfaceControl"),
                jni_sig!("()Landroid/view/SurfaceControl;"),
                &[],
            )?
            .l()?;
        // The SurfaceView gets a new surface each time the window shows again.
        if control.is_null()
            || !env
                .call_method(&control, jni_str!("isValid"), jni_sig!("()Z"), &[])?
                .z()?
        {
            return Ok(None);
        }
        control
    };
    Ok((!parent.is_null()).then_some(parent))
}

// A transaction that makes a surface a child of a parent above its own surface, none while
// the window cannot take children.
fn reparent<'local>(
    env: &mut Env<'local>,
    state: &LayerState,
    parent: &JObject,
) -> jni::errors::Result<Option<JObject<'local>>> {
    let control = state.control.as_obj();
    let transaction = if sdk_version() >= ROOT_SDK {
        env.call_method(
            parent,
            jni_str!("buildReparentTransaction"),
            jni_sig!("(Landroid/view/SurfaceControl;)Landroid/view/SurfaceControl$Transaction;"),
            &[control.into()],
        )?
        .l()?
    } else {
        let transaction = transaction(env)?;
        env.call_method(
            &transaction,
            jni_str!("reparent"),
            jni_sig!("(Landroid/view/SurfaceControl;Landroid/view/SurfaceControl;)Landroid/view/SurfaceControl$Transaction;"),
            &[control.into(), parent.into()],
        )?;
        transaction
    };
    if transaction.is_null() {
        return Ok(None);
    }
    env.call_method(
        &transaction,
        jni_str!("setLayer"),
        jni_sig!("(Landroid/view/SurfaceControl;I)Landroid/view/SurfaceControl$Transaction;"),
        &[control.into(), JValue::Int(state.z)],
    )?;
    Ok(Some(transaction))
}

/// Place a child surface at an area of the window, or fill it, cropped to the window.
pub(crate) unsafe fn set_layer_view_frame(
    _: NativeContentHandle,
    layer: NativeContentHandle,
    frame: Option<LogicalRect>,
) {
    let key = key(layer);
    LAYERS.with_borrow_mut(|layers| {
        if let Some(state) = layers.get_mut(&key) {
            state.frame = frame;
            place(key, state);
        }
    });
}

// Attach a layer to its parent when it changed and place it at its frame. A layer whose
// parent does not exist yet tries again at the next display frame.
fn place(key: usize, state: &mut LayerState) {
    let density = android_density();
    let (window_width, window_height) = android_window_size();
    let (window_width, window_height) = (window_width as f32, window_height as f32);
    let [x, y, width, height] = state
        .frame
        .map_or([0.0, 0.0, window_width, window_height], |frame| {
            [frame.x, frame.y, frame.width, frame.height].map(|value| value * density)
        });
    // The part of the layer inside the window, in its own coordinates.
    let crop = [
        (-x).max(0.0),
        (-y).max(0.0),
        (window_width - x).min(width),
        (window_height - y).min(height),
    ]
    .map(|value| value.round() as i32);
    state.outside = crop[2] <= crop[0] || crop[3] <= crop[1];
    android_with_env(|env| {
        let parent = parent(env)?;
        let same = match (&parent, &state.parent) {
            (Some(parent), Some(current)) => env.is_same_object(parent, current.as_obj())?,
            _ => false,
        };
        let reparented = match &parent {
            Some(parent) if !same => reparent(env, state, parent)?,
            _ => None,
        };
        state.parent = match (&parent, &reparented) {
            (Some(parent), Some(_)) => Some(env.new_global_ref(parent)?),
            (Some(_), None) if same => state.parent.take(),
            _ => None,
        };
        let transaction = match reparented {
            Some(transaction) => transaction,
            None => transaction(env)?,
        };
        let control = state.control.as_obj();
        let sig =
            jni_sig!("(Landroid/view/SurfaceControl;II)Landroid/view/SurfaceControl$Transaction;");
        env.call_method(
            &transaction,
            jni_str!("setBufferSize"),
            sig,
            &[
                control.into(),
                JValue::Int((width.round() as i32).max(1)),
                JValue::Int((height.round() as i32).max(1)),
            ],
        )?;
        // A layer outside the window, like before it has a size, is only hidden.
        if !state.outside {
            let class = env.find_class(jni_str!("android/graphics/Rect"))?;
            let rect = env.new_object(&class, jni_sig!("(IIII)V"), &crop.map(JValue::Int))?;
            if sdk_version() >= POSITION_SDK {
                let sig = jni_sig!(
                    "(Landroid/view/SurfaceControl;FF)Landroid/view/SurfaceControl$Transaction;"
                );
                env.call_method(
                    &transaction,
                    jni_str!("setPosition"),
                    sig,
                    &[control.into(), JValue::Float(x), JValue::Float(y)],
                )?;
                let sig = jni_sig!(
                    "(Landroid/view/SurfaceControl;Landroid/graphics/Rect;)Landroid/view/SurfaceControl$Transaction;"
                );
                env.call_method(
                    &transaction,
                    jni_str!("setCrop"),
                    sig,
                    &[control.into(), (&rect).into()],
                )?;
            } else {
                // The cropped part of the buffer, shown unscaled at its place in the window.
                let [left, top, right, bottom] = crop;
                let (x, y) = (x.round() as i32, y.round() as i32);
                let destination = env.new_object(
                    &class,
                    jni_sig!("(IIII)V"),
                    &[x + left, y + top, x + right, y + bottom].map(JValue::Int),
                )?;
                let sig = jni_sig!(
                    "(Landroid/view/SurfaceControl;Landroid/graphics/Rect;Landroid/graphics/Rect;I)Landroid/view/SurfaceControl$Transaction;"
                );
                env.call_method(
                    &transaction,
                    jni_str!("setGeometry"),
                    sig,
                    &[
                        control.into(),
                        (&rect).into(),
                        (&destination).into(),
                        JValue::Int(-1),
                    ],
                )?;
            }
        }
        set_visibility(env, &transaction, control, state.visible())?;
        apply(env, &transaction)
    });
    if state.parent.is_none() && !std::mem::replace(&mut state.retrying, true) {
        android_post_frame(move || {
            LAYERS.with_borrow_mut(|layers| {
                if let Some(state) = layers.get_mut(&key) {
                    state.retrying = false;
                    place(key, state);
                }
            });
        });
    }
}

fn set_visibility(
    env: &mut Env,
    transaction: &JObject,
    control: &JObject,
    visible: bool,
) -> jni::errors::Result<()> {
    env.call_method(
        transaction,
        jni_str!("setVisibility"),
        jni_sig!("(Landroid/view/SurfaceControl;Z)Landroid/view/SurfaceControl$Transaction;"),
        &[control.into(), JValue::Bool(visible)],
    )?;
    Ok(())
}

pub(crate) unsafe fn set_layer_view_hidden(layer: NativeContentHandle, hidden: bool) {
    LAYERS.with_borrow_mut(|layers| {
        let Some(state) = layers.get_mut(&key(layer)) else {
            return;
        };
        state.hidden = hidden;
        android_with_env(|env| {
            let transaction = transaction(env)?;
            set_visibility(env, &transaction, state.control.as_obj(), state.visible())?;
            apply(env, &transaction)
        });
    });
}

/// Detach a child surface from the window and release it.
pub(crate) unsafe fn remove_layer_view(layer: NativeContentHandle) {
    let Some(state) = LAYERS.with_borrow_mut(|layers| layers.remove(&key(layer))) else {
        return;
    };
    android_with_env(|env| {
        let transaction = transaction(env)?;
        let control = state.control.as_obj();
        env.call_method(
            &transaction,
            jni_str!("reparent"),
            jni_sig!("(Landroid/view/SurfaceControl;Landroid/view/SurfaceControl;)Landroid/view/SurfaceControl$Transaction;"),
            &[control.into(), (&JObject::null()).into()],
        )?;
        apply(env, &transaction)?;
        env.call_method(
            state.surface.as_obj(),
            jni_str!("release"),
            jni_sig!("()V"),
            &[],
        )?;
        env.call_method(control, jni_str!("release"), jni_sig!("()V"), &[])?;
        Ok(())
    });
}
