/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Drawing the world with the console's GPU: the chunks lit by the light palette, fog in the
//! distance, the sky, the other players, the particles, the held block and the targeted
//! block's outline, after the shaders of the desktop game.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::f32::consts::PI;

use super::light::{encode_rgb, fog_color, mix};
use super::platform::{Fog, Gpu, Mtx, Mtx44, Pass, State, Texture, Vertex};
use super::stream::Chunk;
use crate::avatars::Others;
use crate::camera::View;
#[cfg(not(feature = "std"))]
use crate::math::Float;
use crate::particles::{Particles, SIZE};
use crate::tiles;
use crate::world::{self, CHUNK_SIZE, Coord, SECTION_COUNT};

const FOV: f32 = 75.0;
const NEAR: f32 = 0.05;
// The size of the held block, a bit smaller than in held.wgsl on the smaller TV screen.
const HELD_SIZE: f32 = 0.22;
const UVS: [[f32; 2]; 4] = [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]];
const WHITE: u32 = 0xffff_ffff;

// The faces of a unit box: +x, -x, +y, -y, +z, -z, clockwise seen from outside, and the
// texture coordinates of their corners.
const BOX_FACES: [[[f32; 3]; 4]; 6] = [
    [
        [1.0, 1.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
    ],
    [
        [0.0, 1.0, 1.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
    ],
    [
        [1.0, 1.0, 0.0],
        [1.0, 1.0, 1.0],
        [0.0, 1.0, 1.0],
        [0.0, 1.0, 0.0],
    ],
    [
        [1.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
    ],
    [
        [1.0, 1.0, 1.0],
        [1.0, 0.0, 1.0],
        [0.0, 0.0, 1.0],
        [0.0, 1.0, 1.0],
    ],
    [
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
    ],
];
const BOX_UVS: [[[f32; 2]; 4]; 6] = {
    let flipped = [UVS[3], UVS[2], UVS[1], UVS[0]];
    [UVS, UVS, flipped, flipped, UVS, UVS]
};

/// A perspective projection like libogc's `guPerspective`.
fn perspective(fov: f32, aspect: f32, near: f32, far: f32) -> Mtx44 {
    let cot = 1.0 / (fov.to_radians() * 0.5).tan();
    let depth = 1.0 / (far - near);
    [
        [cot / aspect, 0.0, 0.0, 0.0],
        [0.0, cot, 0.0, 0.0],
        [0.0, 0.0, -near * depth, -(far * near) * depth],
        [0.0, 0.0, -1.0, 0.0],
    ]
}

/// An orthographic projection of the 2D screen space like libogc's `guOrtho`, y goes down.
pub(super) fn ortho(width: f32, height: f32) -> Mtx44 {
    [
        [2.0 / width, 0.0, 0.0, -1.0],
        [0.0, -2.0 / height, 0.0, 1.0],
        [0.0, 0.0, -1.0, -1.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

// The camera looks along negative z in eye space
fn model_view(view: &View, offset: [f32; 3]) -> Mtx {
    let rows = [view.right, view.up, view.forward.map(|v| -v)];
    let relative: [f32; 3] = core::array::from_fn(|i| offset[i] - view.eye[i]);
    rows.map(|row| {
        [
            row[0],
            row[1],
            row[2],
            row[0] * relative[0] + row[1] * relative[1] + row[2] * relative[2],
        ]
    })
}

fn concat(a: &Mtx, b: &Mtx) -> Mtx {
    core::array::from_fn(|row| {
        core::array::from_fn(|column| {
            let product = (0..3).map(|k| a[row][k] * b[k][column]).sum::<f32>();
            if column == 3 {
                product + a[row][3]
            } else {
                product
            }
        })
    })
}

// Chunks are drawn relative to the player's chunk, so the floats stay small
const fn offset(coord: Coord, center: Coord) -> [f32; 3] {
    [
        ((coord.0 - center.0) * CHUNK_SIZE as i64) as f32,
        0.0,
        ((coord.1 - center.1) * CHUNK_SIZE as i64) as f32,
    ]
}

const fn rgba([r, g, b]: [u8; 3]) -> u32 {
    u32::from_be_bytes([r, g, b, 255])
}

fn quad(
    gpu: &mut impl Gpu,
    texture: Texture,
    positions: [[f32; 3]; 4],
    uvs: [[f32; 2]; 4],
    color: u32,
) {
    let vertices: [Vertex; 4] = core::array::from_fn(|i| Vertex {
        position: positions[i],
        color,
        uv: uvs[i],
    });
    gpu.quads(texture, &vertices);
}

fn draw_box(gpu: &mut impl Gpu, material: u8, bounds: [[f32; 3]; 2], daylight: f32, sprite: bool) {
    for face in 0..6 {
        if sprite && face != 4 {
            continue;
        }
        let positions = BOX_FACES[face]
            .map(|p| core::array::from_fn(|a| bounds[0][a] + (bounds[1][a] - bounds[0][a]) * p[a]));
        let light = if world::is_lava(material) {
            1.0
        } else {
            daylight.max(0.15) * [0.8, 0.8, 1.0, 0.5, 0.65, 0.65][face]
        };
        let tile = tiles::tile(material, if sprite { 6 } else { face as u32 });
        quad(
            gpu,
            Texture::Tile(tile),
            positions,
            BOX_UVS[face],
            rgba(encode_rgb([light; 3])),
        );
    }
}

/// The world's projection and light of a frame.
pub(super) struct Renderer {
    daylight: f32,
    projection: Mtx44,
    far: f32,
    fog_end: f32,
}

impl Renderer {
    pub(super) fn new(size: [f32; 2], distance: i64) -> Self {
        let far = (distance * CHUNK_SIZE as i64 * 3) as f32;
        Self {
            daylight: 1.0,
            projection: perspective(FOV, size[0] / size[1], NEAR, far),
            far,
            fog_end: (distance as f32 - 1.5) * CHUNK_SIZE as f32,
        }
    }

    /// Clears the screen to the color of the sky and the fog.
    pub(super) fn begin(&mut self, gpu: &mut impl Gpu, daylight: f32) {
        self.daylight = daylight;
        gpu.begin(encode_rgb(fog_color(daylight)), daylight);
    }

    fn world(&self, gpu: &mut impl Gpu, state: State) {
        gpu.projection(&self.projection, false);
        gpu.state(state);
    }

    pub(super) fn draw_world<G: Gpu>(
        &self,
        gpu: &mut G,
        view: &View,
        center: Coord,
        chunks: &BTreeMap<Coord, Chunk<G::Mesh>>,
        eye_material: Option<u8>,
    ) {
        // The fog of water and lava is close and colored, like the shaders' immersion fog
        let (start, end, color) = match eye_material {
            Some(m) if world::is_water(m) => (
                0.0,
                24.0,
                [0.025, 0.16, 0.23].map(|c| c * self.daylight.max(0.06)),
            ),
            Some(m) if world::is_lava(m) => (0.0, 4.0, [0.65, 0.035, 0.005]),
            _ => (self.fog_end * 0.75, self.fog_end, fog_color(self.daylight)),
        };
        gpu.fog(Some(Fog {
            start,
            end,
            color: encode_rgb(color),
            planes: [NEAR, self.far],
        }));

        // Nearest chunks first, so the depth test rejects more of the farther ones
        let mut visible: Vec<(i64, Coord, [f32; 3])> = chunks
            .keys()
            .map(|&coord| (coord, offset(coord, center)))
            .filter(|(_, offset)| view.visible(*offset))
            .map(|(coord, offset)| {
                (
                    (coord.0 - center.0).pow(2) + (coord.1 - center.1).pow(2),
                    coord,
                    offset,
                )
            })
            .collect();
        visible.sort_unstable_by_key(|&(distance, ..)| distance);

        let opaque = State {
            depth_test: true,
            depth_write: true,
            cull: true,
            ..State::default()
        };
        self.world(gpu, opaque);
        for &(_, coord, offset) in &visible {
            draw_chunk(gpu, view, &chunks[&coord].mesh, offset, Pass::Solid);
        }
        // Cutouts like leaves and plants only write depth for their visible texels
        gpu.state(State {
            cutout: true,
            ..opaque
        });
        for &(_, coord, offset) in &visible {
            draw_chunk(gpu, view, &chunks[&coord].mesh, offset, Pass::Cutout);
        }
        // Translucent blocks blend over everything else, farthest first
        gpu.state(State {
            depth_test: true,
            blend: true,
            ..State::default()
        });
        for &(_, coord, offset) in visible.iter().rev() {
            draw_chunk(gpu, view, &chunks[&coord].mesh, offset, Pass::Translucent);
        }
        gpu.fog(None);
    }

    pub(super) fn draw_sky(&self, gpu: &mut impl Gpu, view: &View, eye: Option<u8>, now: f32) {
        if eye.is_some_and(|m| world::fluid(m).is_some()) {
            return;
        }
        self.world(
            gpu,
            State {
                blend: true,
                ..State::default()
            },
        );
        let radius = self.far * 0.4;
        gpu.model_view(&model_view(view, view.eye));
        let color = rgba(encode_rgb(mix([0.06, 0.08, 0.16], [1.0; 3], self.daylight)));
        for (face, corners) in BOX_FACES.iter().enumerate() {
            let positions = corners.map(|p| p.map(|v| (v * 2.0 - 1.0) * radius));
            let layers: &[usize] = match face {
                2 => &[4, 5],
                3 => &[0],
                _ => &[1, 3, 2],
            };
            for &layer in layers {
                let uvs = if layer == 5 {
                    BOX_UVS[face].map(|[u, v]| [u * 2.0 + now * 0.0025, v * 2.0 + now * 0.00125])
                } else {
                    BOX_UVS[face]
                };
                quad(gpu, Texture::Sky(layer), positions, uvs, color);
            }
        }
        let day = self.daylight > 0.5;
        let azimuth: f32 = if day { 1.256_637 } else { -1.884_956 };
        let elevation: f32 = 0.973_894;
        let (sin_a, cos_a) = azimuth.sin_cos();
        let (sin_e, cos_e) = elevation.sin_cos();
        let body = [sin_a * cos_e, sin_e, cos_a * cos_e];
        let right = [cos_a, 0.0, -sin_a];
        let up = [-sin_e * sin_a, cos_e, -sin_e * cos_a];
        let positions = UVS.map(|[u, v]| {
            core::array::from_fn(|a| {
                radius
                    * (body[a]
                        + right[a] * (u * 2.0 - 1.0) * 0.194
                        + up[a] * (1.0 - v * 2.0) * 0.194)
            })
        });
        quad(
            gpu,
            Texture::Sky(if day { 6 } else { 7 }),
            positions,
            UVS,
            WHITE,
        );
    }

    pub(super) fn draw_players(
        &self,
        gpu: &mut impl Gpu,
        view: &View,
        center: Coord,
        others: &Others,
    ) {
        self.world(
            gpu,
            State {
                depth_test: true,
                depth_write: true,
                blend: true,
                cutout: true,
                ..State::default()
            },
        );
        let base = model_view(view, [0.0; 3]);
        let color = rgba(encode_rgb([self.daylight.max(0.15); 3]));
        for bytes in others.instances(center, 0).as_chunks::<48>().0 {
            let v: [f32; 12] = core::array::from_fn(|i| {
                f32::from_ne_bytes([
                    bytes[i * 4],
                    bytes[i * 4 + 1],
                    bytes[i * 4 + 2],
                    bytes[i * 4 + 3],
                ])
            });
            let (sy, cy) = v[3].sin_cos();
            let (sp, cp) = v[4].sin_cos();
            let model = [
                [cy, sy * sp, sy * cp, v[0]],
                [0.0, cp, -sp, v[1]],
                [-sy, cy * sp, cy * cp, v[2]],
            ];
            gpu.model_view(&concat(&base, &model));
            for (face, uvs) in BOX_FACES.into_iter().zip(BOX_UVS) {
                let positions = face.map(|p| core::array::from_fn(|a| v[9 + a] + p[a] * v[6 + a]));
                quad(
                    gpu,
                    Texture::Character(v[5] as usize),
                    positions,
                    uvs,
                    color,
                );
            }
        }
    }

    /// The block in the hand, placed and swung like held.wgsl of the desktop game. The swing
    /// progress goes from 0 to 1.
    pub(super) fn draw_held(&self, gpu: &mut impl Gpu, material: u8, progress: f32) {
        let swing = (progress * PI).sin();
        let reach = (progress.sqrt() * PI).sin();
        let (sy, cy) = (0.8 - swing * 0.35).sin_cos();
        let (sx, cx) = (0.12 + reach * 0.6).sin_cos();
        // The x rotation after the y rotation, of a block scaled around its center
        let rotation = [
            [cy, 0.0, sy],
            [sx * sy, cx, -sx * cy],
            [-cx * sy, sx, cx * cy],
        ];
        // A bit higher than in held.wgsl, so the larger hotbar on TVs doesn't cover it
        let position = [
            0.56 - reach * 0.4,
            -0.4 + (progress.sqrt() * 2.0 * PI).sin() * 0.2,
            -0.85 - swing * 0.2,
        ];
        let matrix: Mtx = core::array::from_fn(|row| {
            let [x, y, z] = rotation[row].map(|value| value * HELD_SIZE);
            [x, y, z, position[row] - (x + y + z) / 2.0]
        });
        // Drawn over the world, so only the front faces keep the faces in order
        self.world(
            gpu,
            State {
                cull: true,
                blend: true,
                ..State::default()
            },
        );
        gpu.model_view(&matrix);
        let full = [[[0.0; 3], [1.0; 3]]];
        let boxes = world::boxes(material);
        for &bounds in if boxes.is_empty() { &full[..] } else { boxes } {
            draw_box(
                gpu,
                material,
                bounds.map(|p| p.map(|v| v as f32)),
                self.daylight,
                world::is_sprite(material),
            );
        }
    }

    pub(super) fn draw_particles(
        &self,
        gpu: &mut impl Gpu,
        view: &View,
        center: Coord,
        particles: &Particles,
    ) {
        self.world(
            gpu,
            State {
                depth_test: true,
                depth_write: true,
                blend: true,
                ..State::default()
            },
        );
        for particle in &particles.particles {
            let offset = [
                (particle.position[0] - (center.0 * CHUNK_SIZE as i64) as f64) as f32,
                particle.position[1] as f32,
                (particle.position[2] - (center.1 * CHUNK_SIZE as i64) as f64) as f32,
            ];
            gpu.model_view(&model_view(view, offset));
            draw_box(
                gpu,
                particle.material,
                [[-SIZE / 2.0; 3], [SIZE / 2.0; 3]],
                self.daylight,
                false,
            );
        }
    }

    /// Outlines the targeted block like the selection shader.
    pub(super) fn draw_selection(
        &self,
        gpu: &mut impl Gpu,
        view: &View,
        center: Coord,
        block: [i64; 3],
        material: u8,
    ) {
        const GROW: f32 = 0.004;
        let size = CHUNK_SIZE as i64;
        let offset = [
            (block[0] - center.0 * size) as f32,
            block[1] as f32,
            (block[2] - center.1 * size) as f32,
        ];
        self.world(
            gpu,
            State {
                depth_test: true,
                depth_write: true,
                blend: true,
                ..State::default()
            },
        );
        gpu.model_view(&model_view(view, offset));
        let full = [[[0.0; 3], [1.0; 3]]];
        let boxes = world::boxes(material);
        let boxes = if boxes.is_empty() { &full[..] } else { boxes };
        let grow = |end: usize| if end == 0 { -GROW } else { GROW };
        let mut vertices = Vec::with_capacity(boxes.len() * 24);
        for bounds in boxes {
            for axis in 0..3 {
                for corner in 0..4 {
                    for end in 0..2 {
                        let mut p = [0.0f32; 3];
                        p[axis] = bounds[end][axis] as f32 + grow(end);
                        p[(axis + 1) % 3] =
                            bounds[corner & 1][(axis + 1) % 3] as f32 + grow(corner & 1);
                        p[(axis + 2) % 3] = bounds[(corner >> 1) & 1][(axis + 2) % 3] as f32
                            + grow((corner >> 1) & 1);
                        vertices.push(Vertex {
                            position: p,
                            color: 0x0000_00a0,
                            uv: [0.0; 2],
                        });
                    }
                }
            }
        }
        gpu.lines(&vertices);
    }
}

fn draw_chunk<G: Gpu>(gpu: &mut G, view: &View, mesh: &G::Mesh, offset: [f32; 3], pass: Pass) {
    let mut positioned = false;
    for section in 0..SECTION_COUNT {
        if !G::draws(mesh, section, pass) || !view.section_visible(offset, section) {
            continue;
        }
        // Most chunks have nothing to draw in some passes
        if !positioned {
            positioned = true;
            gpu.model_view(&model_view(view, offset));
        }
        gpu.chunk(mesh, section, pass);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ortho_maps_the_screen_corners() {
        let matrix = ortho(640.0, 480.0);
        let apply = |x: f32, y: f32| {
            [0, 1].map(|row| matrix[row][0] * x + matrix[row][1] * y + matrix[row][3])
        };
        let near = |a: [f32; 2], b: [f32; 2]| (0..2).all(|i| (a[i] - b[i]).abs() < 1e-6);
        assert!(near(apply(0.0, 0.0), [-1.0, 1.0]));
        assert!(near(apply(640.0, 480.0), [1.0, -1.0]));
    }

    #[test]
    fn concat_applies_the_right_matrix_first() {
        let translate: Mtx = [
            [1.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 2.0],
            [0.0, 0.0, 1.0, 3.0],
        ];
        let scale: Mtx = [
            [2.0, 0.0, 0.0, 0.0],
            [0.0, 2.0, 0.0, 0.0],
            [0.0, 0.0, 2.0, 0.0],
        ];
        let matrix = concat(&translate, &scale);
        assert_eq!(matrix[0], [2.0, 0.0, 0.0, 1.0]);
        assert_eq!(matrix[2], [0.0, 0.0, 2.0, 3.0]);
    }
}
