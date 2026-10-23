/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use crate::biome::{Biome, Column, Terrain, smooth};

pub(crate) const CHUNK_SIZE: usize = 16;
pub(crate) const HEIGHT: usize = 128;
pub(crate) const SECTION_SIZE: usize = 16;
pub(crate) const SECTION_COUNT: usize = HEIGHT / SECTION_SIZE;
pub(crate) const WORLD_SEED: u64 = 42;
static SEED: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
static TERRAIN: std::sync::OnceLock<Terrain> = std::sync::OnceLock::new();

pub(crate) fn set_seed(seed: u64) {
    assert_eq!(*SEED.get_or_init(|| seed), seed, "World seed changed");
}
pub(crate) const QUAD_BYTES: usize = 8;
const PADDED_SIZE: usize = CHUNK_SIZE + 2;
pub(crate) type Coord = (i64, i64);

// Material IDs are shared with the tile mapping in terrain.wgsl.
const GRASS: u8 = 1;
const DIRT: u8 = 2;
const STONE: u8 = 3;
const SAND: u8 = 4;
const SNOW: u8 = 5;
const OAK: u8 = 6;
const BIRCH: u8 = 7;
const PINE: u8 = 8;
const LEAVES: u8 = 9;
const AUTUMN_LEAVES: u8 = 10;
const PINE_LEAVES: u8 = 11;
const FIRST_PLANT: u8 = 12;
const COAL_ORE: u8 = 16;
const LEGACY_IRON_ORE: u8 = 17;
const GOLD_ORE: u8 = 18;
const DIAMOND_ORE: u8 = 19;
const SILVER_ORE: u8 = 20;
const RUBY_ORE: u8 = 42;
const DENSE_RUBY_ORE: u8 = 43;
const EMERALD_ORE: u8 = 47;
const DENSE_EMERALD_ORE: u8 = 48;
const IRON_ORE: u8 = 50;
const DENSE_IRON_ORE: u8 = 51;
const DENSE_COAL_ORE: u8 = 52;
const DENSE_DIAMOND_ORE: u8 = 53;
const DENSE_GOLD_ORE: u8 = 55;
const LEGACY_DENSE_IRON_ORE: u8 = 57;
const DENSE_SILVER_ORE: u8 = 59;
#[cfg(test)]
const ORES: [u8; 14] = [
    COAL_ORE,
    IRON_ORE,
    GOLD_ORE,
    DIAMOND_ORE,
    SILVER_ORE,
    RUBY_ORE,
    EMERALD_ORE,
    DENSE_COAL_ORE,
    DENSE_IRON_ORE,
    DENSE_GOLD_ORE,
    DENSE_DIAMOND_ORE,
    DENSE_SILVER_ORE,
    DENSE_RUBY_ORE,
    DENSE_EMERALD_ORE,
];
pub(crate) const WATER: u8 = 22;
const BEDROCK: u8 = 23;
pub(crate) const LAVA: u8 = 24;
const CACTUS: u8 = 25;
const GLASS: u8 = 62;
const GLASS_FRAME: u8 = 63;
pub(crate) const SEA_LEVEL: u8 = 48;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Edit {
    pub(crate) position: [i64; 3],
    pub(crate) material: u8,
}

pub(crate) const MATERIALS: &[(u8, &str)] = &[
    (1, "Grass"),
    (2, "Dirt"),
    (3, "Stone"),
    (4, "Sand"),
    (5, "Snow"),
    (6, "Oak log"),
    (7, "Birch log"),
    (8, "Pine log"),
    (9, "Green leaves"),
    (10, "Orange leaves"),
    (11, "Pine leaves"),
    (12, "Grass 1"),
    (13, "Grass 2"),
    (14, "Grass 3"),
    (15, "Grass 4"),
    (COAL_ORE, "Coal ore"),
    (GOLD_ORE, "Gold ore"),
    (DIAMOND_ORE, "Diamond ore"),
    (SILVER_ORE, "Silver ore"),
    (21, "Birch planks"),
    (22, "Water"),
    (24, "Lava"),
    (25, "Cactus"),
    (26, "Gray bricks"),
    (27, "Red bricks"),
    (28, "Oak planks"),
    (29, "Pine planks"),
    (30, "Blue wool"),
    (31, "Green wool"),
    (32, "Red wool"),
    (33, "White wool"),
    (34, "Dirt gravel"),
    (35, "Stone gravel"),
    (36, "Oven"),
    (37, "Red rock"),
    (38, "Gray stone"),
    (39, "Tan wool"),
    (40, "Sandy dirt"),
    (41, "Gray sand"),
    (RUBY_ORE, "Ruby ore"),
    (DENSE_RUBY_ORE, "Dense ruby ore"),
    (44, "Sandy gray stone"),
    (45, "Ice"),
    (46, "Red sand"),
    (EMERALD_ORE, "Emerald ore"),
    (DENSE_EMERALD_ORE, "Dense emerald ore"),
    (49, "Sandy red rock"),
    (IRON_ORE, "Iron ore"),
    (DENSE_IRON_ORE, "Dense iron ore"),
    (DENSE_COAL_ORE, "Dense coal ore"),
    (DENSE_DIAMOND_ORE, "Dense diamond ore"),
    (54, "Dirt stone"),
    (DENSE_GOLD_ORE, "Dense gold ore"),
    (56, "Grass stone"),
    (58, "Sandy stone"),
    (DENSE_SILVER_ORE, "Dense silver ore"),
    (60, "Snowy stone"),
    (61, "Crafting table"),
    (GLASS, "Glass"),
    (GLASS_FRAME, "Framed glass"),
];

pub(crate) fn valid_material(material: u8) -> bool {
    MATERIALS.iter().any(|&(id, _)| id == material)
}

pub(crate) fn canonical_material(material: u8) -> Option<u8> {
    let material = match material {
        LEGACY_IRON_ORE => IRON_ORE,
        LEGACY_DENSE_IRON_ORE => DENSE_IRON_ORE,
        _ => material,
    };
    valid_material(material).then_some(material)
}

fn valid_stored_block(material: u8) -> bool {
    material == 0
        || material == BEDROCK
        || canonical_material(material).is_some()
            && !(FIRST_PLANT..FIRST_PLANT + 4).contains(&material)
}

// A one-voxel halo includes terrain, ores and trees from neighboring chunks.
pub(crate) struct ChunkData {
    heights: [u8; PADDED_SIZE * PADDED_SIZE],
    blocks: Vec<u8>,
    light: Vec<u8>,
    plants: Vec<[u8; 4]>,
    max_height: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TreeStyle {
    Oak,
    Birch,
    Pine,
    Autumn,
}

struct Tree {
    x: i64,
    z: i64,
    ground: u8,
    height: i32,
    style: TreeStyle,
}

fn hash(x: i64, y: i64, z: i64, seed: u64) -> u64 {
    let mut value = (x as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ (y as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9)
        ^ (z as u64).wrapping_mul(0x94d0_49bb_1331_11eb)
        ^ seed
        ^ *SEED.get_or_init(|| WORLD_SEED);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

#[cfg(test)]
pub(crate) fn ground_height(x: i64, z: i64) -> f64 {
    f64::from(terrain_column(x, z).height)
}

fn terrain_column(x: i64, z: i64) -> Column {
    TERRAIN
        .get_or_init(|| Terrain::new(*SEED.get_or_init(|| WORLD_SEED)))
        .sample(x, z)
}

fn tree_at(cell_x: i64, cell_z: i64) -> Option<Tree> {
    let random = hash(cell_x, 0, cell_z, 7183);
    let x = cell_x * 10 + 2 + ((random >> 8) % 6) as i64;
    let z = cell_z * 10 + 2 + ((random >> 12) % 6) as i64;
    let column = terrain_column(x, z);
    let ground = column.height;
    let density = match column.biome {
        Biome::Forest => (30.0 + 60.0 * smooth(0.54, 0.75, column.humidity)) as u64,
        Biome::Plains => (4.0 + 26.0 * smooth(0.46, 0.54, column.humidity)) as u64,
        Biome::Taiga => 65,
        Biome::Mountains if ground < 82 => 20,
        _ => 0,
    };
    if random % 100 >= density
        || ground <= SEA_LEVEL + 1
        || [(1, 0), (-1, 0), (0, 1), (0, -1)]
            .into_iter()
            .any(|(dx, dz)| ground.abs_diff(terrain_column(x + dx, z + dz).height) > 2)
    {
        return None;
    }
    let style = if matches!(column.biome, Biome::Mountains | Biome::Taiga) {
        TreeStyle::Pine
    } else {
        let variant = (random >> 16) % 4;
        if column.temperature < 0.46 && variant < 2 {
            TreeStyle::Autumn
        } else if column.humidity > 0.65 && variant < 3 || variant == 3 {
            TreeStyle::Birch
        } else {
            TreeStyle::Oak
        }
    };
    let height = match style {
        TreeStyle::Oak => 5,
        TreeStyle::Birch => 7,
        TreeStyle::Pine => 8,
        TreeStyle::Autumn => 4,
    } + ((random >> 20) % 3) as i32;
    Some(Tree {
        x,
        z,
        ground,
        height,
        style,
    })
}

fn is_leaf(material: u32) -> bool {
    (u32::from(LEAVES)..=u32::from(PINE_LEAVES)).contains(&material)
}

pub(crate) const fn is_translucent(material: u32) -> bool {
    matches!(material, 22 | 62 | 63)
}

fn face_visible(material: u32, neighbor: u32) -> bool {
    // Preserve solid faces behind translucent blocks. The larger translucent ID
    // owns boundaries between unlike translucent materials to avoid coplanar faces.
    material != 0
        && (neighbor == 0
            || (is_translucent(neighbor) && (!is_translucent(material) || material > neighbor))
            || (neighbor == u32::from(LAVA)
                && material != u32::from(LAVA)
                && material != u32::from(WATER)))
}

impl ChunkData {
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut raw = Vec::new();
        raw.extend_from_slice(&self.heights);
        raw.extend_from_slice(&self.blocks);
        raw.extend_from_slice(&self.light);
        raw.extend_from_slice(&(self.max_height as u16).to_le_bytes());
        raw.extend_from_slice(&(self.plants.len() as u16).to_le_bytes());
        for plant in &self.plants {
            raw.extend_from_slice(plant);
        }
        let mut encoded = b"VXC1".to_vec();
        let mut index = 0;
        while index < raw.len() {
            let value = raw[index];
            let count = raw[index..]
                .iter()
                .take(255)
                .take_while(|&&b| b == value)
                .count();
            encoded.extend_from_slice(&[count as u8, value]);
            index += count;
        }
        encoded
    }

    pub(crate) fn decode(encoded: &[u8]) -> Result<Self, String> {
        const COLUMNS: usize = PADDED_SIZE * PADDED_SIZE;
        const BLOCKS: usize = COLUMNS * HEIGHT;
        const BASE: usize = COLUMNS + 2 * BLOCKS + 4;
        if !encoded.starts_with(b"VXC1") || !(encoded.len() - 4).is_multiple_of(2) {
            return Err("Unsupported or truncated chunk data".into());
        }
        let mut raw = Vec::with_capacity(BASE);
        for &[count, value] in encoded[4..].as_chunks::<2>().0 {
            if count == 0
                || raw.len() + usize::from(count) > BASE + CHUNK_SIZE * CHUNK_SIZE * HEIGHT * 4
            {
                return Err("Invalid chunk compression".into());
            }
            raw.resize(raw.len() + usize::from(count), value);
        }
        if raw.len() < BASE {
            return Err("Truncated chunk".into());
        }
        let heights: [u8; COLUMNS] = raw[..COLUMNS]
            .try_into()
            .expect("Validated height map length");
        let mut blocks = raw[COLUMNS..COLUMNS + BLOCKS].to_vec();
        let light = raw[COLUMNS + BLOCKS..COLUMNS + 2 * BLOCKS].to_vec();
        let max_height = usize::from(u16::from_le_bytes(
            raw[BASE - 4..BASE - 2]
                .try_into()
                .expect("Validated maximum height field"),
        ));
        let count = usize::from(u16::from_le_bytes(
            raw[BASE - 2..BASE]
                .try_into()
                .expect("Validated plant count field"),
        ));
        if raw.len() != BASE + count * 4
            || max_height > HEIGHT
            || heights.iter().any(|&h| h == 0 || usize::from(h) > HEIGHT)
            || blocks.iter().any(|&b| !valid_stored_block(b))
        {
            return Err("Invalid chunk dimensions or materials".into());
        }
        for material in &mut blocks {
            if let Some(canonical) = canonical_material(*material) {
                *material = canonical;
            }
        }
        let plants: Vec<[u8; 4]> = raw[BASE..].as_chunks::<4>().0.to_vec();
        if plants.iter().any(|&[x, y, z, m]| {
            usize::from(x) >= CHUNK_SIZE
                || usize::from(z) >= CHUNK_SIZE
                || usize::from(y) >= max_height
                || !(FIRST_PLANT..FIRST_PLANT + 4).contains(&m)
        }) || heights.iter().any(|&h| usize::from(h) > max_height)
            || blocks
                .as_chunks::<HEIGHT>()
                .0
                .iter()
                .any(|column| column[max_height..].iter().any(|&b| b != 0))
        {
            return Err("Invalid chunk height or plants".into());
        }
        Ok(Self {
            heights,
            blocks,
            light,
            plants,
            max_height,
        })
    }

    pub(crate) fn generate(coord: Coord) -> Self {
        Self::generate_edited(coord, &[])
    }

    pub(crate) fn generate_edited(coord: Coord, edits: &[Edit]) -> Self {
        Self::rebuild(coord, std::collections::HashMap::new(), edits)
    }

    pub(crate) fn rebuild(
        coord: Coord,
        mut bases: std::collections::HashMap<Coord, Self>,
        edits: &[Edit],
    ) -> Self {
        let mut chunk = bases
            .remove(&coord)
            .unwrap_or_else(|| Self::generate_unlit(coord, false));
        chunk.apply_edits(coord, edits);
        chunk.bake_light(coord, edits, &mut bases);
        chunk
    }

    pub(crate) fn voxel(&self, position: [i32; 3]) -> u8 {
        if position[0] < 0
            || position[0] >= CHUNK_SIZE as i32
            || position[2] < 0
            || position[2] >= CHUNK_SIZE as i32
            || position[1] < 0
            || position[1] >= HEIGHT as i32
        {
            return 0;
        }
        let [x, y, z] = position;
        self.plants
            .iter()
            .find(|p| p[..3] == [x as u8, y as u8, z as u8])
            .map_or_else(|| self.block(position) as u8, |p| p[3])
    }

    pub(crate) fn apply_edits(&mut self, coord: Coord, edits: &[Edit]) {
        for edit in edits {
            let [wx, y, wz] = edit.position;
            let x = wx - coord.0 * CHUNK_SIZE as i64;
            let z = wz - coord.1 * CHUNK_SIZE as i64;
            if !(-1..=CHUNK_SIZE as i64).contains(&x)
                || !(-1..=CHUNK_SIZE as i64).contains(&z)
                || !(0..HEIGHT as i64).contains(&y)
            {
                continue;
            }
            let index = ((z + 1) as usize * PADDED_SIZE + (x + 1) as usize) * HEIGHT + y as usize;
            let plant = (FIRST_PLANT..FIRST_PLANT + 4).contains(&edit.material);
            self.blocks[index] = if plant { 0 } else { edit.material };
            self.plants
                .retain(|p| [i64::from(p[0]), i64::from(p[1]), i64::from(p[2])] != [x, y, z]);
            if plant && (0..CHUNK_SIZE as i64).contains(&x) && (0..CHUNK_SIZE as i64).contains(&z) {
                self.plants.push([x as u8, y as u8, z as u8, edit.material]);
            }
            self.max_height = self.max_height.max(y as usize + 1);
        }
    }

    fn generate_unlit(coord: Coord, lighting_only: bool) -> Self {
        let origin = (coord.0 * CHUNK_SIZE as i64, coord.1 * CHUNK_SIZE as i64);
        let columns = std::array::from_fn(|index| {
            terrain_column(
                origin.0 + (index % PADDED_SIZE) as i64 - 1,
                origin.1 + (index / PADDED_SIZE) as i64 - 1,
            )
        });
        let mut chunk = Self::from_columns(columns);
        if !lighting_only {
            chunk.add_outcrops(origin);
            chunk.add_ores(origin);
        }
        // Include every root whose radius-four canopy can touch the halo.
        for cell_z in
            (origin.1 - 5).div_euclid(10)..=(origin.1 + CHUNK_SIZE as i64 + 4).div_euclid(10)
        {
            for cell_x in
                (origin.0 - 5).div_euclid(10)..=(origin.0 + CHUNK_SIZE as i64 + 4).div_euclid(10)
            {
                if let Some(tree) = tree_at(cell_x, cell_z) {
                    chunk.add_tree(&tree, origin);
                }
            }
        }
        chunk.add_cacti(&columns, origin);
        chunk.add_lava(origin);
        if lighting_only {
            return chunk;
        }
        for z in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                let y = chunk.heights[(z + 1) * PADDED_SIZE + x + 1];
                let random = hash(origin.0 + x as i64, 0, origin.1 + z as i64, 9341);
                let column = columns[(z + 1) * PADDED_SIZE + x + 1];
                let density = match column.biome {
                    Biome::Plains => 50,
                    Biome::Forest => 25,
                    Biome::Taiga => 15,
                    Biome::Mountains => 10,
                    _ => 0,
                };
                if random % 100 < density
                    && chunk.block([x as i32, i32::from(y) - 1, z as i32]) == u32::from(GRASS)
                    && chunk.block([x as i32, i32::from(y), z as i32]) == 0
                {
                    chunk.plants.push([
                        x as u8,
                        y,
                        z as u8,
                        FIRST_PLANT + ((random >> 8) % 4) as u8,
                    ]);
                    chunk.max_height = chunk.max_height.max(usize::from(y) + 1);
                }
            }
        }
        chunk
    }

    #[cfg(test)]
    pub(crate) fn from_heights(heights: [u8; PADDED_SIZE * PADDED_SIZE]) -> Self {
        Self::from_columns(heights.map(|height| Column {
            height,
            biome: Biome::Plains,
            ..Column::default()
        }))
    }

    fn from_columns(columns: [Column; PADDED_SIZE * PADDED_SIZE]) -> Self {
        let heights = columns.map(|column| column.height);
        let mut blocks = vec![0; PADDED_SIZE * PADDED_SIZE * HEIGHT];
        for (index, column) in columns.iter().enumerate() {
            let height = usize::from(column.height);
            let sandy = matches!(
                column.biome,
                Biome::Sea | Biome::River | Biome::Desert | Biome::Beach
            ) || column.height <= SEA_LEVEL + 1;
            let surface = if sandy {
                SAND
            } else if matches!(column.biome, Biome::Taiga | Biome::Tundra)
                || height as f64 >= 78.0 + column.temperature * 28.0
            {
                SNOW
            } else if column.biome == Biome::Mountains && column.ruggedness > 0.45 && height >= 76 {
                STONE
            } else {
                GRASS
            };
            for y in 0..height {
                blocks[index * HEIGHT + y] = if y < 3 {
                    BEDROCK
                } else if y == height - 1 {
                    surface
                } else if y >= height.saturating_sub(4) {
                    if sandy {
                        SAND
                    } else if surface == STONE {
                        STONE
                    } else {
                        DIRT
                    }
                } else {
                    STONE
                };
            }
            if column.height < SEA_LEVEL {
                blocks[index * HEIGHT + height..index * HEIGHT + usize::from(SEA_LEVEL)]
                    .fill(WATER);
            }
        }
        let max_height =
            usize::from((*heights.iter().max().expect("Chunk has columns")).max(SEA_LEVEL));
        Self {
            heights,
            light: vec![15; blocks.len()],
            blocks,
            plants: Vec::new(),
            max_height,
        }
    }

    fn block(&self, [x, y, z]: [i32; 3]) -> u32 {
        if y < 0 || y >= HEIGHT as i32 {
            return 0;
        }
        u32::from(
            self.blocks[((z + 1) as usize * PADDED_SIZE + (x + 1) as usize) * HEIGHT + y as usize],
        )
    }

    fn put(&mut self, [x, y, z]: [i32; 3], material: u8, replace_leaves: bool) {
        if x < -1
            || x > CHUNK_SIZE as i32
            || z < -1
            || z > CHUNK_SIZE as i32
            || y < 0
            || y >= HEIGHT as i32
        {
            return;
        }
        let index = ((z + 1) as usize * PADDED_SIZE + (x + 1) as usize) * HEIGHT + y as usize;
        if self.blocks[index] == 0 || (replace_leaves && is_leaf(u32::from(self.blocks[index]))) {
            self.blocks[index] = material;
            self.max_height = self.max_height.max(y as usize + 1);
        }
    }

    fn add_cacti(&mut self, columns: &[Column; PADDED_SIZE * PADDED_SIZE], origin: Coord) {
        for (index, column) in columns.iter().enumerate() {
            if column.biome != Biome::Desert || column.height <= SEA_LEVEL + 1 {
                continue;
            }
            let x = (index % PADDED_SIZE) as i32 - 1;
            let z = (index / PADDED_SIZE) as i32 - 1;
            let wx = origin.0 + i64::from(x);
            let wz = origin.1 + i64::from(z);
            // One jittered candidate per six-block cell keeps cacti apart across chunks.
            let random = hash(wx.div_euclid(6), 0, wz.div_euclid(6), 5821);
            if random % 100 >= 5
                || wx.rem_euclid(6) != 1 + ((random >> 8) % 4) as i64
                || wz.rem_euclid(6) != 1 + ((random >> 16) % 4) as i64
            {
                continue;
            }
            let ground = i32::from(column.height);
            let height = 1 + ((random >> 24) % 3) as i32;
            if ground + height > HEIGHT as i32
                || self.block([x, ground - 1, z]) != u32::from(SAND)
                || (ground..ground + height).any(|y| self.block([x, y, z]) != 0)
                || [(1, 0), (-1, 0), (0, 1), (0, -1)]
                    .into_iter()
                    .any(|(dx, dz)| terrain_column(wx + dx, wz + dz).height > column.height)
            {
                continue;
            }
            for y in ground..ground + height {
                self.put([x, y, z], CACTUS, false);
            }
        }
    }

    fn add_tree(&mut self, tree: &Tree, origin: Coord) {
        let x = (tree.x - origin.0) as i32;
        let z = (tree.z - origin.1) as i32;
        let ground = i32::from(tree.ground);
        let wood = match tree.style {
            TreeStyle::Birch => BIRCH,
            TreeStyle::Pine => PINE,
            _ => OAK,
        };
        let leaves = match tree.style {
            TreeStyle::Autumn => AUTUMN_LEAVES,
            TreeStyle::Pine => PINE_LEAVES,
            _ => LEAVES,
        };
        for y in 0..=tree.height {
            self.put([x, ground + y, z], wood, true);
        }
        if tree.style == TreeStyle::Autumn {
            for dx in [-2, -1, 1, 2] {
                self.put([x + dx, ground + tree.height - 1, z], wood, true);
            }
        }
        for y in 2..=tree.height + 2 {
            let radius: i32 = match tree.style {
                TreeStyle::Oak => match y - tree.height {
                    -2 => 2,
                    -1 | 0 => 3,
                    1 => 2,
                    2 => 1,
                    _ => continue,
                },
                TreeStyle::Birch => match y - tree.height {
                    -2 | -1 => 2,
                    0 | 1 => 1,
                    _ => continue,
                },
                TreeStyle::Pine => {
                    if y > tree.height + 1 {
                        continue;
                    }
                    if y >= tree.height {
                        1
                    } else {
                        (1 + (tree.height - y) / 3).min(3)
                    }
                }
                TreeStyle::Autumn => match y - tree.height {
                    -1 | 0 => 4,
                    1 => 3,
                    2 => 1,
                    _ => continue,
                },
            };
            for dz in -radius..=radius {
                for dx in -radius..=radius {
                    if dx * dx + dz * dz <= radius * radius + 1 {
                        self.put([x + dx, ground + y, z + dz], leaves, false);
                    }
                }
            }
        }
    }

    fn add_lava(&mut self, origin: Coord) {
        // Rare, small lava vents provide natural block-light sources on rocky slopes.
        for cz in (origin.1 - 3).div_euclid(32)..=(origin.1 + 18).div_euclid(32) {
            for cx in (origin.0 - 3).div_euclid(32)..=(origin.0 + 18).div_euclid(32) {
                let random = hash(cx, 0, cz, 3167);
                if !random.is_multiple_of(12) {
                    continue;
                }
                let wx = cx * 32 + 8 + ((random >> 8) % 16) as i64;
                let wz = cz * 32 + 8 + ((random >> 16) % 16) as i64;
                let column = terrain_column(wx, wz);
                if column.biome != Biome::Mountains || column.height <= SEA_LEVEL + 4 {
                    continue;
                }
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        if dx * dx + dz * dz > 1 {
                            continue;
                        }
                        let x = (wx - origin.0) as i32 + dx;
                        let z = (wz - origin.1) as i32 + dz;
                        if !(-1..=16).contains(&x) || !(-1..=16).contains(&z) {
                            continue;
                        }
                        let index = (z + 1) as usize * PADDED_SIZE + (x + 1) as usize;
                        let ground = self.heights[index];
                        if ground == column.height && self.block([x, i32::from(ground), z]) == 0 {
                            self.blocks[index * HEIGHT + usize::from(ground) - 1] = LAVA;
                        }
                    }
                }
            }
        }
    }

    fn bake_light(
        &mut self,
        coord: Coord,
        edits: &[Edit],
        bases: &mut std::collections::HashMap<Coord, Self>,
    ) {
        // A 16-block margin covers the entire 15-level propagation radius, including
        // the mesh halo. No neighbor loading or later remeshing is necessary.
        const WIDTH: usize = CHUNK_SIZE * 3;
        let mut blocks = vec![0; WIDTH * WIDTH * HEIGHT];
        for dz in -1..=1 {
            for dx in -1..=1 {
                let mut neighbor;
                let source = if dx == 0 && dz == 0 {
                    &*self
                } else {
                    neighbor = bases
                        .remove(&(coord.0 + dx, coord.1 + dz))
                        .unwrap_or_else(|| {
                            Self::generate_unlit((coord.0 + dx, coord.1 + dz), true)
                        });
                    neighbor.apply_edits((coord.0 + dx, coord.1 + dz), edits);
                    &neighbor
                };
                for z in 0..CHUNK_SIZE {
                    for x in 0..CHUNK_SIZE {
                        let from = ((z + 1) * PADDED_SIZE + x + 1) * HEIGHT;
                        let to = (((dz + 1) as usize * CHUNK_SIZE + z) * WIDTH
                            + (dx + 1) as usize * CHUNK_SIZE
                            + x)
                            * HEIGHT;
                        blocks[to..to + HEIGHT]
                            .copy_from_slice(&source.blocks[from..from + HEIGHT]);
                    }
                }
            }
        }
        let light = crate::lighting::bake(
            &blocks,
            WIDTH,
            HEIGHT,
            |material| match material {
                0 => 0,
                WATER => 2,
                GLASS | GLASS_FRAME => 1,
                LEAVES..=PINE_LEAVES => 1,
                _ => 15,
            },
            |material| if material == LAVA { 15 } else { 0 },
        );
        for z in 0..PADDED_SIZE {
            for x in 0..PADDED_SIZE {
                let from = ((z + CHUNK_SIZE - 1) * WIDTH + x + CHUNK_SIZE - 1) * HEIGHT;
                let to = (z * PADDED_SIZE + x) * HEIGHT;
                self.light[to..to + HEIGHT].copy_from_slice(&light[from..from + HEIGHT]);
            }
        }
    }

    fn face_key(&self, material: u32, neighbor: [i32; 3]) -> i32 {
        let mut light = self.light_at(neighbor);
        if material == u32::from(LAVA) {
            light |= 0xf0;
        }
        (material | u32::from(light) << 8) as i32
    }

    fn light_at(&self, [x, y, z]: [i32; 3]) -> u8 {
        if y >= HEIGHT as i32 {
            return 15;
        }
        if y < 0 {
            return 0;
        }
        self.light[((z + 1) as usize * PADDED_SIZE + (x + 1) as usize) * HEIGHT + y as usize]
    }

    fn add_outcrops(&mut self, origin: Coord) {
        // Small exposed rock patches make common veins visible without digging.
        for z in -1..=CHUNK_SIZE as i32 {
            for x in -1..=CHUNK_SIZE as i32 {
                let wx = origin.0 + i64::from(x);
                let wz = origin.1 + i64::from(z);
                let random = hash(wx.div_euclid(8), 0, wz.div_euclid(8), 9914);
                let dx = wx.rem_euclid(8) - 4;
                let dz = wz.rem_euclid(8) - 4;
                let column = (z + 1) as usize * PADDED_SIZE + (x + 1) as usize;
                let height = usize::from(self.heights[column]);
                if random.is_multiple_of(9)
                    && dx * dx + dz * dz <= 10
                    && (54..76).contains(&height)
                    && self.blocks[column * HEIGHT + height - 1] == GRASS
                {
                    self.blocks[column * HEIGHT + height - 4..column * HEIGHT + height].fill(STONE);
                }
            }
        }
    }

    fn add_ores(&mut self, origin: Coord) {
        // Coarse global cells seed small spherical veins, including across chunk boundaries.
        for cell_z in
            (origin.1 - 3).div_euclid(6)..=(origin.1 + CHUNK_SIZE as i64 + 2).div_euclid(6)
        {
            for cell_x in
                (origin.0 - 3).div_euclid(6)..=(origin.0 + CHUNK_SIZE as i64 + 2).div_euclid(6)
            {
                for cell_y in 0..(HEIGHT as i64 / 6 + 1) {
                    let random = hash(cell_x, cell_y, cell_z, 5729);
                    if !random.is_multiple_of(4) {
                        continue;
                    }
                    let x = cell_x * 6 + ((random >> 8) % 6) as i64 - origin.0;
                    let y = (cell_y * 6 + ((random >> 12) % 6) as i64) as i32;
                    let z = cell_z * 6 + ((random >> 16) % 6) as i64 - origin.1;
                    let common = if y < 16 {
                        [
                            COAL_ORE,
                            IRON_ORE,
                            GOLD_ORE,
                            DIAMOND_ORE,
                            SILVER_ORE,
                            RUBY_ORE,
                            EMERALD_ORE,
                        ][((random >> 20) % 7) as usize]
                    } else if y < 28 {
                        [
                            COAL_ORE,
                            IRON_ORE,
                            GOLD_ORE,
                            SILVER_ORE,
                            RUBY_ORE,
                            EMERALD_ORE,
                        ][((random >> 20) % 6) as usize]
                    } else {
                        [COAL_ORE, IRON_ORE][((random >> 20) % 2) as usize]
                    };
                    let material = if (random >> 28).is_multiple_of(8) {
                        match common {
                            COAL_ORE => DENSE_COAL_ORE,
                            IRON_ORE => DENSE_IRON_ORE,
                            GOLD_ORE => DENSE_GOLD_ORE,
                            DIAMOND_ORE => DENSE_DIAMOND_ORE,
                            SILVER_ORE => DENSE_SILVER_ORE,
                            RUBY_ORE => DENSE_RUBY_ORE,
                            EMERALD_ORE => DENSE_EMERALD_ORE,
                            _ => unreachable!(),
                        }
                    } else {
                        common
                    };
                    for dz in -2..=2 {
                        for dy in -2..=2 {
                            for dx in -2..=2 {
                                if dx * dx + dy * dy + dz * dz > 5 {
                                    continue;
                                }
                                let [px, py, pz] = [x as i32 + dx, y + dy, z as i32 + dz];
                                if px < -1
                                    || px > CHUNK_SIZE as i32
                                    || pz < -1
                                    || pz > CHUNK_SIZE as i32
                                    || py < 0
                                    || py >= HEIGHT as i32
                                {
                                    continue;
                                }
                                let index = ((pz + 1) as usize * PADDED_SIZE + (px + 1) as usize)
                                    * HEIGHT
                                    + py as usize;
                                if self.blocks[index] == STONE {
                                    self.blocks[index] = material;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn mesh(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(256 * QUAD_BYTES);
        for section in 0..self.max_height.div_ceil(SECTION_SIZE) {
            let origin = [0, (section * SECTION_SIZE) as i32, 0];
            let dimensions = [CHUNK_SIZE, SECTION_SIZE, CHUNK_SIZE];
            let mut mask = [0i32; CHUNK_SIZE * SECTION_SIZE];
            // Sweep each axis; merge adjacent coplanar faces of the same material.
            for axis in 0..3 {
                let u = (axis + 1) % 3;
                let v = (axis + 2) % 3;
                let width = dimensions[u];
                let height = dimensions[v];
                for slice in 0..=dimensions[axis] {
                    for j in 0..height {
                        for i in 0..width {
                            let mut a = origin;
                            a[axis] += slice as i32 - 1;
                            a[u] += i as i32;
                            a[v] += j as i32;
                            let mut b = a;
                            b[axis] += 1;
                            let left = self.block(a);
                            let right = self.block(b);
                            // Neighbors come from the whole world, but only this chunk owns its faces.
                            mask[j * width + i] = if face_visible(left, right) && slice > 0 {
                                self.face_key(left, b)
                            } else if face_visible(right, left) && slice < dimensions[axis] {
                                -self.face_key(right, a)
                            } else {
                                0
                            };
                        }
                    }
                    for j in 0..height {
                        let mut i = 0;
                        while i < width {
                            let material = mask[j * width + i];
                            if material == 0 {
                                i += 1;
                                continue;
                            }
                            let mut w = 1;
                            while i + w < width && mask[j * width + i + w] == material {
                                w += 1;
                            }
                            let mut h = 1;
                            while j + h < height
                                && (0..w).all(|k| mask[(j + h) * width + i + k] == material)
                            {
                                h += 1;
                            }
                            let mut position = origin;
                            position[axis] += slice as i32;
                            position[u] += i as i32;
                            position[v] += j as i32;
                            let face = axis as u32 * 2 + u32::from(material < 0);
                            position[1] -= origin[1];
                            let mut quad =
                                pack_quad(position, face, [w, h], material.unsigned_abs() & 255);
                            quad[0] |= (section as u32) << 27;
                            quad[1] |= (material.unsigned_abs() >> 8) << 10;
                            for value in quad {
                                bytes.extend_from_slice(&value.to_ne_bytes());
                            }
                            for row in j..j + h {
                                mask[row * width + i..row * width + i + w].fill(0);
                            }
                            i += w;
                        }
                    }
                }
            }
        }
        // Two intersecting planes, with a reverse-wound copy of each plane.
        for &[x, y, z, material] in &self.plants {
            for face in [6, 7] {
                for back in [false, true] {
                    let mut quad = pack_quad(
                        [
                            i32::from(x),
                            i32::from(y) % SECTION_SIZE as i32,
                            i32::from(z),
                        ],
                        face,
                        [1, 1],
                        u32::from(material),
                    );
                    quad[1] |=
                        u32::from(self.light_at([i32::from(x), i32::from(y), i32::from(z)])) << 10;
                    quad[0] |= u32::from(back) << 26 | (u32::from(y) / SECTION_SIZE as u32) << 27;
                    for word in quad {
                        bytes.extend_from_slice(&word.to_ne_bytes());
                    }
                }
            }
        }
        bytes
            .as_chunks_mut::<QUAD_BYTES>()
            .0
            .sort_unstable_by_key(|quad| {
                let packed = u32::from_ne_bytes(quad[..4].try_into().expect("Packed quad word"));
                ((packed >> 27) & 15, is_translucent((packed >> 20) & 63))
            });
        bytes
    }
}

// First word: x:5, local y:7, z:5, face:3, material:6, reverse winding:1, section:4.
// Second word: width:5, height:5, sky:4, block light:4, GPU chunk slot:14.
const fn pack_quad(position: [i32; 3], face: u32, extent: [usize; 2], material: u32) -> [u32; 2] {
    [
        position[0] as u32
            | (position[1] as u32) << 5
            | (position[2] as u32) << 12
            | face << 17
            | material << 20,
        extent[0] as u32 | (extent[1] as u32) << 5,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_catalog_is_sorted_unique_and_fits_the_packed_format() {
        assert!(MATERIALS.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert!(
            MATERIALS
                .iter()
                .all(|&(material, name)| material <= 63 && !name.is_empty())
        );
        assert_eq!(MATERIALS.last().map(|&(material, _)| material), Some(63));
        assert_eq!(canonical_material(LEGACY_IRON_ORE), Some(IRON_ORE));
        assert_eq!(
            canonical_material(LEGACY_DENSE_IRON_ORE),
            Some(DENSE_IRON_ORE)
        );
        assert!(!valid_material(LEGACY_IRON_ORE));
        assert!(!valid_material(LEGACY_DENSE_IRON_ORE));
    }

    #[test]
    fn edited_boundary_blocks_and_light_agree_and_grass_can_be_removed() {
        let edits = [
            Edit {
                position: [-1, 120, 0],
                material: LAVA,
            },
            Edit {
                position: [0, 121, 0],
                material: FIRST_PLANT,
            },
        ];
        let left = ChunkData::generate_edited((-1, 0), &edits);
        let right = ChunkData::generate_edited((0, 0), &edits);
        assert_eq!(left.voxel([15, 120, 0]), LAVA);
        assert_eq!(right.block([-1, 120, 0]), u32::from(LAVA));
        assert!(right.light_at([0, 120, 0]) >> 4 > 0);
        assert_eq!(left.light_at([16, 120, 0]), right.light_at([0, 120, 0]));
        assert_eq!(right.voxel([0, 121, 0]), FIRST_PLANT);
        let mut restored = ChunkData::decode(&right.encode()).unwrap();
        restored.apply_edits(
            (0, 0),
            &[Edit {
                position: [0, 121, 0],
                material: 0,
            }],
        );
        assert_eq!(restored.voxel([0, 121, 0]), 0);
        restored.apply_edits(
            (0, 0),
            &[Edit {
                position: [0, 121, 0],
                material: STONE,
            }],
        );
        assert_eq!(restored.voxel([0, 121, 0]), STONE);
    }

    #[test]
    fn chunk_storage_roundtrips_blocks_lighting_plants_and_mesh() {
        for coord in [(0, 0), (-17, 23)] {
            let chunk = ChunkData::generate(coord);
            let encoded = chunk.encode();
            let restored = ChunkData::decode(&encoded).unwrap();
            assert_eq!(restored.blocks, chunk.blocks);
            assert_eq!(restored.light, chunk.light);
            assert_eq!(restored.heights, chunk.heights);
            assert_eq!(restored.plants, chunk.plants);
            assert_eq!(restored.mesh(), chunk.mesh());
            assert!(encoded.len() < chunk.blocks.len() + chunk.light.len());
            assert!(ChunkData::decode(&encoded[..encoded.len() - 1]).is_err());
        }
        assert!(ChunkData::decode(b"VXC1").is_err());
        assert!(ChunkData::decode(b"VXC1\0\x01").is_err());
        assert!(ChunkData::decode(b"VXC2").is_err());
        let mut oversized = b"VXC1".to_vec();
        oversized.extend_from_slice(&[255; 2000]);
        assert!(ChunkData::decode(&oversized).is_err());

        let mut construction = ChunkData::from_heights([60; PADDED_SIZE * PADDED_SIZE]);
        construction.apply_edits(
            (0, 0),
            &[Edit {
                position: [8, 60, 8],
                material: 38,
            }],
        );
        let restored = ChunkData::decode(&construction.encode()).unwrap();
        assert_eq!(restored.voxel([8, 60, 8]), 38);
    }

    #[test]
    fn legacy_iron_materials_are_normalized_during_decode() {
        let mut chunk = ChunkData::from_heights([60; PADDED_SIZE * PADDED_SIZE]);
        chunk.blocks[3] = LEGACY_IRON_ORE;
        chunk.blocks[4] = LEGACY_DENSE_IRON_ORE;

        let restored = ChunkData::decode(&chunk.encode()).unwrap();

        assert_eq!(restored.blocks[3], IRON_ORE);
        assert_eq!(restored.blocks[4], DENSE_IRON_ORE);
    }

    #[test]
    fn all_biomes_generate_with_consistent_water_and_surface_materials() {
        let biomes = [
            Biome::Sea,
            Biome::Plains,
            Biome::Forest,
            Biome::Desert,
            Biome::River,
            Biome::Mountains,
            Biome::Beach,
            Biome::Taiga,
            Biome::Tundra,
        ];
        let mut examples = [None; 9];
        let mut snowy = false;
        for z in (-2048..=2048).step_by(32) {
            for x in (-2048..=2048).step_by(32) {
                let column = terrain_column(x, z);
                let index = biomes.iter().position(|&b| b == column.biome).unwrap();
                examples[index].get_or_insert((x, z));
                snowy |= column.biome == Biome::Mountains && column.height >= 88;
            }
        }
        assert!(snowy);
        for (biome, example) in biomes.into_iter().zip(examples) {
            let (x, z) = example.unwrap_or_else(|| panic!("Missing biome {biome:?}"));
            let coord = (x.div_euclid(16), z.div_euclid(16));
            let chunk = ChunkData::generate(coord);
            let right = ChunkData::generate((coord.0 + 1, coord.1));
            for z in 0..CHUNK_SIZE as i32 {
                for x in 0..CHUNK_SIZE as i32 {
                    let column =
                        terrain_column(coord.0 * 16 + i64::from(x), coord.1 * 16 + i64::from(z));
                    for y in i32::from(column.height)..i32::from(SEA_LEVEL) {
                        assert_eq!(chunk.block([x, y, z]), u32::from(WATER));
                    }
                    if column.biome == Biome::Desert {
                        assert_eq!(
                            chunk.block([x, i32::from(column.height) - 1, z]),
                            u32::from(SAND)
                        );
                    }
                }
                for y in 0..HEIGHT as i32 {
                    assert_eq!(chunk.block([16, y, z]), right.block([0, y, z]));
                    assert_eq!(chunk.block([15, y, z]), right.block([-1, y, z]));
                }
            }
        }
    }

    #[test]
    fn desert_cacti_grow_on_sand_and_match_neighbor_halos() {
        let mut heights = [false; 3];
        for z in (-2048i64..=2048).step_by(128) {
            for x in (-2048i64..=2048).step_by(128) {
                if terrain_column(x, z).biome != Biome::Desert {
                    continue;
                }
                let coord = (x.div_euclid(16), z.div_euclid(16));
                let chunk = ChunkData::generate_unlit(coord, false);
                let neighbor = ChunkData::generate_unlit((coord.0 + 1, coord.1), true);
                for z in 0..CHUNK_SIZE as i32 {
                    for y in 0..HEIGHT as i32 {
                        assert_eq!(
                            chunk.block([16, y, z]) == u32::from(CACTUS),
                            neighbor.block([0, y, z]) == u32::from(CACTUS)
                        );
                        assert_eq!(
                            chunk.block([15, y, z]) == u32::from(CACTUS),
                            neighbor.block([-1, y, z]) == u32::from(CACTUS)
                        );
                    }
                    for x in 0..CHUNK_SIZE as i32 {
                        let column = terrain_column(
                            coord.0 * 16 + i64::from(x),
                            coord.1 * 16 + i64::from(z),
                        );
                        let ground = i32::from(column.height);
                        if chunk.block([x, ground, z]) != u32::from(CACTUS) {
                            continue;
                        }
                        assert_eq!(column.biome, Biome::Desert);
                        assert!(column.height > SEA_LEVEL + 1);
                        assert_eq!(chunk.block([x, ground - 1, z]), u32::from(SAND));
                        let height = (ground..HEIGHT as i32)
                            .take_while(|&y| chunk.block([x, y, z]) == u32::from(CACTUS))
                            .count();
                        assert!((1..=3).contains(&height));
                        heights[height - 1] = true;
                    }
                }
                if heights.into_iter().all(|seen| seen) {
                    return;
                }
            }
        }
        panic!("Missing cactus heights: {heights:?}");
    }

    #[test]
    fn liquids_merge_internal_faces_and_keep_the_seabed() {
        for liquid in [WATER, LAVA] {
            let mut chunk = ChunkData::from_columns(
                [Column {
                    height: 12,
                    biome: Biome::Sea,
                    ..Column::default()
                }; PADDED_SIZE * PADDED_SIZE],
            );
            for block in &mut chunk.blocks {
                if *block == WATER {
                    *block = liquid;
                }
            }
            let mesh = chunk.mesh();
            assert_eq!(mesh.len(), 3 * QUAD_BYTES);
            let top = mesh
                .as_chunks::<QUAD_BYTES>()
                .0
                .iter()
                .find(|q| {
                    (u32::from_ne_bytes(q[..4].try_into().unwrap()) >> 20) & 63 == u32::from(liquid)
                })
                .unwrap();
            let packed = u32::from_ne_bytes(top[..4].try_into().unwrap());
            assert_eq!((packed >> 20) & 63, u32::from(liquid));
            assert_eq!(
                ((packed >> 27) & 15) * SECTION_SIZE as u32 + ((packed >> 5) & 127),
                u32::from(SEA_LEVEL)
            );
            assert!(!face_visible(u32::from(liquid), u32::from(liquid)));
            assert!(face_visible(u32::from(STONE), u32::from(liquid)));
        }
        assert!(face_visible(u32::from(LAVA), u32::from(WATER)));
        assert!(!face_visible(u32::from(WATER), u32::from(LAVA)));
        assert!(!face_visible(u32::from(LAVA), u32::from(STONE)));
    }

    #[test]
    fn translucent_blocks_hide_matching_faces_and_expose_solid_neighbors() {
        for material in [WATER, GLASS, GLASS_FRAME] {
            let material = u32::from(material);
            assert!(is_translucent(material));
            assert!(!face_visible(material, material));
            assert!(face_visible(u32::from(STONE), material));
            assert!(!face_visible(material, u32::from(STONE)));
        }
        assert!(face_visible(u32::from(GLASS_FRAME), u32::from(GLASS)));
        assert!(!face_visible(u32::from(GLASS), u32::from(GLASS_FRAME)));
    }

    #[test]
    fn lava_keeps_all_six_surrounding_faces_after_reloading() {
        let mut chunk = ChunkData::from_columns(
            [Column {
                height: 12,
                biome: Biome::Sea,
                ..Column::default()
            }; PADDED_SIZE * PADDED_SIZE],
        );
        let original = chunk.mesh().len();
        chunk.apply_edits(
            (0, 0),
            &[Edit {
                position: [8, 5, 8],
                material: LAVA,
            }],
        );
        let chunk = ChunkData::decode(&chunk.encode()).unwrap();
        let mesh = chunk.mesh();
        assert_eq!(mesh.len(), original + 6 * QUAD_BYTES);
        let mut walls = [false; 6];
        for quad in mesh.as_chunks::<QUAD_BYTES>().0 {
            let packed = u32::from_ne_bytes(quad[..4].try_into().unwrap());
            if (packed >> 20) & 63 == u32::from(STONE) {
                walls[((packed >> 17) & 7) as usize] = true;
            }
        }
        assert!(walls.into_iter().all(|visible| visible));
    }

    #[test]
    fn trees_follow_biome_density_and_avoid_water_and_desert() {
        let mut forest = [0; 2];
        let mut plains = [0; 2];
        for z in -100..100 {
            for x in -100..100 {
                let random = hash(x, 0, z, 7183);
                let wx = x * 10 + 2 + ((random >> 8) % 6) as i64;
                let wz = z * 10 + 2 + ((random >> 12) % 6) as i64;
                let column = terrain_column(wx, wz);
                let tree = tree_at(x, z);
                match column.biome {
                    Biome::Forest => {
                        forest[0] += 1;
                        forest[1] += usize::from(tree.is_some());
                    }
                    Biome::Plains => {
                        plains[0] += 1;
                        plains[1] += usize::from(tree.is_some());
                    }
                    Biome::Sea | Biome::River | Biome::Desert | Biome::Beach | Biome::Tundra => {
                        assert!(tree.is_none())
                    }
                    _ => {}
                }
            }
        }
        assert!(forest[0] > 100 && plains[0] > 100);
        assert!(forest[1] * plains[0] > plains[1] * forest[0] * 4);
    }

    #[test]
    fn bedrock_is_continuous_and_section_boundaries_do_not_add_faces() {
        for coord in [(0, 0), (-12, 3), (42, -27)] {
            let chunk = ChunkData::generate(coord);
            for z in 0..CHUNK_SIZE as i32 {
                for x in 0..CHUNK_SIZE as i32 {
                    for y in 0..3 {
                        assert_eq!(chunk.block([x, y, z]), u32::from(BEDROCK));
                    }
                    for y in 3..i32::from(
                        chunk.heights[(z + 1) as usize * PADDED_SIZE + (x + 1) as usize],
                    ) {
                        assert_ne!(chunk.block([x, y, z]), 0);
                    }
                }
            }
        }
        let full = ChunkData::from_heights([HEIGHT as u8; PADDED_SIZE * PADDED_SIZE]);
        let mesh = full.mesh();
        assert_eq!(mesh.len(), 2 * QUAD_BYTES);
        let tops: Vec<_> = mesh
            .as_chunks::<8>()
            .0
            .iter()
            .map(|q| {
                let a = u32::from_ne_bytes(q[..4].try_into().unwrap());
                ((a >> 27) & 15) * SECTION_SIZE as u32 + ((a >> 5) & 127)
            })
            .collect();
        assert_eq!(tops, [0, HEIGHT as u32]);
    }

    #[test]
    fn generated_lava_is_an_emissive_light_source() {
        for cz in -16..16 {
            for cx in -16..16 {
                let random = hash(cx, 0, cz, 3167);
                if !random.is_multiple_of(12) {
                    continue;
                }
                let x = cx * 32 + 8 + ((random >> 8) % 16) as i64;
                let z = cz * 32 + 8 + ((random >> 16) % 16) as i64;
                let column = terrain_column(x, z);
                if column.biome != Biome::Mountains || column.height <= SEA_LEVEL + 4 {
                    continue;
                }
                let chunk = ChunkData::generate((x.div_euclid(16), z.div_euclid(16)));
                let [px, py, pz] = [
                    x.rem_euclid(16) as i32,
                    i32::from(column.height) - 1,
                    z.rem_euclid(16) as i32,
                ];
                if chunk.block([px, py, pz]) == u32::from(LAVA) {
                    assert_eq!(chunk.light_at([px, py, pz]) >> 4, 15);
                    assert_eq!(chunk.light_at([px, py + 1, pz]) >> 4, 14);
                    return;
                }
            }
        }
        panic!("No generated lava source found");
    }

    #[test]
    fn packed_quads_preserve_boundary_values() {
        for position in [[0, 0, 0], [16, 64, 16]] {
            for face in 0..8 {
                for extent in [[1, 1], [16, 16]] {
                    for material in 1..=BEDROCK.into() {
                        let [a, b] = pack_quad(position, face, extent, material);
                        assert_eq!(
                            [a & 31, (a >> 5) & 127, (a >> 12) & 31],
                            position.map(|v| v as u32)
                        );
                        assert_eq!((a >> 17) & 7, face);
                        assert_eq!((a >> 20) & 63, material);
                        assert_eq!([b & 31, (b >> 5) & 31], extent.map(|v| v as u32));
                        assert_eq!(b >> 10, 0);
                        let lit = b | (0xab << 10) | (16383 << 18);
                        assert_eq!(lit & 31, extent[0] as u32);
                        assert_eq!((lit >> 5) & 31, extent[1] as u32);
                        assert_eq!((lit >> 10) & 255, 0xab);
                        assert_eq!(lit >> 18, 16383);
                    }
                }
            }
        }
    }

    #[test]
    fn terrain_is_repeatable_in_positive_and_negative_chunks() {
        for coord in [(0, 0), (-1, -1), (20_000, -30_000)] {
            let a = ChunkData::generate(coord);
            let b = ChunkData::generate(coord);
            assert_eq!(a.heights, b.heights);
            assert_eq!(a.blocks, b.blocks);
            assert_eq!(a.plants, b.plants);
            assert!(a.heights.iter().all(|&h| h > 0 && usize::from(h) < HEIGHT));
        }
    }

    #[test]
    fn neighbor_halos_agree_across_zero() {
        let left = ChunkData::generate((-1, 0));
        let right = ChunkData::generate((0, 0));
        for z in 0..PADDED_SIZE {
            assert_eq!(
                left.heights[z * PADDED_SIZE + CHUNK_SIZE + 1],
                right.heights[z * PADDED_SIZE + 1]
            );
            assert_eq!(
                left.heights[z * PADDED_SIZE + CHUNK_SIZE],
                right.heights[z * PADDED_SIZE]
            );
        }
    }

    #[test]
    fn decorated_halos_match_in_both_axes() {
        for coord in [(-1, 0), (0, -1), (3, -4), (-5, -5)] {
            let a = ChunkData::generate(coord);
            let right = ChunkData::generate((coord.0 + 1, coord.1));
            let front = ChunkData::generate((coord.0, coord.1 + 1));
            for y in 0..HEIGHT as i32 {
                for t in -1..=CHUNK_SIZE as i32 {
                    assert_eq!(a.block([16, y, t]), right.block([0, y, t]));
                    assert_eq!(a.light_at([16, y, t]), right.light_at([0, y, t]));
                    assert_eq!(a.block([15, y, t]), right.block([-1, y, t]));
                    assert_eq!(a.block([t, y, 16]), front.block([t, y, 0]));
                    assert_eq!(a.light_at([t, y, 16]), front.light_at([t, y, 0]));
                    assert_eq!(a.block([t, y, 15]), front.block([t, y, -1]));
                }
            }
        }
    }

    #[test]
    fn tree_styles_have_distinct_trunks_canopies_and_heights() {
        let mut tops = Vec::new();
        for (style, height, wood, leaves) in [
            (TreeStyle::Oak, 6, OAK, LEAVES),
            (TreeStyle::Birch, 8, BIRCH, LEAVES),
            (TreeStyle::Pine, 9, PINE, PINE_LEAVES),
            (TreeStyle::Autumn, 5, OAK, AUTUMN_LEAVES),
        ] {
            let mut chunk = ChunkData::from_heights([60; PADDED_SIZE * PADDED_SIZE]);
            chunk.add_tree(
                &Tree {
                    x: 8,
                    z: 8,
                    ground: 60,
                    height,
                    style,
                },
                (0, 0),
            );
            assert_eq!(chunk.block([8, 60, 8]), u32::from(wood));
            assert!(chunk.blocks.iter().filter(|&&v| v == leaves).count() > 20);
            assert!(chunk.max_height <= HEIGHT);
            tops.push(chunk.max_height);
        }
        assert_eq!(tops, vec![69, 70, 71, 68]);
    }

    #[test]
    fn generation_contains_grass_and_all_ore_types() {
        let mut ore_types = [false; ORES.len()];
        let mut plants = [false; 4];
        let mut exposed_ores = 0;
        for z in -3..=3 {
            for x in -3..=3 {
                // Sample multiple broad biome regions, not only the spawn coastline.
                let chunk = ChunkData::generate((x * 13, z * 13));
                for &material in &chunk.blocks {
                    if let Some(index) = ORES.iter().position(|&ore| ore == material) {
                        ore_types[index] = true;
                    }
                }
                for &[px, py, pz, material] in &chunk.plants {
                    plants[(material - FIRST_PLANT) as usize] = true;
                    assert_eq!(
                        chunk.block([i32::from(px), i32::from(py) - 1, i32::from(pz)]),
                        u32::from(GRASS)
                    );
                    assert_eq!(
                        chunk.block([i32::from(px), i32::from(py), i32::from(pz)]),
                        0
                    );
                }
                for pz in 0..CHUNK_SIZE {
                    for px in 0..CHUNK_SIZE {
                        let y = i32::from(chunk.heights[(pz + 1) * PADDED_SIZE + px + 1]) - 1;
                        if ORES.contains(&(chunk.block([px as i32, y, pz as i32]) as u8)) {
                            exposed_ores += 1;
                        }
                    }
                }
            }
        }
        assert!(ore_types.into_iter().all(|v| v));
        assert!(plants.into_iter().all(|v| v));
        assert!(exposed_ores > 0);
    }

    #[test]
    fn grass_has_two_planes_with_both_windings() {
        let mut chunk = ChunkData::from_heights([60; PADDED_SIZE * PADDED_SIZE]);
        chunk.plants.push([8, 60, 8, FIRST_PLANT]);
        let mesh = chunk.mesh();
        let plants: Vec<_> = mesh
            .as_chunks::<8>()
            .0
            .iter()
            .filter_map(|q| {
                let a = u32::from_ne_bytes(q[..4].try_into().unwrap());
                let face = (a >> 17) & 7;
                (face >= 6).then_some((face, (a >> 26) & 1, (a >> 20) & 63))
            })
            .collect();
        assert_eq!(plants, vec![(6, 0, 12), (6, 1, 12), (7, 0, 12), (7, 1, 12)]);
    }

    #[test]
    fn solid_leaves_hide_adjacent_faces() {
        assert!(!face_visible(u32::from(OAK), u32::from(LEAVES)));
        assert!(!face_visible(u32::from(LEAVES), u32::from(OAK)));
        assert!(!face_visible(u32::from(LEAVES), u32::from(LEAVES)));
        assert!(face_visible(u32::from(LEAVES), 0));
    }

    #[test]
    fn flat_chunk_has_only_merged_top_and_bottom() {
        let chunk = ChunkData::from_heights([60; PADDED_SIZE * PADDED_SIZE]);
        assert_eq!(chunk.mesh().len(), 2 * QUAD_BYTES);
    }

    #[test]
    fn merged_area_matches_exposed_faces() {
        for coord in [(0, 0), (-1, -1), (17, -42)] {
            let chunk = ChunkData::generate(coord);
            let mesh = chunk.mesh();
            let mut actual = 0u64;
            for quad in mesh.as_chunks::<QUAD_BYTES>().0 {
                let [a, b] = [
                    u32::from_ne_bytes(quad[..4].try_into().unwrap()),
                    u32::from_ne_bytes(quad[4..].try_into().unwrap()),
                ];
                if (a >> 17) & 7 < 6 {
                    actual += u64::from((b & 31) * ((b >> 5) & 31));
                }
                assert!(a & 31 <= 16 && (a >> 5) & 127 <= 64 && (a >> 12) & 31 <= 16);
            }
            let mut expected = 0;
            for z in 0..CHUNK_SIZE as i32 {
                for x in 0..CHUNK_SIZE as i32 {
                    for y in 0..HEIGHT as i32 {
                        if chunk.block([x, y, z]) == 0 {
                            continue;
                        }
                        for [dx, dy, dz] in [
                            [1, 0, 0],
                            [-1, 0, 0],
                            [0, 1, 0],
                            [0, -1, 0],
                            [0, 0, 1],
                            [0, 0, -1],
                        ] {
                            expected += u64::from(face_visible(
                                chunk.block([x, y, z]),
                                chunk.block([x + dx, y + dy, z + dz]),
                            ));
                        }
                    }
                }
            }
            assert_eq!(actual, expected);
        }
    }
}
