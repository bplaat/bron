/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::rc::Rc;

use bwindow::ffi::*;

use super::context::{Clear, Jni, PlatformCanvasContext, Resources};
use crate::path::Transform;

/// Draws with an android.graphics.Canvas into its own premultiplied RGBA bitmap.
pub(crate) struct PlatformOffscreenCanvas {
    bitmap: GlobalRef,
    canvas: GlobalRef,
    resources: Rc<Resources>,
    storage: Vec<u8>,
    dirty: bool,
}

impl PlatformOffscreenCanvas {
    pub(crate) fn new(width: u32, height: u32, length: usize) -> Self {
        let env = JniEnv::current();
        let jni = Jni::get(env);
        unsafe {
            let class = env.find_class(c"android/graphics/Bitmap");
            let config_class = env.find_class(c"android/graphics/Bitmap$Config");
            let config = env.static_object_field(
                config_class,
                c"ARGB_8888",
                c"Landroid/graphics/Bitmap$Config;",
            );
            let create = env.static_method(
                class,
                c"createBitmap",
                c"(IILandroid/graphics/Bitmap$Config;)Landroid/graphics/Bitmap;",
            );
            let bitmap = env.call_static_object(
                class,
                create,
                &[
                    jvalue { i: width as jint },
                    jvalue { i: height as jint },
                    jvalue { l: config },
                ],
            );
            let canvas =
                env.new_object(jni.canvas_class(), jni.canvas_new, &[jvalue { l: bitmap }]);
            for object in [config, config_class, class] {
                env.delete_local_ref(object);
            }
            Self {
                bitmap: GlobalRef::from_local(env, bitmap),
                canvas: GlobalRef::from_local(env, canvas),
                resources: Resources::new(env),
                storage: vec![0; length],
                dirty: false,
            }
        }
    }

    pub(crate) fn begin_draw(&mut self) -> PlatformCanvasContext {
        self.dirty = true;
        PlatformCanvasContext::new(
            self.canvas.as_raw(),
            self.resources.clone(),
            Transform::IDENTITY,
            Clear::Transparent,
        )
    }

    #[allow(
        clippy::missing_const_for_fn,
        reason = "keeps the shared offscreen API non-const"
    )]
    pub(crate) fn end_draw(&mut self) {}

    pub(crate) fn pixels(&mut self) -> &[u8] {
        if !std::mem::take(&mut self.dirty) {
            return &self.storage;
        }
        let env = JniEnv::current();
        unsafe {
            let buffer = env.new_direct_byte_buffer(self.storage.as_mut_ptr(), self.storage.len());
            env.call_void_method(
                self.bitmap.as_raw(),
                c"copyPixelsToBuffer",
                c"(Ljava/nio/Buffer;)V",
                &[jvalue { l: buffer }],
            );
            env.delete_local_ref(buffer);
        }
        &self.storage
    }
}
