/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) const SIZE: usize = 64;
const AIR: u8 = 0;
const GRASS: u8 = 1;
const DIRT: u8 = 2;
const SAND: u8 = 3;
const STONE: u8 = 4;
const GREYSTONE: u8 = 5;
const COAL: u8 = 6;
const IRON: u8 = 7;
const GOLD: u8 = 8;
const DIAMOND: u8 = 9;
const LAVA: u8 = 10;
const WATER: u8 = 11;
const LEAVES: u8 = 12;
const TRUNK: u8 = 13;
const CACTUS: u8 = 14;
const BRICK: u8 = 15;
const PLANKS: u8 = 16;
const FACES: [[i32; 3]; 6] = [
    [0, 0, 1],
    [0, 0, -1],
    [1, 0, 0],
    [-1, 0, 0],
    [0, 1, 0],
    [0, -1, 0],
];

pub(crate) struct Face {
    pub(crate) position: [f32; 4],
    pub(crate) data: [u32; 4],
}

pub(crate) struct World {
    voxels: Vec<u8>,
    heights: Vec<u8>,
    seed: u32,
    sea: usize,
}

impl World {
    pub(crate) fn generate() -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("System time")
            .subsec_nanos();
        let mut world = Self {
            voxels: vec![AIR; SIZE * SIZE * SIZE],
            heights: vec![0; SIZE * SIZE],
            seed,
            sea: 0,
        };
        world.generate_heights();
        world.generate_terrain();
        world.build_hut();
        world.grow_plants();
        world
    }

    fn hash(&self, seed: u32, x: i32, y: i32, z: i32) -> f32 {
        let mut hash = self
            .seed
            .wrapping_add(seed)
            .wrapping_add((x as u32).wrapping_mul(0x8da6_b343))
            .wrapping_add((y as u32).wrapping_mul(0xd816_3841))
            .wrapping_add((z as u32).wrapping_mul(0xcb1a_b31f));
        hash ^= hash >> 15;
        hash = hash.wrapping_mul(0x2c1b_3c6d);
        hash ^= hash >> 12;
        hash = hash.wrapping_mul(0x297a_2d39);
        hash ^= hash >> 15;
        hash as f32 / u32::MAX as f32
    }

    fn noise(&self, seed: u32, point: [f32; 3]) -> f32 {
        let cell = point.map(f32::floor);
        let weight = std::array::from_fn::<_, 3, _>(|axis| {
            let fraction = point[axis] - cell[axis];
            fraction * fraction * (3.0 - 2.0 * fraction)
        });
        let [x, y, z] = cell.map(|value| value as i32);
        let mix = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let layer = |z| {
            mix(
                mix(
                    self.hash(seed, x, y, z),
                    self.hash(seed, x + 1, y, z),
                    weight[0],
                ),
                mix(
                    self.hash(seed, x, y + 1, z),
                    self.hash(seed, x + 1, y + 1, z),
                    weight[0],
                ),
                weight[1],
            )
        };
        mix(layer(z), layer(z + 1), weight[2])
    }

    fn fractal(&self, seed: u32, mut point: [f32; 3], octaves: usize) -> f32 {
        let (mut sum, mut total, mut amplitude) = (0.0, 0.0, 1.0);
        for octave in 0..octaves {
            sum += amplitude * self.noise(seed.wrapping_add(octave as u32 * 7919), point);
            total += amplitude;
            amplitude *= 0.5;
            point = point.map(|value| value * 2.0);
        }
        sum / total
    }

    fn generate_heights(&mut self) {
        let mut raw = vec![0.0; SIZE * SIZE];
        let (mut lowest, mut highest) = (f32::MAX, f32::MIN);
        for z in 0..SIZE {
            for x in 0..SIZE {
                let continents =
                    self.fractal(0x51a7_b3c9, [x as f32 * 0.021, 0.0, z as f32 * 0.021], 4);
                let mountains = self.fractal(
                    0x2f8e_11d5,
                    [x as f32 * 0.045 + 17.0, 17.0, z as f32 * 0.045 + 17.0],
                    3,
                );
                let value = continents + mountains.powf(4.5) * 1.2;
                raw[x + z * SIZE] = value;
                lowest = lowest.min(value);
                highest = highest.max(value);
            }
        }
        let span = if highest - lowest < 0.001 {
            1.0
        } else {
            highest - lowest
        };
        let mut histogram = [0; SIZE];
        for (height, value) in self.heights.iter_mut().zip(raw) {
            let level = 6 + ((value - lowest) / span * 44.0) as usize;
            *height = level as u8;
            histogram[level] += 1;
        }
        let mut submerged = 0;
        for (level, count) in histogram.into_iter().enumerate() {
            submerged += count;
            if submerged >= SIZE * SIZE * 35 / 100 {
                self.sea = level;
                break;
            }
        }
    }

    fn ore(&self, x: usize, y: usize, z: usize) -> u8 {
        let point = [x as f32 * 0.14, y as f32 * 0.14, z as f32 * 0.14];
        if y < 14 && self.noise(0x0d1a_3f11, point) > 0.80 {
            DIAMOND
        } else if y < 22 && self.noise(0x901d_5c27, point) > 0.80 {
            GOLD
        } else if y < 34 && self.noise(0x1201_ab73, point) > 0.78 {
            IRON
        } else if self.noise(0xc0a1_5e09, point) > 0.76 {
            COAL
        } else {
            STONE
        }
    }

    fn generate_terrain(&mut self) {
        for z in 0..SIZE {
            for x in 0..SIZE {
                let desert = self.fractal(
                    0x7b3d_90a1,
                    [x as f32 * 0.017 - 31.0, -31.0, z as f32 * 0.017 - 31.0],
                    2,
                );
                let height = self.heights[x + z * SIZE] as usize;
                let sand = height <= self.sea + 1 || desert > 0.63;
                let rocky = height > 40;
                let surface = if sand {
                    SAND
                } else if rocky {
                    GREYSTONE
                } else {
                    GRASS
                };
                let filler = if sand {
                    SAND
                } else if rocky {
                    STONE
                } else {
                    DIRT
                };
                for y in 0..=height {
                    let mut kind = if y < 2 {
                        LAVA
                    } else if y < 5 {
                        GREYSTONE
                    } else if y == height {
                        surface
                    } else if y > height.saturating_sub(4) {
                        filler
                    } else {
                        self.ore(x, y, z)
                    };
                    if y > 4 && y + 2 < height {
                        let cave = self.fractal(
                            0x3c9d_7a15,
                            [x as f32 * 0.075, y as f32 * 0.11, z as f32 * 0.075],
                            2,
                        );
                        if cave > 0.615 && cave < 0.715 {
                            kind = AIR;
                        }
                    }
                    self.set(x, y, z, kind);
                }
                for y in height + 1..=self.sea {
                    self.set(x, y, z, WATER);
                }
            }
        }
    }

    fn build_hut(&mut self) {
        let (mut best, mut best_range) = (None, 7);
        for z in 6..SIZE - 13 {
            for x in 6..SIZE - 13 {
                let (mut lowest, mut highest, mut grass) = (SIZE, 0, true);
                for dz in 0..7 {
                    for dx in 0..7 {
                        let height = self.heights[x + dx + (z + dz) * SIZE] as usize;
                        lowest = lowest.min(height);
                        highest = highest.max(height);
                        grass &= self.get((x + dx) as i32, height as i32, (z + dz) as i32) == GRASS;
                    }
                }
                if grass && highest - lowest < best_range {
                    best = Some((x, z, highest));
                    best_range = highest - lowest;
                }
            }
        }
        let Some((x, z, base)) = best else {
            return;
        };
        for dz in 0..7 {
            for dx in 0..7 {
                let column = x + dx;
                let row = z + dz;
                for y in self.heights[column + row * SIZE] as usize..base {
                    self.set(column, y, row, DIRT);
                }
                self.set(column, base, row, PLANKS);
                for y in base + 1..=base + 4 {
                    self.set(column, y, row, AIR);
                }
                let edge_x = dx == 0 || dx == 6;
                let edge_z = dz == 0 || dz == 6;
                if edge_x || edge_z {
                    for y in base + 1..base + 4 {
                        self.set(column, y, row, if edge_x && edge_z { TRUNK } else { BRICK });
                    }
                }
                self.set(column, base + 4, row, PLANKS);
                self.heights[column + row * SIZE] = (base + 4) as u8;
            }
        }
        self.set(x + 3, base + 1, z, AIR);
        self.set(x + 3, base + 2, z, AIR);
        self.set(x, base + 2, z + 3, AIR);
        self.set(x + 6, base + 2, z + 3, AIR);
        self.set(x + 1, base + 5, z + 1, GREYSTONE);
        self.set(x + 1, base + 6, z + 1, GREYSTONE);
        self.heights[x + 1 + (z + 1) * SIZE] = (base + 6) as u8;
    }

    fn grow_plants(&mut self) {
        for z in 3..SIZE - 3 {
            for x in 3..SIZE - 3 {
                let height = self.heights[x + z * SIZE] as usize;
                if height <= self.sea + 1
                    || self.get(x as i32, (height + 1) as i32, z as i32) != AIR
                {
                    continue;
                }
                let surface = self.get(x as i32, height as i32, z as i32);
                if surface == GRASS && self.hash(0x4f2c_81a3, x as i32, 0, z as i32) > 0.980 {
                    let trunk = 4 + (self.hash(0x11ee_22ff, x as i32, 1, z as i32) * 3.0) as usize;
                    for y in height + 1..=height + trunk {
                        self.set(x, y, z, TRUNK);
                    }
                    let crown = height + trunk;
                    for dy in -2..=2 {
                        for dz in -2..=2 {
                            for dx in -2..=2 {
                                let distance = (dx * dx + dz * dz) as f32 + (dy * dy) as f32 * 1.6;
                                let (px, py, pz) =
                                    (x as i32 + dx, crown as i32 + dy, z as i32 + dz);
                                if distance <= 5.2
                                    && px >= 0
                                    && py >= 0
                                    && pz >= 0
                                    && px < SIZE as i32
                                    && py < SIZE as i32
                                    && pz < SIZE as i32
                                    && self.get(px, py, pz) == AIR
                                {
                                    self.set(px as usize, py as usize, pz as usize, LEAVES);
                                }
                            }
                        }
                    }
                } else if surface == SAND && self.hash(0x9ab3_0f57, x as i32, 0, z as i32) > 0.950 {
                    let cactus = 2 + (self.hash(0x6d4e_2b19, x as i32, 0, z as i32) * 3.0) as usize;
                    for y in height + 1..=height + cactus {
                        self.set(x, y, z, CACTUS);
                    }
                }
            }
        }
    }

    fn get(&self, x: i32, y: i32, z: i32) -> u8 {
        if x < 0 || y < 0 || z < 0 || x >= SIZE as i32 || y >= SIZE as i32 || z >= SIZE as i32 {
            AIR
        } else {
            self.voxels[x as usize + y as usize * SIZE + z as usize * SIZE * SIZE]
        }
    }

    fn set(&mut self, x: usize, y: usize, z: usize, kind: u8) {
        self.voxels[x + y * SIZE + z * SIZE * SIZE] = kind;
    }

    fn ambient_occlusion(&self, x: i32, y: i32, z: i32) -> u8 {
        let mut blocked = 0;
        for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if (dx != 0 || dy != 0 || dz != 0)
                        && !matches!(self.get(x + dx, y + dy, z + dz), AIR | WATER)
                    {
                        blocked += 1;
                    }
                }
            }
        }
        (255.0 * (1.0 - 0.45 * blocked as f32 / 26.0)) as u8
    }

    pub(crate) fn faces(&self, materials: &[[u32; 3]]) -> (Vec<Face>, u32) {
        let mut faces = Vec::new();
        let mut opaque = 0;
        for water in [false, true] {
            for z in 0..SIZE {
                for y in 0..SIZE {
                    for x in 0..SIZE {
                        let kind = self.get(x as i32, y as i32, z as i32);
                        if kind == AIR || (kind == WATER) != water {
                            continue;
                        }
                        let mut occlusion = None;
                        for (face, offset) in FACES.iter().enumerate() {
                            let neighbor = self.get(
                                x as i32 + offset[0],
                                y as i32 + offset[1],
                                z as i32 + offset[2],
                            );
                            if neighbor != AIR && !(neighbor == WATER && !water) {
                                continue;
                            }
                            let shade = *occlusion.get_or_insert_with(|| {
                                self.ambient_occlusion(x as i32, y as i32, z as i32)
                            });
                            let layer = materials[kind as usize][if face == 4 {
                                1
                            } else if face == 5 {
                                2
                            } else {
                                0
                            }];
                            faces.push(Face {
                                position: [x as f32, y as f32, z as f32, f32::from(shade)],
                                data: [face as u32, layer, u32::from(water), 0],
                            });
                        }
                    }
                }
            }
            if !water {
                opaque = faces.len() as u32;
            }
        }
        (faces, opaque)
    }
}

pub(crate) fn face_bytes(faces: &[Face]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(faces.len() * 32);
    for face in faces {
        for value in face.position {
            bytes.extend_from_slice(&value.to_ne_bytes());
        }
        for value in face.data {
            bytes.extend_from_slice(&value.to_ne_bytes());
        }
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ambient_occlusion_counts_only_opaque_neighbors() {
        let mut world = World {
            voxels: vec![AIR; SIZE * SIZE * SIZE],
            heights: vec![0; SIZE * SIZE],
            seed: 0,
            sea: 0,
        };
        assert_eq!(world.ambient_occlusion(32, 32, 32), 255);

        world.set(33, 32, 32, WATER);
        assert_eq!(world.ambient_occlusion(32, 32, 32), 255);

        world.set(33, 32, 32, STONE);
        assert_eq!(world.ambient_occlusion(32, 32, 32), 250);

        for z in 31..=33 {
            for y in 31..=33 {
                for x in 31..=33 {
                    if (x, y, z) != (32, 32, 32) {
                        world.set(x, y, z, STONE);
                    }
                }
            }
        }
        assert_eq!(world.ambient_occlusion(32, 32, 32), 140);
    }
}
