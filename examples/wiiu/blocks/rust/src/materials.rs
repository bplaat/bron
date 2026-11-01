/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use alloc::vec::Vec;

use wut_sys::*;

use crate::texture::Texture;
use crate::world::Material;

// The block images in the order of Material::ALL
const IMAGES: [&[u8]; Material::COUNT] = [
    include_bytes!("assets/brick_red.png"),
    include_bytes!("assets/cactus_side.png"),
    include_bytes!("assets/cactus_top.png"),
    include_bytes!("assets/dirt.png"),
    include_bytes!("assets/dirt_grass.png"),
    include_bytes!("assets/grass_top.png"),
    include_bytes!("assets/greystone.png"),
    include_bytes!("assets/lava.png"),
    include_bytes!("assets/leaves.png"),
    include_bytes!("assets/sand.png"),
    include_bytes!("assets/stone.png"),
    include_bytes!("assets/stone_coal.png"),
    include_bytes!("assets/stone_diamond.png"),
    include_bytes!("assets/stone_gold.png"),
    include_bytes!("assets/stone_iron.png"),
    include_bytes!("assets/trunk_side.png"),
    include_bytes!("assets/trunk_top.png"),
    include_bytes!("assets/water.png"),
    include_bytes!("assets/wood.png"),
];

pub struct Materials {
    textures: Vec<Texture>,
    sampler: GX2Sampler,
}

impl Materials {
    pub fn load() -> Option<Self> {
        let textures = IMAGES
            .iter()
            .map(|image| Texture::decode(image))
            .collect::<Option<Vec<_>>>()?;
        // Sharp texels like the other block renderers, the textures have no mipmaps yet
        let mut sampler = GX2Sampler::default();
        unsafe {
            GX2InitSampler(
                &mut sampler,
                GX2_TEX_CLAMP_MODE_WRAP,
                GX2_TEX_XY_FILTER_MODE_POINT,
            )
        };
        Some(Self { textures, sampler })
    }

    pub fn texture(&self, material: Material) -> *const GX2Texture {
        self.textures[material.index()].as_ptr()
    }

    pub fn sampler(&self) -> *const GX2Sampler {
        &self.sampler
    }
}
