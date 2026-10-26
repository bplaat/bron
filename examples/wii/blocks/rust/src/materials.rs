/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use libogc_sys::{DCFlushRange, GX_FALSE, GX_InitTexObj, GX_REPEAT, GX_TF_CMPR, GXTexObj};

use crate::world::Material;

const IMAGE_SIZE: usize = 128 * 128 / 2;

#[repr(align(32))]
struct AlignedImages([u8; IMAGE_SIZE * Material::COUNT]);

static IMAGES: AlignedImages =
    AlignedImages(*include_bytes!(concat!(env!("OUT_DIR"), "/materials.bin")));

pub struct Materials {
    textures: [GXTexObj; Material::COUNT],
}

impl Materials {
    pub fn load() -> Self {
        let mut textures = [GXTexObj { val: [0; 8] }; Material::COUNT];
        unsafe {
            DCFlushRange(IMAGES.0.as_ptr().cast_mut().cast(), IMAGES.0.len() as u32);
            for (index, texture) in textures.iter_mut().enumerate() {
                GX_InitTexObj(
                    texture,
                    IMAGES.0.as_ptr().add(index * IMAGE_SIZE).cast_mut().cast(),
                    128,
                    128,
                    GX_TF_CMPR,
                    GX_REPEAT,
                    GX_REPEAT,
                    GX_FALSE,
                );
            }
        }
        Self { textures }
    }

    pub fn get(&self, material: Material) -> &GXTexObj {
        &self.textures[material.index()]
    }
}
