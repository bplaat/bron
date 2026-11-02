/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Turns the packed greedy quads of a chunk into quads of four vertices, grouped by section,
//! pass and tile so a GPU draws each group with one texture. Each vertex has its position in
//! the chunk, a texture coordinate on the block grid and its index in the light palette.

use alloc::vec::Vec;
use core::ops::Range;

use super::light::{TINT_BEDROCK, TINT_LAVA, TINT_PINE, TINT_WATER};
use super::platform::Pass;
use crate::tiles::tile;
use crate::world::{self, HEIGHT, QUAD_BYTES, SECTION_SIZE, quad_material};

/// The most quads of a group, GX draws have a 16-bit vertex count.
pub const MAX_QUADS: usize = u16::MAX as usize / 4;

/// A vertex of a chunk.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vertex {
    /// The position in the chunk, in blocks.
    pub position: [f32; 3],
    /// The texture coordinate, the tiles repeat every block.
    pub uv: [f32; 2],
    /// The index in [`super::palette`]: the tint, the corner's occlusion, the face and the
    /// light byte of the quad.
    pub light: u16,
}

/// Quads of one section, pass and tile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// The section of the chunk.
    pub section: usize,
    /// The pass that draws the quads.
    pub pass: Pass,
    /// The block tile of the quads.
    pub tile: usize,
    /// The vertices of the quads in [`Mesh::vertices`], four per quad.
    pub vertices: Range<usize>,
}

/// The quads of a chunk, sorted by section, pass and tile.
#[derive(Debug, Default)]
pub struct Mesh {
    /// The vertices of all quads, each quad turns clockwise for its front.
    pub vertices: Vec<Vertex>,
    /// The groups of quads.
    pub groups: Vec<Group>,
}

struct Quad {
    words: [u32; 3],
    tile: usize,
    pass: Pass,
}

impl Quad {
    const fn section(&self) -> usize {
        ((self.words[0] >> 27) & 15) as usize
    }

    const fn key(&self) -> (usize, Pass, usize) {
        (self.section(), self.pass, self.tile)
    }
}

/// Builds the quads of a chunk's packed mesh, tiles with alpha go in the cutout pass.
pub fn build(packed: &[u8], has_alpha: impl Fn(usize) -> bool) -> Mesh {
    let mut quads: Vec<Quad> = packed
        .as_chunks::<QUAD_BYTES>()
        .0
        .iter()
        .map(|bytes| {
            let words = [0, 4, 8]
                .map(|i| u32::from_ne_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]));
            let material = quad_material(words[0]);
            let tile = tile(material as u8, (words[0] >> 17) & 7);
            let pass = if world::is_translucent(material) {
                Pass::Translucent
            } else if has_alpha(tile) {
                Pass::Cutout
            } else {
                Pass::Solid
            };
            Quad { words, tile, pass }
        })
        .collect();
    quads.sort_by_key(Quad::key);

    let mut mesh = Mesh {
        vertices: Vec::with_capacity(quads.len() * 4),
        groups: Vec::new(),
    };
    for run in quads.chunk_by(|a, b| a.key() == b.key()) {
        for part in run.chunks(MAX_QUADS) {
            let start = mesh.vertices.len();
            for quad in part {
                mesh.vertices.extend(vertices(quad.words));
            }
            let (section, pass, tile) = part[0].key();
            mesh.groups.push(Group {
                section,
                pass,
                tile,
                vertices: start..mesh.vertices.len(),
            });
        }
    }
    mesh
}

fn vertices([a, b, shape]: [u32; 3]) -> [Vertex; 4] {
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
    // GPUs split quads at 0-2. Rotate corners to choose the brighter diagonal.
    let light = [
        shape & 3,
        (shape >> 2) & 3,
        (shape >> 6) & 3,
        (shape >> 4) & 3,
    ];
    let flip = usize::from(light[0] + light[2] < light[1] + light[3]);
    let order = if reverse { [0, 1, 2, 3] } else { [0, 3, 2, 1] };
    let corners = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    order.map(|corner| {
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
        Vertex {
            position: p,
            uv,
            light: tint << 13
                | (light[index] as u16) << 11
                | (face as u16) << 8
                | ((b >> 10) & 255) as u16,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::ChunkData;

    #[test]
    fn groups_cover_all_quads_in_order() {
        let chunk = ChunkData::generate(world::WORLD_SEED, (0, 0));
        let packed = chunk.mesh();
        let mesh = build(&packed, |tile| tile % 2 == 0);
        assert_eq!(mesh.vertices.len(), packed.len() / QUAD_BYTES * 4);
        let mut next = 0;
        for pair in mesh.groups.windows(2) {
            let key = |group: &Group| (group.section, group.pass, group.tile);
            assert!(key(&pair[0]) <= key(&pair[1]));
        }
        for group in &mesh.groups {
            assert_eq!(group.vertices.start, next);
            assert!(group.vertices.len() <= MAX_QUADS * 4);
            next = group.vertices.end;
        }
        assert_eq!(next, mesh.vertices.len());
    }
}
