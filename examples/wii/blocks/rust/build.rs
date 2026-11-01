/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::path::PathBuf;
use std::{env, fs};

use gxtexconv::{Format, Texture};

#[path = "../../../../tools/wii-toolchain/build.rs"]
mod wii;

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    wii::build(manifest_dir.join("../../../../tools/wii-toolchain"));

    // Keep textures in Material order; materials.rs loads the concatenated image data.
    let assets = manifest_dir.join("src/assets");
    let mut images = Vec::new();
    for name in [
        "brick_red",
        "cactus_side",
        "cactus_top",
        "dirt",
        "dirt_grass",
        "grass_top",
        "greystone",
        "lava",
        "leaves",
        "sand",
        "stone",
        "stone_coal",
        "stone_diamond",
        "stone_gold",
        "stone_iron",
        "trunk_side",
        "trunk_top",
        "water",
        "wood",
    ] {
        let path = assets.join(format!("{name}.png"));
        println!("cargo:rerun-if-changed={}", path.display());
        let texture = Texture::load_mipmapped(&path, Format::Cmpr)
            .unwrap_or_else(|err| panic!("failed to convert {}: {err}", path.display()));
        assert!(
            texture.width == 128 && texture.height == 128 && texture.levels == 5,
            "{} must be a 128x128 texture with five mipmap levels",
            path.display()
        );
        images.extend(texture.data);
    }
    fs::write(out_dir.join("materials.bin"), images).expect("failed to write materials.bin");
}
