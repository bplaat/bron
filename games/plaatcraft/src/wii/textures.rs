/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The block tiles, font and cursor, converted by build.rs with gxtexconv.

use libogc_sys::*;

pub(crate) struct Texture {
    offset: usize,
    width: u16,
    height: u16,
    format: u8,
    levels: u8,
}

include!(concat!(env!("OUT_DIR"), "/textures.rs"));

#[repr(align(32))]
struct Data<T: ?Sized>(T);

static DATA: &Data<[u8]> = &Data(*include_bytes!(concat!(env!("OUT_DIR"), "/textures.bin")));

pub(crate) struct Textures {
    pub(crate) tiles: Vec<GXTexObj>,
    pub(crate) sky: Vec<GXTexObj>,
    pub(crate) characters: Vec<GXTexObj>,
    pub(crate) font: GXTexObj,
    pub(crate) mouse: GXTexObj,
    pub(crate) cursors: [GXTexObj; 4],
}

impl Textures {
    pub(crate) fn load() -> Self {
        crate::wii::memory::flush(&DATA.0);
        Self {
            tiles: TILES
                .iter()
                .map(|texture| object(texture, GX_REPEAT))
                .collect(),
            characters: CHARACTERS
                .iter()
                .map(|texture| object(texture, GX_CLAMP))
                .collect(),
            sky: SKY
                .iter()
                .map(|texture| object(texture, GX_REPEAT))
                .collect(),
            font: object(&FONT, GX_CLAMP),
            mouse: object(&MOUSE, GX_CLAMP),
            cursors: CURSORS.map(|texture| object(&texture, GX_CLAMP)),
        }
    }
}

/// Whether a block tile has transparent texels, build.rs only converts opaque tiles to CMPR.
pub(crate) fn has_alpha(tile: usize) -> bool {
    TILES[tile].format != GX_TF_CMPR
}

fn object(texture: &Texture, wrap: u8) -> GXTexObj {
    let mut object = GXTexObj { val: [0; 8] };
    let mipmapped = texture.levels > 1;
    unsafe {
        GX_InitTexObj(
            &mut object,
            DATA.0.as_ptr().add(texture.offset).cast_mut().cast(),
            texture.width,
            texture.height,
            texture.format,
            wrap,
            wrap,
            u8::from(mipmapped),
        );
        if mipmapped {
            // Pixel art stays sharp up close and blends between mipmaps in the distance
            GX_InitTexObjLOD(
                &mut object,
                GX_LIN_MIP_LIN,
                GX_NEAR,
                0.0,
                f32::from(texture.levels - 1),
                0.0,
                GX_FALSE,
                GX_TRUE,
                GX_ANISO_1,
            );
        }
    }
    object
}
