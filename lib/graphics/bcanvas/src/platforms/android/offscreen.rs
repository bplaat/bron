/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::rc::Rc;

use jni::sys::jvalue;
use jni::{JValue, jni_sig, jni_str};

use super::context::{Clear, GlobalObject, Jni, PlatformCanvasContext, Resources, with_env};
use crate::copy_rgba_premultiplied_to_straight;
use crate::path::Transform;

/// Draws with an android.graphics.Canvas into its own premultiplied RGBA bitmap.
pub(crate) struct PlatformOffscreenCanvas {
    bitmap: GlobalObject,
    canvas: GlobalObject,
    resources: Rc<Resources>,
    storage: Vec<u8>,
    output: Vec<u8>,
}

impl PlatformOffscreenCanvas {
    pub(crate) fn new(width: u32, height: u32, length: usize) -> Self {
        let (bitmap, canvas, resources) = with_env(|env| {
            let jni = Jni::get(env)?;
            let config_class = env.find_class(jni_str!("android/graphics/Bitmap$Config"))?;
            let config = env
                .get_static_field(
                    &config_class,
                    jni_str!("ARGB_8888"),
                    jni_sig!("Landroid/graphics/Bitmap$Config;"),
                )?
                .l()?;
            let class = env.find_class(jni_str!("android/graphics/Bitmap"))?;
            let bitmap = env
                .call_static_method(
                    &class,
                    jni_str!("createBitmap"),
                    jni_sig!("(IILandroid/graphics/Bitmap$Config;)Landroid/graphics/Bitmap;"),
                    &[
                        JValue::Int(width as i32),
                        JValue::Int(height as i32),
                        (&config).into(),
                    ],
                )?
                .l()?;
            let canvas = unsafe {
                env.new_object_unchecked(
                    &jni.canvas_class,
                    jni.canvas_new,
                    &[jvalue { l: bitmap.as_raw() }],
                )
            }?;
            Ok((
                env.new_global_ref(&bitmap)?,
                env.new_global_ref(&canvas)?,
                Resources::new(env)?,
            ))
        });
        Self {
            bitmap,
            canvas,
            resources,
            storage: vec![0; length],
            output: vec![0; length],
        }
    }

    pub(crate) fn begin_draw(&mut self) -> PlatformCanvasContext {
        with_env(|env| {
            PlatformCanvasContext::new(
                env,
                &self.canvas,
                self.resources.clone(),
                Transform::IDENTITY,
                Clear::Transparent,
            )
        })
    }

    #[allow(
        clippy::missing_const_for_fn,
        reason = "keeps the shared offscreen API non-const"
    )]
    pub(crate) fn end_draw(&mut self) {}

    pub(crate) fn pixels(&mut self) -> &[u8] {
        let (storage, bitmap) = (&mut self.storage, &self.bitmap);
        with_env(|env| {
            let buffer =
                unsafe { env.new_direct_byte_buffer(storage.as_mut_ptr(), storage.len()) }?;
            env.call_method(
                bitmap,
                jni_str!("copyPixelsToBuffer"),
                jni_sig!("(Ljava/nio/Buffer;)V"),
                &[(&buffer).into()],
            )?
            .v()
        });
        copy_rgba_premultiplied_to_straight(&self.storage, &mut self.output);
        &self.output
    }
}
