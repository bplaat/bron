/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The block textures and which one each face of a material shows.

use crate::world;

/// The block textures in the order of the texture array layers.
pub const TILES: [&str; 73] = [
    "tiles/grass_top.png",
    "tiles/dirt_grass.png",
    "tiles/dirt.png",
    "tiles/stone.png",
    "tiles/sand.png",
    "tiles/snow.png",
    "tiles/dirt_snow.png",
    "tiles/trunk_side.png",
    "tiles/trunk_top.png",
    "tiles/trunk_white_side.png",
    "tiles/trunk_white_top.png",
    "tiles/leaves.png",
    "tiles/leaves_orange.png",
    "tiles/grass1.png",
    "tiles/grass2.png",
    "tiles/grass3.png",
    "tiles/grass4.png",
    "tiles/stone_coal.png",
    "tiles/stone_gold.png",
    "tiles/stone_diamond.png",
    "tiles/stone_silver.png",
    "tiles/water.png",
    "tiles/lava.png",
    "tiles/cactus_side.png",
    "tiles/cactus_top.png",
    "tiles/cactus_inside.png",
    "tiles/brick_grey.png",
    "tiles/brick_red.png",
    "tiles/wood.png",
    "tiles/wood_red.png",
    "tiles/cotton_blue.png",
    "tiles/cotton_green.png",
    "tiles/cotton_red.png",
    "tiles/cotton_white.png",
    "tiles/gravel_dirt.png",
    "tiles/gravel_stone.png",
    "tiles/oven.png",
    "tiles/redstone.png",
    "tiles/greystone.png",
    "tiles/cotton_tan.png",
    "tiles/dirt_sand.png",
    "tiles/greysand.png",
    "tiles/greystone_ruby.png",
    "tiles/greystone_ruby_alt.png",
    "tiles/greystone_sand.png",
    "tiles/ice.png",
    "tiles/redsand.png",
    "tiles/redstone_emerald.png",
    "tiles/redstone_emerald_alt.png",
    "tiles/redstone_sand.png",
    "tiles/stone_iron.png",
    "tiles/stone_iron_alt.png",
    "tiles/stone_coal_alt.png",
    "tiles/stone_diamond_alt.png",
    "tiles/stone_dirt.png",
    "tiles/stone_gold_alt.png",
    "tiles/stone_grass.png",
    "tiles/stone_sand.png",
    "tiles/stone_silver_alt.png",
    "tiles/stone_snow.png",
    "tiles/table.png",
    "tiles/glass.png",
    "tiles/glass_frame.png",
    "tiles/trunk_red_side.png",
    "tiles/trunk_red_top.png",
    "tiles/wood_white.png",
    "tiles/grass_brown.png",
    "tiles/grass_tan.png",
    "tiles/mushroom_brown.png",
    "tiles/mushroom_red.png",
    "tiles/mushroom_tan.png",
    "tiles/rock.png",
    "tiles/rock_moss.png",
];

/// The [`TILES`] index of a tile name.
const fn tile_index(name: &str) -> u8 {
    let name = name.as_bytes();
    let mut index = 0;
    'tiles: while index < TILES.len() {
        // Every tile path is `tiles/{name}.png`
        let path = TILES[index].as_bytes();
        if path.len() == name.len() + 10 {
            let mut i = 0;
            while i < name.len() {
                if path[6 + i] != name[i] {
                    index += 1;
                    continue 'tiles;
                }
                i += 1;
            }
            return index as u8;
        }
        index += 1;
    }
    panic!("Unknown tile");
}

/// The [`TILES`] index of every face of every material, faces are numbered like in
/// [`crate::world::ChunkData::mesh`]: axis times two plus one for the negative side, the
/// two diagonal planes 6 and 7 of sprites use the +z face tile.
pub const FACE_TILES: [[u8; 8]; 128] = {
    let mut table = [[0; 8]; 128];
    let mut material = 0;
    while material < table.len() {
        if let Some(look) = world::look(material as u8) {
            let mut face = 0;
            while face < 8 {
                table[material][face] = tile_index(look.faces[if face < 6 { face } else { 4 }]);
                face += 1;
            }
        }
        material += 1;
    }
    table
};

/// The [`TILES`] index a face of a material shows, see [`FACE_TILES`].
pub const fn tile(material: u8, face: u32) -> usize {
    FACE_TILES[material as usize % 128][face as usize] as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{LAVA, MATERIALS, WATER};

    #[test]
    fn every_material_face_has_a_matching_tile() {
        for path in TILES {
            assert!(path.starts_with("tiles/") && path.ends_with(".png"));
        }
        for &(material, _) in MATERIALS {
            assert!(world::look(material).is_some());
        }
        let name = |material, face| TILES[tile(material, face)];
        assert_eq!(name(1, 2), "tiles/grass_top.png");
        assert_eq!(name(1, 0), "tiles/dirt_grass.png");
        assert_eq!(name(1, 3), "tiles/dirt.png");
        assert_eq!(name(8, 2), "tiles/trunk_red_top.png");
        assert_eq!(name(9, 0), "tiles/leaves.png");
        assert_eq!(name(11, 0), "tiles/leaves.png");
        assert_eq!(name(12, 6), "tiles/grass1.png");
        assert_eq!(name(21, 0), "tiles/wood_white.png");
        assert_eq!(name(WATER, 2), "tiles/water.png");
        assert_eq!(name(23, 0), "tiles/stone.png");
        assert_eq!(name(LAVA, 2), "tiles/lava.png");
        assert_eq!(name(25, 3), "tiles/cactus_inside.png");
        assert_eq!(name(36, 4), "tiles/oven.png");
        assert_eq!(name(36, 0), "tiles/stone.png");
        assert_eq!(name(62, 0), "tiles/glass.png");
        assert_eq!(name(72, 6), "tiles/rock_moss.png");
    }
}
