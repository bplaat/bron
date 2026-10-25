/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use alloc::vec::Vec;

use crate::libogc::*;
use crate::materials::Materials;
use crate::memory::AlignedBuffer;
use crate::world::{Face, Material};

const UV: [[u8; 2]; 4] = [[0, 1], [1, 1], [1, 0], [0, 0]];
const MAX_FACES_PER_BATCH: usize = 16_000;

struct DisplayList {
    buffer: AlignedBuffer,
    size: u32,
}

pub struct Mesh {
    lists: [Option<DisplayList>; Material::COUNT],
}

impl Mesh {
    pub fn build(faces: &[Vec<Face>; Material::COUNT]) -> Option<Self> {
        let mut lists = core::array::from_fn(|_| None);
        for (material, instances) in Material::ALL.into_iter().zip(faces) {
            if instances.is_empty() {
                continue;
            }

            // Reserve one extra command block for GX display-list padding.
            let batches = instances.len() / MAX_FACES_PER_BATCH + 1;
            let capacity = instances
                .len()
                .checked_mul(40)?
                .checked_add(batches.checked_mul(64)?)?
                .checked_add(159)?
                & !31;
            let mut buffer = AlignedBuffer::new(capacity)?;
            buffer.invalidate();
            unsafe {
                GX_BeginDispList(buffer.as_ptr(), buffer.size());
                for batch in instances.chunks(MAX_FACES_PER_BATCH) {
                    GX_Begin(GX_QUADS, GX_VTXFMT0, (batch.len() * 4) as u16);
                    for face in batch {
                        emit_face(face, material == Material::Water);
                    }
                }
            }

            let size = unsafe { GX_EndDispList() };
            if size == 0 || size > buffer.size() || !size.is_multiple_of(32) {
                return None;
            }
            lists[material.index()] = Some(DisplayList { buffer, size });
        }
        Some(Self { lists })
    }

    pub fn draw(&self, textures: &Materials) {
        unsafe {
            GX_SetZMode(GX_ENABLE, GX_LEQUAL, GX_TRUE);
            GX_SetBlendMode(GX_BM_NONE, GX_BL_ONE, GX_BL_ZERO, GX_LO_CLEAR);
            for material in Material::ALL.into_iter().filter(|&m| m != Material::Water) {
                let Some(list) = &self.lists[material.index()] else {
                    continue;
                };
                GX_LoadTexObj(textures.get(material), GX_TEXMAP0 as u8);
                GX_CallDispList(list.buffer.as_ptr().cast_const(), list.size);
            }

            if let Some(list) = &self.lists[Material::Water.index()] {
                GX_SetZMode(GX_ENABLE, GX_LEQUAL, GX_FALSE);
                GX_SetBlendMode(GX_BM_BLEND, GX_BL_SRCALPHA, GX_BL_INVSRCALPHA, GX_LO_CLEAR);
                GX_LoadTexObj(textures.get(Material::Water), GX_TEXMAP0 as u8);
                GX_CallDispList(list.buffer.as_ptr().cast_const(), list.size);
            }
        }
    }
}

fn emit_face(face: &Face, water: bool) {
    let corners = face.side.corners();
    let shade = ((0.42 + 0.58 * face.side.light()) * f32::from(face.ambient_occlusion)) as u8;

    unsafe {
        for (corner, uv) in corners.iter().zip(UV) {
            // The world fits in 0..64, so 8-bit integer positions are exact and a quarter of the size of floats
            for (position, offset) in face.position.iter().zip(corner) {
                pipe_u8(position + offset);
            }
            pipe_u8(shade);
            pipe_u8(shade);
            pipe_u8(shade);
            pipe_u8(if water { 153 } else { 255 });
            pipe_u8(uv[0]);
            pipe_u8(uv[1]);
        }
    }
}

#[inline]
unsafe fn pipe_u8(value: u8) {
    unsafe { core::ptr::write_volatile(wgPipe.cast::<u8>(), value) }
}
