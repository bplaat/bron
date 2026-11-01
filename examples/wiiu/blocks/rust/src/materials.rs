/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use core::mem;

use wut_sys::*;

use crate::world::Material;

// The block textures in the order of Material::ALL, the GPU reads the BC1 textures in place,
// which needs the 4 KiB alignment of their tiles
#[repr(C, align(4096))]
struct Textures<T: ?Sized>(T);

static TEXTURES: &Textures<[u8]> =
    &Textures(*include_bytes!(concat!(env!("OUT_DIR"), "/textures.gtx")));

pub struct Materials {
    textures: [GX2Texture; Material::COUNT],
    sampler: GX2Sampler,
}

impl Materials {
    pub fn load() -> Option<Self> {
        let mut textures: [GX2Texture; Material::COUNT] = unsafe { mem::zeroed() };
        for (index, texture) in textures.iter_mut().enumerate() {
            let file = TEXTURES.0.as_ptr().cast();
            if unsafe { GFDGetTextureInPlace(texture, index as u32, file) } == FALSE {
                return None;
            }
        }
        // Sharp texels up close like the other block renderers, blended mipmaps in the distance
        let mut sampler = GX2Sampler::default();
        unsafe {
            GX2InitSampler(
                &mut sampler,
                GX2_TEX_CLAMP_MODE_WRAP,
                GX2_TEX_XY_FILTER_MODE_POINT,
            );
            GX2InitSamplerZMFilter(
                &mut sampler,
                GX2_TEX_Z_FILTER_MODE_POINT,
                GX2_TEX_MIP_FILTER_MODE_LINEAR,
            );
        }
        Some(Self { textures, sampler })
    }

    pub fn texture(&self, material: Material) -> *const GX2Texture {
        &self.textures[material.index()]
    }

    pub fn sampler(&self) -> *const GX2Sampler {
        &self.sampler
    }
}
