/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Turns the packed greedy quads of a chunk into a GX display list. Each vertex has a
//! fixed point position in the chunk, a palette index and fixed point texture coordinates,
//! the quads are grouped by section, pass and tile so a draw is one texture.

use alloc::vec::Vec;
use core::ops::Range;

use plaatcraft_engine::tiles::tile;
use plaatcraft_engine::world::{
    self, HEIGHT, QUAD_BYTES, SECTION_COUNT, SECTION_SIZE, quad_material,
};

use crate::textures::has_alpha;

const GX_QUADS: u8 = 0x80;
// A draw has a 16-bit vertex count
const MAX_QUADS: usize = u16::MAX as usize / 4;

// The palette is indexed by tint, face and the light byte of a quad
pub const TINT_PINE: u16 = 1;
pub const TINT_BEDROCK: u16 = 2;
pub const TINT_LAVA: u16 = 3;
pub const TINT_WATER: u16 = 4;
pub const TINTS: usize = 5;
pub const PALETTE_SIZE: usize = TINTS << 13;

// The passes have their own depth, alpha and blend state
pub const SOLID: usize = 0;
pub const CUTOUT: usize = 1;
pub const TRANSLUCENT: usize = 2;

/// A part of the display list with quads of one tile.
pub struct Draw {
    pub tile: u8,
    pub list: Range<usize>,
}

pub struct Mesh {
    /// 32-byte aligned by the allocator and flushed from the CPU cache.
    pub list: Vec<u8>,
    pub draws: Vec<Draw>,
    /// The draws of each section and pass.
    pub sections: [[Range<usize>; 3]; SECTION_COUNT],
}

struct Quad {
    words: [u32; 3],
    tile: u8,
}

impl Quad {
    fn section(&self) -> usize {
        ((self.words[0] >> 27) & 15) as usize
    }

    fn pass(&self) -> usize {
        if world::is_translucent(quad_material(self.words[0])) {
            TRANSLUCENT
        } else if has_alpha(self.tile) {
            CUTOUT
        } else {
            SOLID
        }
    }
}

pub fn build(packed: &[u8]) -> Mesh {
    let mut quads: Vec<Quad> = packed
        .as_chunks::<QUAD_BYTES>()
        .0
        .iter()
        .map(|bytes| {
            let words = [0, 4, 8].map(|i| u32::from_ne_bytes(bytes[i..i + 4].try_into().unwrap()));
            let face = (words[0] >> 17) & 7;
            Quad {
                words,
                tile: tile(quad_material(words[0]) as u8, face) as u8,
            }
        })
        .collect();
    quads.sort_by_key(|quad| (quad.section(), quad.pass(), quad.tile));

    let mut mesh = Mesh {
        list: Vec::with_capacity(quads.len() * 48 + 64),
        draws: Vec::new(),
        sections: Default::default(),
    };
    let mut start = 0;
    while start < quads.len() {
        let first = &quads[start];
        let key = (first.section(), first.pass(), first.tile);
        let mut end = start + 1;
        while end < quads.len()
            && end - start < MAX_QUADS
            && (quads[end].section(), quads[end].pass(), quads[end].tile) == key
        {
            end += 1;
        }
        let offset = mesh.list.len();
        mesh.list.push(GX_QUADS);
        mesh.list.extend(((end - start) as u16 * 4).to_be_bytes());
        for quad in &quads[start..end] {
            emit(&mut mesh.list, quad.words);
        }
        // Display lists are called in 32 byte blocks, padded with GX_NOP commands
        mesh.list.resize(mesh.list.len().next_multiple_of(32), 0);
        let (section, pass, tile) = key;
        let draws = &mut mesh.sections[section][pass];
        if draws.start == draws.end {
            *draws = mesh.draws.len()..mesh.draws.len();
        }
        draws.end += 1;
        mesh.draws.push(Draw {
            tile,
            list: offset..mesh.list.len(),
        });
        start = end;
    }
    crate::memory::flush(&mesh.list);
    mesh
}

fn emit(list: &mut Vec<u8>, [a, b, shape]: [u32; 3]) {
    let origin = [
        (a & 31) as f32,
        (((a >> 5) & 31) + ((a >> 27) & 15) * SECTION_SIZE as u32) as f32,
        ((a >> 12) & 31) as f32,
    ];
    let face = (a >> 17) & 7;
    let material = quad_material(a) as u8;
    let reverse = (face & 1) ^ ((a >> 26) & 1) != 0;
    let tint = match material {
        11 => TINT_PINE,
        23 => TINT_BEDROCK,
        m if world::is_lava(m) => TINT_LAVA,
        m if world::is_water(m) => TINT_WATER,
        _ => 0,
    };
    // GX splits quads at 0-2. Rotate corners to choose the brighter diagonal.
    let light = [
        shape & 3,
        (shape >> 2) & 3,
        (shape >> 6) & 3,
        (shape >> 4) & 3,
    ];
    let flip = usize::from(light[0] + light[2] < light[1] + light[3]);
    let order = if reverse { [0, 1, 2, 3] } else { [0, 3, 2, 1] };
    let corners = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    for corner in order {
        let index = (corner + flip) % 4;
        let [cx, cy] = corners[index];
        let mut p = origin;
        let uv = if face >= 6 {
            p[0] += cx;
            p[1] += cy;
            p[2] += if face == 7 { 1.0 - cx } else { cx };
            [cx, 1.0 - cy]
        } else {
            let axis = (face / 2) as usize;
            let mut offset = [cx * (b & 31) as f32, cy * ((b >> 5) & 31) as f32];
            if shape & 256 != 0 {
                let low = [(shape >> 9) & 1, (shape >> 11) & 1].map(|v| v as f32 * 0.5);
                let high = [(shape >> 10) & 1, (shape >> 12) & 1].map(|v| 1.0 - v as f32 * 0.5);
                offset = [
                    low[0] + (high[0] - low[0]) * cx,
                    low[1] + (high[1] - low[1]) * cy,
                ];
                p[axis] += ((shape >> 13) & 3) as f32 * 0.5;
            }
            p[(axis + 1) % 3] += offset[0];
            p[(axis + 2) % 3] += offset[1];
            // Keep texture coordinates on the block grid, as the desktop shaders do.
            let uv = match axis {
                0 => [p[2], HEIGHT as f32 - p[1]],
                1 => [p[0], p[2]],
                _ => [p[0], HEIGHT as f32 - p[1]],
            };
            if world::fluid(material).is_some()
                && (face == 2 || axis == 0 && cx == 1.0 || axis == 2 && cy == 1.0)
            {
                p[1] -= 1.0 - world::fluid_height(material);
            }
            uv
        };
        // Fixed point retains sub-block shapes and fluid heights with compact vertices.
        for value in p {
            list.extend(((value * 256.0 + 0.5) as u16).to_be_bytes());
        }
        let color = (tint << 13
            | (light[index] as u16) << 11
            | (face as u16) << 8
            | ((b >> 10) & 255) as u16)
            .to_be_bytes();
        list.extend(color);
        for value in uv {
            list.extend(((value * 256.0 + 0.5) as u16).to_be_bytes());
        }
    }
}
