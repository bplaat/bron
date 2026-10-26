/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The block tiles, font and cursor, converted by build.rs with gxtexconv.

use alloc::vec::Vec;

use libogc_sys::*;

pub struct Texture {
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

pub struct Textures {
    pub tiles: Vec<GXTexObj>,
    pub font: GXTexObj,
    pub cursor: GXTexObj,
}

impl Textures {
    pub fn load() -> Self {
        crate::memory::flush(&DATA.0);
        Self {
            tiles: TILES
                .iter()
                .map(|texture| object(texture, GX_REPEAT))
                .collect(),
            font: object(&FONT, GX_CLAMP),
            cursor: object(&CURSOR, GX_CLAMP),
        }
    }
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
