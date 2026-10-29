/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};

use gxtexconv::{Format, Texture};

#[path = "../../tools/wii-toolchain/build.rs"]
mod wii;

// Converts an image to a GX texture and appends it to the texture data, returns the Rust
// expression that describes it in textures.rs
fn convert(data: &mut Vec<u8>, path: &Path, mipmaps: bool) -> String {
    println!("cargo:rerun-if-changed={}", path.display());
    let bytes = fs::read(path).expect("failed to read texture");
    let image = image::decode(&bytes).expect("failed to decode texture");
    // Opaque images compress to CMPR, cutouts and translucent images need alpha
    let opaque = image
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| p[3] == 255);
    let format = if opaque { Format::Cmpr } else { Format::Rgb5a3 };
    // The sky layers and character sprites have different sizes, including a 513-pixel edge,
    // so they're scaled to one tile-aligned square like the desktop texture arrays.
    let normalized;
    let texture_path = if path
        .parent()
        .is_some_and(|p| p.ends_with("sky") || p.ends_with("characters"))
    {
        let mut pixels = Vec::with_capacity(256 * 256 * 4);
        for y in 0..256 {
            for x in 0..256 {
                let i = (((y * image.height() / 256) * image.width() + x * image.width() / 256) * 4)
                    as usize;
                pixels.extend_from_slice(&image.pixels()[i..i + 4]);
            }
        }
        let bitmap = image::Bitmap::new(256, 256, pixels).expect("valid bitmap");
        normalized =
            PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join(path.file_name().unwrap());
        fs::write(
            &normalized,
            image::encode(&bitmap, image::Format::Png).expect("failed to encode texture"),
        )
        .expect("failed to write texture");
        &normalized
    } else {
        path
    };
    let texture = if mipmaps {
        Texture::load_mipmapped(texture_path, format)
    } else {
        Texture::load(texture_path, format)
    }
    .expect("failed to convert texture");
    let offset = data.len();
    data.extend(&texture.data);
    data.resize(data.len().next_multiple_of(32), 0);
    format!(
        "Texture {{ offset: {offset}, width: {}, height: {}, format: {}, levels: {} }}",
        texture.width, texture.height, texture.format as u8, texture.levels
    )
}

fn main() {
    let manifest_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    // The C libraries are only built and linked for the Wii, not when checking for the host
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("powerpc") {
        wii::build(manifest_dir.join("../../tools/wii-toolchain"));
    }

    // The block tiles are shared with the bwindow game, they all go in one texture blob
    let assets = manifest_dir.join("../plaatcraft/assets");
    let mut data = Vec::new();
    let mut code =
        String::from("pub const TILES: [Texture; plaatcraft_engine::tiles::TILES.len()] = [\n");
    for tile in plaatcraft_engine::tiles::TILES {
        writeln!(
            code,
            "    {},",
            convert(&mut data, &assets.join(tile), true)
        )
        .unwrap();
    }
    code.push_str("];\n");
    code.push_str("pub const SKY: [Texture; 8] = [\n");
    for file in [
        "skybox_bottom.png",
        "skybox_side.png",
        "skybox_sideClouds.png",
        "skybox_sideHills.png",
        "skybox_top.png",
        "clouds.png",
        "sun.png",
        "moon.png",
    ] {
        writeln!(
            code,
            "    {},",
            convert(&mut data, &assets.join("sky").join(file), false)
        )
        .unwrap();
    }
    code.push_str("];\n");

    code.push_str("pub const CHARACTERS: [Texture; 4] = [\n");
    for part in plaatcraft_engine::avatars::PARTS {
        writeln!(
            code,
            "    {},",
            convert(
                &mut data,
                &assets.join("characters").join(format!("male_{part}.png")),
                false
            )
        )
        .unwrap();
    }
    code.push_str("];\n");
    for (name, file) in [("FONT", "font.png"), ("CURSOR", "cursor.png")] {
        let texture = convert(
            &mut data,
            &manifest_dir.join("src/assets").join(file),
            false,
        );
        writeln!(code, "pub const {name}: Texture = {texture};").unwrap();
    }
    fs::write(out_dir.join("textures.bin"), data).expect("failed to write textures.bin");
    fs::write(out_dir.join("textures.rs"), code).expect("failed to write textures.rs");
}
