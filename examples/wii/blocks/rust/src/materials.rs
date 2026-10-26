/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use libogc_sys::{
    DCFlushRange, GX_ANISO_4, GX_FALSE, GX_InitTexObj, GX_InitTexObjLOD, GX_LIN_MIP_LIN, GX_NEAR,
    GX_REPEAT, GX_TF_CMPR, GX_TRUE, GXTexObj,
};

use crate::world::Material;

const SIZE: usize = 128;
// CMPR mipmaps go down to one 8x8 tile, each level is a quarter of the one before
const LEVELS: usize = 5;
const IMAGE_SIZE: usize = {
    let mut size = 0;
    let mut level = 0;
    while level < LEVELS {
        size += (SIZE >> level) * (SIZE >> level) / 2;
        level += 1;
    }
    size
};

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
                    SIZE as u16,
                    SIZE as u16,
                    GX_TF_CMPR,
                    GX_REPEAT,
                    GX_REPEAT,
                    GX_TRUE,
                );
                // Like the desktop samplers: sharp texels up close, trilinear and anisotropic
                // mipmaps in the distance, anisotropy needs the edge LOD
                GX_InitTexObjLOD(
                    texture,
                    GX_LIN_MIP_LIN,
                    GX_NEAR,
                    0.0,
                    (LEVELS - 1) as f32,
                    0.0,
                    GX_FALSE,
                    GX_TRUE,
                    GX_ANISO_4,
                );
            }
        }
        Self { textures }
    }

    pub fn get(&self, material: Material) -> &GXTexObj {
        &self.textures[material.index()]
    }
}
