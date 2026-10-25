/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

pub(crate) const SIZE: usize = 64;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Material {
    BrickRed,
    CactusSide,
    CactusTop,
    Dirt,
    DirtGrass,
    GrassTop,
    Greystone,
    Lava,
    Leaves,
    Sand,
    Stone,
    StoneCoal,
    StoneDiamond,
    StoneGold,
    StoneIron,
    TrunkSide,
    TrunkTop,
    Water,
    Wood,
}

impl Material {
    pub(crate) const ALL: [Self; 19] = [
        Self::BrickRed,
        Self::CactusSide,
        Self::CactusTop,
        Self::Dirt,
        Self::DirtGrass,
        Self::GrassTop,
        Self::Greystone,
        Self::Lava,
        Self::Leaves,
        Self::Sand,
        Self::Stone,
        Self::StoneCoal,
        Self::StoneDiamond,
        Self::StoneGold,
        Self::StoneIron,
        Self::TrunkSide,
        Self::TrunkTop,
        Self::Water,
        Self::Wood,
    ];

    pub(crate) const COUNT: usize = Self::ALL.len();

    pub(crate) const fn file_name(self) -> &'static str {
        match self {
            Self::BrickRed => "brick_red.png",
            Self::CactusSide => "cactus_side.png",
            Self::CactusTop => "cactus_top.png",
            Self::Dirt => "dirt.png",
            Self::DirtGrass => "dirt_grass.png",
            Self::GrassTop => "grass_top.png",
            Self::Greystone => "greystone.png",
            Self::Lava => "lava.png",
            Self::Leaves => "leaves.png",
            Self::Sand => "sand.png",
            Self::Stone => "stone.png",
            Self::StoneCoal => "stone_coal.png",
            Self::StoneDiamond => "stone_diamond.png",
            Self::StoneGold => "stone_gold.png",
            Self::StoneIron => "stone_iron.png",
            Self::TrunkSide => "trunk_side.png",
            Self::TrunkTop => "trunk_top.png",
            Self::Water => "water.png",
            Self::Wood => "wood.png",
        }
    }

    pub(crate) const fn index(self) -> usize {
        self as usize
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Block {
    Air,
    Grass,
    Dirt,
    Sand,
    Stone,
    Greystone,
    Coal,
    Iron,
    Gold,
    Diamond,
    Lava,
    Water,
    Leaves,
    Trunk,
    Cactus,
    Brick,
    Planks,
}

impl Block {
    const fn material(self, side: FaceSide) -> Material {
        use Block as B;
        use FaceSide as S;
        use Material as M;

        match (self, side) {
            (B::Grass, S::Top) => M::GrassTop,
            (B::Grass, S::Bottom) => M::Dirt,
            (B::Grass, _) => M::DirtGrass,
            (B::Dirt, _) => M::Dirt,
            (B::Sand, _) => M::Sand,
            (B::Stone, _) => M::Stone,
            (B::Greystone, _) => M::Greystone,
            (B::Coal, _) => M::StoneCoal,
            (B::Iron, _) => M::StoneIron,
            (B::Gold, _) => M::StoneGold,
            (B::Diamond, _) => M::StoneDiamond,
            (B::Lava, _) => M::Lava,
            (B::Water, _) => M::Water,
            (B::Leaves, _) => M::Leaves,
            (B::Trunk, S::Front | S::Back | S::Right | S::Left) => M::TrunkSide,
            (B::Trunk, _) => M::TrunkTop,
            (B::Cactus, S::Front | S::Back | S::Right | S::Left) => M::CactusSide,
            (B::Cactus, _) => M::CactusTop,
            (B::Brick, _) => M::BrickRed,
            (B::Planks, _) => M::Wood,
            (B::Air, _) => unreachable!(),
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug)]
pub(crate) enum FaceSide {
    Front,
    Back,
    Right,
    Left,
    Top,
    Bottom,
}

impl FaceSide {
    const ALL: [Self; 6] = [
        Self::Front,
        Self::Back,
        Self::Right,
        Self::Left,
        Self::Top,
        Self::Bottom,
    ];

    const fn offset(self) -> [i32; 3] {
        match self {
            Self::Front => [0, 0, 1],
            Self::Back => [0, 0, -1],
            Self::Right => [1, 0, 0],
            Self::Left => [-1, 0, 0],
            Self::Top => [0, 1, 0],
            Self::Bottom => [0, -1, 0],
        }
    }

    /// Offsets from the voxel's minimum corner
    #[cfg(test)]
    pub(crate) const fn corners(self) -> [[u8; 3]; 4] {
        match self {
            Self::Front => [[0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]],
            Self::Back => [[1, 0, 0], [0, 0, 0], [0, 1, 0], [1, 1, 0]],
            Self::Right => [[1, 0, 1], [1, 0, 0], [1, 1, 0], [1, 1, 1]],
            Self::Left => [[0, 0, 0], [0, 0, 1], [0, 1, 1], [0, 1, 0]],
            Self::Top => [[0, 1, 1], [1, 1, 1], [1, 1, 0], [0, 1, 0]],
            Self::Bottom => [[0, 0, 0], [1, 0, 0], [1, 0, 1], [0, 0, 1]],
        }
    }

    #[cfg(test)]
    pub(crate) const fn light(self) -> f32 {
        // max(dot(normal, normalize([0.45, 0.78, 0.30])), 0)
        match self {
            Self::Front => 0.316_069_78,
            Self::Right => 0.474_104_67,
            Self::Top => 0.821_781_4,
            Self::Back | Self::Left | Self::Bottom => 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Face {
    pub(crate) position: [u8; 3],
    pub(crate) ambient_occlusion: u8,
    pub(crate) side: FaceSide,
}

pub(crate) struct World {
    voxels: Vec<Block>,
    heights: Vec<u8>,
    seed: u32,
    sea_level: usize,
}

impl World {
    pub(crate) fn generate(seed: u32) -> Self {
        let mut world = Self {
            voxels: vec![Block::Air; SIZE * SIZE * SIZE],
            heights: vec![0; SIZE * SIZE],
            seed,
            sea_level: 0,
        };
        world.generate_heights();
        world.generate_terrain();
        world.build_hut();
        world.grow_plants();
        world
    }

    pub(crate) fn faces_by_material(&self) -> [Vec<Face>; Material::COUNT] {
        let mut faces = core::array::from_fn(|_| Vec::new());
        for water in [false, true] {
            for z in 0..SIZE {
                for y in 0..SIZE {
                    for x in 0..SIZE {
                        let position = [x as i32, y as i32, z as i32];
                        let block = self.get(position);
                        if block == Block::Air || (block == Block::Water) != water {
                            continue;
                        }

                        let mut ao = None;
                        for side in FaceSide::ALL {
                            let offset = side.offset();
                            let neighbor = self.get([
                                position[0] + offset[0],
                                position[1] + offset[1],
                                position[2] + offset[2],
                            ]);
                            if neighbor != Block::Air
                                && !(neighbor == Block::Water && block != Block::Water)
                            {
                                continue;
                            }

                            let shade = *ao.get_or_insert_with(|| self.ambient_occlusion(position));
                            faces[block.material(side).index()].push(Face {
                                position: [x as u8, y as u8, z as u8],
                                ambient_occlusion: shade,
                                side,
                            });
                        }
                    }
                }
            }
        }
        faces
    }

    const fn index([x, y, z]: [usize; 3]) -> usize {
        (x * SIZE + y) * SIZE + z
    }

    const fn height_index(x: usize, z: usize) -> usize {
        x * SIZE + z
    }

    fn contains([x, y, z]: [i32; 3]) -> bool {
        [x, y, z]
            .into_iter()
            .all(|coordinate| (0..SIZE as i32).contains(&coordinate))
    }

    fn get(&self, [x, y, z]: [i32; 3]) -> Block {
        if !Self::contains([x, y, z]) {
            Block::Air
        } else {
            self.voxels[Self::index([x as usize, y as usize, z as usize])]
        }
    }

    fn set(&mut self, [x, y, z]: [usize; 3], block: Block) {
        self.voxels[Self::index([x, y, z])] = block;
    }

    fn hash(&self, seed: u32, [x, y, z]: [i32; 3]) -> f32 {
        let mut value = self
            .seed
            .wrapping_add(seed)
            .wrapping_add((x as u32).wrapping_mul(0x8da6_b343))
            .wrapping_add((y as u32).wrapping_mul(0xd816_3841))
            .wrapping_add((z as u32).wrapping_mul(0xcb1a_b31f));
        value ^= value >> 15;
        value = value.wrapping_mul(0x2c1b_3c6d);
        value ^= value >> 12;
        value = value.wrapping_mul(0x297a_2d39);
        value ^= value >> 15;
        value as f32 / u32::MAX as f32
    }

    fn noise(&self, seed: u32, point: [f32; 3]) -> f32 {
        let cell = point.map(f32::floor);
        let fraction = core::array::from_fn::<_, 3, _>(|axis| point[axis] - cell[axis]);
        let weight = fraction.map(|value| value * value * (3.0 - 2.0 * value));
        let [x, y, z] = cell.map(|value| value as i32);
        let mix = |from: f32, to: f32, amount: f32| from + (to - from) * amount;
        let layer = |z| {
            mix(
                mix(
                    self.hash(seed, [x, y, z]),
                    self.hash(seed, [x + 1, y, z]),
                    weight[0],
                ),
                mix(
                    self.hash(seed, [x, y + 1, z]),
                    self.hash(seed, [x + 1, y + 1, z]),
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

    fn ore(&self, [x, y, z]: [usize; 3]) -> Block {
        let point = [x as f32 * 0.14, y as f32 * 0.14, z as f32 * 0.14];
        if y < 14 && self.noise(0x0d1a_3f11, point) > 0.80 {
            Block::Diamond
        } else if y < 22 && self.noise(0x901d_5c27, point) > 0.80 {
            Block::Gold
        } else if y < 34 && self.noise(0x1201_ab73, point) > 0.78 {
            Block::Iron
        } else if self.noise(0xc0a1_5e09, point) > 0.76 {
            Block::Coal
        } else {
            Block::Stone
        }
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
                raw[Self::height_index(x, z)] = value;
                lowest = lowest.min(value);
                highest = highest.max(value);
            }
        }

        let span = if highest - lowest < 0.001 {
            1.0
        } else {
            highest - lowest
        };
        let mut histogram = [0usize; SIZE];
        for z in 0..SIZE {
            for x in 0..SIZE {
                let index = Self::height_index(x, z);
                let height = 6 + ((raw[index] - lowest) / span * 44.0) as usize;
                self.heights[index] = height as u8;
                histogram[height] += 1;
            }
        }

        let mut submerged = 0;
        for (level, count) in histogram.into_iter().enumerate() {
            submerged += count;
            if submerged >= SIZE * SIZE * 35 / 100 {
                self.sea_level = level;
                break;
            }
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
                let height = self.heights[Self::height_index(x, z)] as usize;
                let sand = height <= self.sea_level + 1 || desert > 0.63;
                let rocky = height > 40;
                let (surface, filler) = if sand {
                    (Block::Sand, Block::Sand)
                } else if rocky {
                    (Block::Greystone, Block::Stone)
                } else {
                    (Block::Grass, Block::Dirt)
                };

                for y in 0..=height {
                    let mut block = if y < 2 {
                        Block::Lava
                    } else if y < 5 {
                        Block::Greystone
                    } else if y == height {
                        surface
                    } else if y > height.saturating_sub(4) {
                        filler
                    } else {
                        self.ore([x, y, z])
                    };
                    if y > 4 && y + 2 < height {
                        let cave = self.fractal(
                            0x3c9d_7a15,
                            [x as f32 * 0.075, y as f32 * 0.11, z as f32 * 0.075],
                            2,
                        );
                        if cave > 0.615 && cave < 0.715 {
                            block = Block::Air;
                        }
                    }
                    self.set([x, y, z], block);
                }
                for y in height + 1..=self.sea_level {
                    self.set([x, y, z], Block::Water);
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
                        let (column, row) = (x + dx, z + dz);
                        let height = self.heights[Self::height_index(column, row)] as usize;
                        lowest = lowest.min(height);
                        highest = highest.max(height);
                        grass &=
                            self.get([column as i32, height as i32, row as i32]) == Block::Grass;
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
                let (column, row) = (x + dx, z + dz);
                let ground = self.heights[Self::height_index(column, row)] as usize;
                for y in ground..base {
                    self.set([column, y, row], Block::Dirt);
                }
                self.set([column, base, row], Block::Planks);
                for y in base + 1..=base + 4 {
                    self.set([column, y, row], Block::Air);
                }

                let edge_x = dx == 0 || dx == 6;
                let edge_z = dz == 0 || dz == 6;
                if edge_x || edge_z {
                    let block = if edge_x && edge_z {
                        Block::Trunk
                    } else {
                        Block::Brick
                    };
                    for y in base + 1..base + 4 {
                        self.set([column, y, row], block);
                    }
                }
                self.set([column, base + 4, row], Block::Planks);
                self.heights[Self::height_index(column, row)] = (base + 4) as u8;
            }
        }

        self.set([x + 3, base + 1, z], Block::Air);
        self.set([x + 3, base + 2, z], Block::Air);
        self.set([x, base + 2, z + 3], Block::Air);
        self.set([x + 6, base + 2, z + 3], Block::Air);
        self.set([x + 1, base + 5, z + 1], Block::Greystone);
        self.set([x + 1, base + 6, z + 1], Block::Greystone);
        self.heights[Self::height_index(x + 1, z + 1)] = (base + 6) as u8;
    }

    fn grow_plants(&mut self) {
        for z in 3..SIZE - 3 {
            for x in 3..SIZE - 3 {
                let height = self.heights[Self::height_index(x, z)] as usize;
                if height <= self.sea_level + 1
                    || self.get([x as i32, (height + 1) as i32, z as i32]) != Block::Air
                {
                    continue;
                }

                match self.get([x as i32, height as i32, z as i32]) {
                    Block::Grass if self.hash(0x4f2c_81a3, [x as i32, 0, z as i32]) > 0.98 => {
                        let trunk =
                            4 + (self.hash(0x11ee_22ff, [x as i32, 1, z as i32]) * 3.0) as usize;
                        for y in height + 1..=height + trunk {
                            self.set([x, y, z], Block::Trunk);
                        }

                        let crown = (height + trunk) as i32;
                        for dy in -2..=2 {
                            for dz in -2..=2 {
                                for dx in -2..=2 {
                                    let distance =
                                        (dx * dx + dz * dz) as f32 + (dy * dy) as f32 * 1.6;
                                    let position = [x as i32 + dx, crown + dy, z as i32 + dz];
                                    if distance <= 5.2
                                        && Self::contains(position)
                                        && self.get(position) == Block::Air
                                    {
                                        self.set(
                                            position.map(|coordinate| coordinate as usize),
                                            Block::Leaves,
                                        );
                                    }
                                }
                            }
                        }
                    }
                    Block::Sand if self.hash(0x9ab3_0f57, [x as i32, 0, z as i32]) > 0.95 => {
                        let cactus =
                            2 + (self.hash(0x6d4e_2b19, [x as i32, 0, z as i32]) * 3.0) as usize;
                        for y in height + 1..=height + cactus {
                            self.set([x, y, z], Block::Cactus);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn ambient_occlusion(&self, [x, y, z]: [i32; 3]) -> u8 {
        let mut blocked = 0;
        for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if (dx != 0 || dy != 0 || dz != 0)
                        && !matches!(
                            self.get([x + dx, y + dy, z + dz]),
                            Block::Air | Block::Water
                        )
                    {
                        blocked += 1;
                    }
                }
            }
        }
        (255.0 * (1.0 - 0.45 * blocked as f32 / 26.0)) as u8
    }
}

// Pack WebGPU storage-buffer records with opaque faces before water faces.
pub(crate) fn face_bytes(faces: &[Vec<Face>; Material::COUNT]) -> (Vec<u8>, u32, u32) {
    let count = faces.iter().map(Vec::len).sum::<usize>();
    let mut bytes = Vec::with_capacity(count * 32);
    let mut opaque = 0;
    for water in [false, true] {
        for material in Material::ALL {
            if (material == Material::Water) != water {
                continue;
            }
            for face in &faces[material.index()] {
                for value in face
                    .position
                    .map(f32::from)
                    .into_iter()
                    .chain([f32::from(face.ambient_occlusion)])
                {
                    bytes.extend_from_slice(&value.to_ne_bytes());
                }
                for value in [
                    face.side as u32,
                    material.index() as u32,
                    u32::from(water),
                    0,
                ] {
                    bytes.extend_from_slice(&value.to_ne_bytes());
                }
            }
        }
        if !water {
            opaque = (bytes.len() / 32) as u32;
        }
    }
    let count = (bytes.len() / 32) as u32;
    (bytes, opaque, count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_world() -> World {
        World {
            voxels: vec![Block::Air; SIZE * SIZE * SIZE],
            heights: vec![0; SIZE * SIZE],
            seed: 0,
            sea_level: 0,
        }
    }

    #[test]
    fn block_materials_match_the_face_direction() {
        assert_eq!(Block::Grass.material(FaceSide::Front), Material::DirtGrass);
        assert_eq!(Block::Grass.material(FaceSide::Top), Material::GrassTop);
        assert_eq!(Block::Grass.material(FaceSide::Bottom), Material::Dirt);
        assert_eq!(Block::Trunk.material(FaceSide::Top), Material::TrunkTop);
        assert_eq!(Block::Trunk.material(FaceSide::Left), Material::TrunkSide);
        assert_eq!(Block::Cactus.material(FaceSide::Top), Material::CactusTop);
        assert_eq!(
            Block::Cactus.material(FaceSide::Right),
            Material::CactusSide
        );
    }

    #[test]
    fn ambient_occlusion_counts_only_opaque_neighbors() {
        let mut world = empty_world();
        assert_eq!(world.ambient_occlusion([32, 32, 32]), 255);
        world.set([33, 32, 32], Block::Water);
        assert_eq!(world.ambient_occlusion([32, 32, 32]), 255);
        world.set([33, 32, 32], Block::Stone);
        assert_eq!(world.ambient_occlusion([32, 32, 32]), 250);
        for z in 31..=33 {
            for y in 31..=33 {
                for x in 31..=33 {
                    if [x, y, z] != [32, 32, 32] {
                        world.set([x, y, z], Block::Stone);
                    }
                }
            }
        }
        assert_eq!(world.ambient_occlusion([32, 32, 32]), 140);
    }

    #[test]
    fn water_hides_itself_but_keeps_adjacent_terrain_visible() {
        let mut world = empty_world();
        world.set([32, 32, 32], Block::Stone);
        world.set([33, 32, 32], Block::Water);
        let faces = world.faces_by_material();
        assert_eq!(faces[Material::Stone.index()].len(), 6);
        assert_eq!(faces[Material::Water.index()].len(), 5);
        world.set([34, 32, 32], Block::Water);
        let faces = world.faces_by_material();
        assert_eq!(faces[Material::Stone.index()].len(), 6);
        assert_eq!(faces[Material::Water.index()].len(), 9);
    }

    #[test]
    fn face_winding_and_light_match_the_shader_normals() {
        let sun_length = (0.45_f32 * 0.45 + 0.78 * 0.78 + 0.30 * 0.30).sqrt();
        for side in FaceSide::ALL {
            let corners = side.corners().map(|corner| corner.map(i32::from));
            let a = core::array::from_fn::<_, 3, _>(|axis| corners[1][axis] - corners[0][axis]);
            let b = core::array::from_fn::<_, 3, _>(|axis| corners[2][axis] - corners[0][axis]);
            assert_eq!(
                [
                    a[1] * b[2] - a[2] * b[1],
                    a[2] * b[0] - a[0] * b[2],
                    a[0] * b[1] - a[1] * b[0]
                ],
                side.offset()
            );
            let normal = side.offset().map(|value| value as f32);
            let expected =
                ((normal[0] * 0.45 + normal[1] * 0.78 + normal[2] * 0.30) / sun_length).max(0.0);
            assert!((side.light() - expected).abs() < 0.000_001);
        }
    }
}
