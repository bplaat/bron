/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};

use gtxconv::{Format, Texture};

#[path = "../../tools/wiiu-toolchain/build.rs"]
mod wiiu;

// Converts an image to a GX2 texture, opaque images compress to BC1, the others keep their alpha.
// The sky layers and character sprites have different sizes, including a 513-pixel edge, so
// they're scaled to one square like the desktop texture arrays.
fn convert(out_dir: &Path, path: &Path, mipmaps: bool, square: bool) -> (Texture, bool) {
    println!("cargo:rerun-if-changed={}", path.display());
    let bytes = fs::read(path).expect("failed to read texture");
    let image = image::decode(&bytes).expect("failed to decode texture");
    let opaque = image
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| p[3] == 255);
    let format = if opaque { Format::Bc1 } else { Format::Rgba8 };
    let mut texture_path = path.to_path_buf();
    if square {
        let mut pixels = Vec::with_capacity(256 * 256 * 4);
        for y in 0..256 {
            for x in 0..256 {
                let i = (((y * image.height() / 256) * image.width() + x * image.width() / 256) * 4)
                    as usize;
                pixels.extend_from_slice(&image.pixels()[i..i + 4]);
            }
        }
        let bitmap = image::Bitmap::new(256, 256, pixels).expect("valid bitmap");
        texture_path = out_dir.join(path.file_name().expect("texture has a name"));
        fs::write(
            &texture_path,
            image::encode(&bitmap, image::Format::Png).expect("failed to encode texture"),
        )
        .expect("failed to write texture");
    }
    let texture = Texture::load(&texture_path, format, mipmaps).expect("failed to convert texture");
    (texture, !opaque)
}

fn main() {
    let manifest_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let build_dir = wiiu::build(manifest_dir.join("../../tools/wiiu-toolchain"));
    wiiu::assemble_shader(
        &build_dir,
        &manifest_dir.join("src/shader.vsh"),
        &manifest_dir.join("src/shader.psh"),
    );

    // The block tiles are shared with the bwindow game, all textures go in one .gtx file
    let assets = manifest_dir.join("../plaatcraft/assets");
    let mut textures = Vec::new();
    let mut alpha = Vec::new();
    for tile in plaatcraft_engine::tiles::TILES {
        let (texture, has_alpha) = convert(&out_dir, &assets.join(tile), true, false);
        textures.push(texture);
        alpha.push(has_alpha);
    }
    let sky = [
        "skybox_bottom.png",
        "skybox_side.png",
        "skybox_sideClouds.png",
        "skybox_sideHills.png",
        "skybox_top.png",
        "clouds.png",
        "sun.png",
        "moon.png",
    ]
    .map(|file| assets.join("sky").join(file));
    let characters = plaatcraft_engine::avatars::PARTS
        .map(|part| assets.join("characters").join(format!("male_{part}.png")));
    for path in sky.iter().chain(&characters) {
        textures.push(convert(&out_dir, path, false, true).0);
    }
    // The font and cursor of the engine's client, and a white texture for the untextured draws
    let client_assets = manifest_dir.join("../plaatcraft-engine/assets");
    let white = out_dir.join("white.png");
    let bitmap = image::Bitmap::new(4, 4, vec![255; 4 * 4 * 4]).expect("valid bitmap");
    fs::write(
        &white,
        image::encode(&bitmap, image::Format::Png).expect("failed to encode texture"),
    )
    .expect("failed to write texture");
    for path in [
        client_assets.join("font.png"),
        client_assets.join("cursor.png"),
        white,
    ] {
        textures.push(convert(&out_dir, &path, false, false).0);
    }
    fs::write(out_dir.join("textures.gtx"), gtxconv::write_gtx(&textures))
        .expect("failed to write textures.gtx");

    let tiles = alpha.len();
    let mut code = String::new();
    writeln!(code, "pub const TILES: usize = 0;").unwrap();
    writeln!(code, "pub const SKY: usize = {tiles};").unwrap();
    writeln!(code, "pub const CHARACTERS: usize = {};", tiles + sky.len()).unwrap();
    let rest = tiles + sky.len() + characters.len();
    writeln!(code, "pub const FONT: usize = {rest};").unwrap();
    writeln!(code, "pub const CURSOR: usize = {};", rest + 1).unwrap();
    writeln!(code, "pub const WHITE: usize = {};", rest + 2).unwrap();
    writeln!(code, "pub const COUNT: usize = {};", textures.len()).unwrap();
    writeln!(code, "pub const HAS_ALPHA: [bool; {tiles}] = {alpha:?};").unwrap();
    fs::write(out_dir.join("textures.rs"), code).expect("failed to write textures.rs");
}
