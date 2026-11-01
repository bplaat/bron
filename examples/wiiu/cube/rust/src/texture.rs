/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use core::mem::MaybeUninit;
use core::ptr;

use wut_sys::*;

/// An RGBA8 texture decoded from an image. The rows stay linear, which the GPU reads without
/// tiling, they only need the pitch GX2 calculates.
pub struct Texture(GX2Texture);

impl Texture {
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let image = image::decode(bytes).ok()?;
        let (width, height) = (image.width(), image.height());
        let mut texture = unsafe { MaybeUninit::<GX2Texture>::zeroed().assume_init() };
        texture.surface.dim = GX2_SURFACE_DIM_TEXTURE_2D;
        texture.surface.width = width;
        texture.surface.height = height;
        texture.surface.depth = 1;
        texture.surface.mip_levels = 1;
        texture.surface.format = GX2_SURFACE_FORMAT_UNORM_R8_G8_B8_A8;
        texture.surface.r#use = GX2_SURFACE_USE_TEXTURE;
        texture.surface.tile_mode = GX2_TILE_MODE_LINEAR_ALIGNED;
        texture.view_num_mips = 1;
        texture.view_num_slices = 1;
        texture.comp_map = GX2_COMP_MAP(GX2_SQ_SEL_R, GX2_SQ_SEL_G, GX2_SQ_SEL_B, GX2_SQ_SEL_A);
        unsafe {
            GX2CalcSurfaceSizeAndAlignment(&mut texture.surface);
            GX2InitTextureRegs(&mut texture);
            texture.surface.image = memalign(
                texture.surface.alignment as usize,
                texture.surface.image_size as usize,
            );
        }
        if texture.surface.image.is_null() {
            return None;
        }

        let row_size = width as usize * 4;
        let pitch_size = texture.surface.pitch as usize * 4;
        for (y, row) in image.pixels().chunks_exact(row_size).enumerate() {
            unsafe {
                ptr::copy_nonoverlapping(
                    row.as_ptr(),
                    texture.surface.image.cast::<u8>().add(y * pitch_size),
                    row_size,
                );
            }
        }
        unsafe {
            GX2Invalidate(
                GX2_INVALIDATE_MODE_CPU_TEXTURE,
                texture.surface.image,
                texture.surface.image_size,
            );
        }
        Some(Self(texture))
    }

    pub fn as_ptr(&self) -> *const GX2Texture {
        &self.0
    }
}

impl Drop for Texture {
    fn drop(&mut self) {
        unsafe { free(self.0.surface.image) }
    }
}
