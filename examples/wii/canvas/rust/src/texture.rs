/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use core::ffi::c_void;

use libogc_sys::{GX_CLAMP, GX_FALSE, GX_InitTexObj, GX_TF_RGBA8, GXTexObj};

use crate::memory::AlignedBuffer;

pub struct Texture {
    object: GXTexObj,
    _pixels: AlignedBuffer,
}

impl Texture {
    pub fn from_png(bytes: &[u8]) -> Result<Self, ()> {
        let image = image::decode(bytes).map_err(|_| ())?;
        let width = image.width() as usize;
        let height = image.height() as usize;
        let rgba = image.pixels();
        if width > 1024 || height > 1024 {
            return Err(());
        }

        let padded_width = (width + 3) & !3;
        let padded_height = (height + 3) & !3;
        let mut pixels = AlignedBuffer::new_zeroed(padded_width * padded_height * 4).ok_or(())?;
        let mut out = 0;
        {
            let data = pixels.as_mut_slice();
            for tile_y in (0..padded_height).step_by(4) {
                for tile_x in (0..padded_width).step_by(4) {
                    for plane in 0..2 {
                        for y in 0..4 {
                            for x in 0..4 {
                                let px = tile_x + x;
                                let py = tile_y + y;
                                if px < width && py < height {
                                    let i = (py * width + px) * 4;
                                    data[out] = rgba[i + if plane == 0 { 3 } else { 1 }];
                                    data[out + 1] = rgba[i + if plane == 0 { 0 } else { 2 }];
                                }
                                out += 2;
                            }
                        }
                    }
                }
            }
        }
        pixels.flush();
        let mut object = GXTexObj { val: [0; 8] };
        unsafe {
            GX_InitTexObj(
                &mut object,
                pixels.as_ptr().cast::<c_void>(),
                width as u16,
                height as u16,
                GX_TF_RGBA8,
                GX_CLAMP,
                GX_CLAMP,
                GX_FALSE,
            );
        }
        Ok(Self {
            object,
            _pixels: pixels,
        })
    }

    pub unsafe fn bind(&self, map: u8) {
        unsafe { libogc_sys::GX_LoadTexObj(&self.object, map) }
    }
}
