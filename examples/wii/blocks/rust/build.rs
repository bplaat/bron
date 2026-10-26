/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::path::PathBuf;
use std::{env, fs};

use gxtexconv::Format;

#[path = "../../../../tools/wii-toolchain/build.rs"]
mod wii;

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    wii::build(manifest_dir.join("../../../../tools/wii-toolchain"));

    // Convert the block images to mipmapped CMPR textures, materials.rs loads them in script order
    let script = manifest_dir.join("src/assets/materials.scf");
    println!("cargo:rerun-if-changed={}", script.display());
    let mut images = Vec::new();
    for entry in gxtexconv::read_script(&script).expect("failed to read materials.scf") {
        println!("cargo:rerun-if-changed={}", entry.path.display());
        let texture = entry.load().expect("failed to convert material");
        assert!(
            texture.format == Format::Cmpr
                && texture.width == 128
                && texture.height == 128
                && texture.levels == 5,
            "{} must be a 128x128 CMPR texture with mipmaps",
            entry.path.display()
        );
        images.extend(texture.data);
    }
    fs::write(out_dir.join("materials.bin"), images).expect("failed to write materials.bin");
}
