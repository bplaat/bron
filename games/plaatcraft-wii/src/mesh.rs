/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Turns the packed greedy quads of a chunk into a GX display list. Each vertex is a u8
//! position in the chunk, a u16 index into the light palette and u8 texture coordinates,
//! the quads are grouped by section, pass and tile so a draw is one texture.

use alloc::vec::Vec;
use core::ops::Range;

use plaatcraft_engine::tiles::tile;
use plaatcraft_engine::world::{
    self, HEIGHT, LAVA, QUAD_BYTES, SECTION_COUNT, SECTION_SIZE, WATER, quad_material,
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
pub const PALETTE_SIZE: usize = TINTS << 11;

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
    words: [u32; 2],
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
            let words = [0, 4].map(|i| u32::from_ne_bytes(bytes[i..i + 4].try_into().unwrap()));
            let face = (words[0] >> 17) & 7;
            Quad {
                words,
                tile: tile(quad_material(words[0]) as u8, face) as u8,
            }
        })
        .collect();
    quads.sort_by_key(|quad| (quad.section(), quad.pass(), quad.tile));

    let mut mesh = Mesh {
        list: Vec::with_capacity(quads.len() * 28 + 64),
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

fn emit(list: &mut Vec<u8>, [a, b]: [u32; 2]) {
    let x = (a & 31) as u8;
    let y = (((a >> 5) & 31) + ((a >> 27) & 15) * SECTION_SIZE as u32) as u8;
    let z = ((a >> 12) & 31) as u8;
    let face = (a >> 17) & 7;
    let material = quad_material(a) as u8;
    let reverse = (face & 1) ^ ((a >> 26) & 1) != 0;
    let extent = [(b & 31) as u8, ((b >> 5) & 31) as u8];
    let tint = match material {
        11 => TINT_PINE,
        23 => TINT_BEDROCK,
        LAVA => TINT_LAVA,
        WATER => TINT_WATER,
        _ => 0,
    };
    let color = (tint << 11 | (face as u16) << 8 | ((b >> 10) & 255) as u16).to_be_bytes();
    // GX culls the opposite winding of the terrain shader's triangles
    let corners: [[u8; 2]; 4] = if reverse {
        [[0, 0], [1, 0], [1, 1], [0, 1]]
    } else {
        [[0, 0], [0, 1], [1, 1], [1, 0]]
    };
    for [cx, cy] in corners {
        let mut p = [x, y, z];
        let uv = if face >= 6 {
            // Plants are two diagonal planes through the block
            p[0] += cx;
            p[1] += cy;
            p[2] += if face == 7 { 1 - cx } else { cx };
            [cx, 1 - cy]
        } else {
            let axis = (face / 2) as usize;
            p[(axis + 1) % 3] += cx * extent[0];
            p[(axis + 2) % 3] += cy * extent[1];
            // Textures repeat, so the world y can be flipped as HEIGHT - y in a u8
            let flipped = HEIGHT as u8 - p[1];
            match axis {
                0 => [p[2], flipped],
                1 => [p[0], p[2]],
                _ => [p[0], flipped],
            }
        };
        list.extend(p);
        list.extend(color);
        list.extend(uv);
    }
}
