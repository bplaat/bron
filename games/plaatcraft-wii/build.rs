/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};

use gxtexconv::{Format, Texture};
use plaatcraft_engine::sound::Effect;

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
    let texture = if mipmaps {
        Texture::load_mipmapped(path, format)
    } else {
        Texture::load(path, format)
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

// Renders the sound effects like baudio does on the desktop, as 16-bit big-endian mono samples
// that the DSP mixes
fn render_sounds(out_dir: &Path) {
    let mut data = Vec::new();
    let mut code = format!(
        "pub const SAMPLE_RATE: u32 = {};\npub const SOUNDS: [(usize, usize); {}] = [\n",
        sfxr::SAMPLE_RATE,
        Effect::ALL.len()
    );
    for effect in Effect::ALL {
        let mut generator = sfxr::Generator::new(effect.sample());
        generator.volume = 0.5;
        let offset = data.len();
        let mut chunk = [0.0; 1024];
        while !generator.finished() {
            generator.generate(&mut chunk);
            for sample in chunk {
                data.extend(((sample.clamp(-1.0, 1.0) * 32767.0) as i16).to_be_bytes());
            }
        }
        writeln!(code, "    ({offset}, {}),", data.len() - offset).unwrap();
    }
    code.push_str("];\n");
    fs::write(out_dir.join("sounds.bin"), data).expect("failed to write sounds.bin");
    fs::write(out_dir.join("sounds.rs"), code).expect("failed to write sounds.rs");
}

fn main() {
    let manifest_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    wii::build(manifest_dir.join("../../tools/wii-toolchain"));

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
    render_sounds(&out_dir);
}
