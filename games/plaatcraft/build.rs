/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("README.md")]

use std::fs;
use std::path::{Path, PathBuf};

#[allow(unreachable_pub, reason = "shared with the crate")]
#[path = "src/assets.rs"]
mod assets;
#[allow(unreachable_pub, reason = "shared with the Wii U programs")]
#[path = "../../tools/wiiu-toolchain/build.rs"]
mod wiiu;

fn main() {
    match std::env::var("CARGO_CFG_TARGET_OS")
        .expect("CARGO_CFG_TARGET_OS not set")
        .as_str()
    {
        "wii" => wii_textures(),
        "wiiu" => {
            let build_dir = Path::new("../../target/wiiu-toolchain");
            wiiu::assemble_shader(
                build_dir,
                Path::new("src/wiiu/shader.vsh"),
                Path::new("src/wiiu/shader.psh"),
            );
            wiiu_textures();
        }
        os => desktop(os),
    }
}

fn desktop(os: &str) {
    println!("cargo:rerun-if-env-changed=CARGO_BUNDLE");

    println!("cargo:rerun-if-changed=assets");

    bwebgpu_shader_build::Builder::new()
        .shader("terrain.wgsl", "src/desktop/shaders/terrain.wgsl")
        .shader("liquid.wgsl", "src/desktop/shaders/liquid.wgsl")
        .shader("sky.wgsl", "src/desktop/shaders/sky.wgsl")
        .shader("crosshair.wgsl", "src/desktop/shaders/crosshair.wgsl")
        .shader("selection.wgsl", "src/desktop/shaders/selection.wgsl")
        .shader("preview.wgsl", "src/desktop/shaders/preview.wgsl")
        .shader("player.wgsl", "src/desktop/shaders/player.wgsl")
        .shader("held.wgsl", "src/desktop/shaders/held.wgsl")
        .compile()
        .expect("failed to compile shaders");

    if os == "macos" && std::env::var_os("CARGO_BUNDLE").is_none() {
        build_utils::macos::embed_info_plist("meta/apple/macos/Info.plist");
    }

    if os == "windows" {
        build_utils::windows::compile_application(
            "PlaatCraft",
            "PlaatCraft voxel sandbox",
            "Copyright (c) 2026 Bastiaan van der Plaat",
            "meta/windows/icon.ico",
            &[],
        )
        .expect("failed to compile Windows resources");
    }
}

fn out_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR not set"))
}

/// The textures of the consoles: the tiles, the sky layers, the character parts and the
/// client's font, cursors and mouse, the second value says whether a texture is a square.
fn console_textures() -> Vec<(PathBuf, bool)> {
    let assets = Path::new("assets");
    let client = Path::new("src/client/assets");
    let characters = assets::PARTS.map(|part| format!("characters/male_{part}.png"));
    assets::TILES
        .iter()
        .map(|tile| (assets.join(tile), false))
        .chain(assets::SKY.iter().map(|sky| (assets.join(sky), true)))
        .chain(characters.iter().map(|part| (assets.join(part), true)))
        .chain(
            [
                "font.png",
                "cursor1.png",
                "cursor2.png",
                "cursor3.png",
                "cursor4.png",
                "mouse.png",
            ]
            .map(|file| (client.join(file), false)),
        )
        .collect()
}

/// Decodes a texture and tells whether it's opaque. The sky layers and character sprites have
/// different sizes, including a 513-pixel edge, so with `square` they're scaled to one 256x256
/// square like the desktop texture arrays, written to `OUT_DIR`.
fn load(path: &Path, square: bool) -> (PathBuf, bool) {
    println!("cargo:rerun-if-changed={}", path.display());
    let bytes = fs::read(path).expect("failed to read texture");
    let image = image::decode(&bytes).expect("failed to decode texture");
    let opaque = image
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| p[3] == 255);
    if !square {
        return (path.to_path_buf(), opaque);
    }
    let mut pixels = Vec::with_capacity(256 * 256 * 4);
    for y in 0..256 {
        for x in 0..256 {
            let i = (((y * image.height() / 256) * image.width() + x * image.width() / 256) * 4)
                as usize;
            pixels.extend_from_slice(&image.pixels()[i..i + 4]);
        }
    }
    let bitmap = image::Bitmap::new(256, 256, pixels).expect("valid bitmap");
    let squared = out_dir().join(path.file_name().expect("texture has a name"));
    fs::write(
        &squared,
        image::encode(&bitmap, image::Format::Png).expect("failed to encode texture"),
    )
    .expect("failed to write texture");
    (squared, opaque)
}

/// Converts every texture to GX in one blob and describes them in textures.rs, opaque images
/// compress to CMPR, cutouts and translucent images need alpha.
fn wii_textures() {
    use gxtexconv::{Format, Texture};

    let mut data = Vec::new();
    let mut textures = Vec::new();
    for (index, (path, square)) in console_textures().into_iter().enumerate() {
        let (path, opaque) = load(&path, square);
        let format = if opaque { Format::Cmpr } else { Format::Rgb5a3 };
        let texture = if index < assets::TILES.len() {
            Texture::load_mipmapped(&path, format)
        } else {
            Texture::load(&path, format)
        }
        .expect("failed to convert texture");
        textures.push(format!(
            "Texture {{ offset: {}, width: {}, height: {}, format: {}, levels: {} }}",
            data.len(),
            texture.width,
            texture.height,
            texture.format as u8,
            texture.levels
        ));
        data.extend(&texture.data);
        data.resize(data.len().next_multiple_of(32), 0);
    }

    let mut code = String::new();
    let mut textures = textures.into_iter();
    for (name, count) in [
        ("TILES", assets::TILES.len()),
        ("SKY", assets::SKY.len()),
        ("CHARACTERS", assets::PARTS.len()),
    ] {
        code.push_str(&format!(
            "pub(crate) const {name}: [Texture; {count}] = [\n"
        ));
        for texture in textures.by_ref().take(count) {
            code.push_str(&format!("    {texture},\n"));
        }
        code.push_str("];\n");
    }
    let [font, cursor1, cursor2, cursor3, cursor4, mouse] =
        [(); 6].map(|()| textures.next().expect("client texture"));
    code.push_str(&format!("pub(crate) const FONT: Texture = {font};\n"));
    code.push_str(&format!("pub(crate) const MOUSE: Texture = {mouse};\n"));
    code.push_str(&format!(
        "pub(crate) const CURSORS: [Texture; 4] = [{cursor1}, {cursor2}, {cursor3}, {cursor4}];\n"
    ));
    fs::write(out_dir().join("textures.bin"), data).expect("failed to write textures.bin");
    fs::write(out_dir().join("textures.rs"), code).expect("failed to write textures.rs");
}

/// Converts every texture to GX2 in one .gtx file, opaque images compress to BC1, the others
/// keep their alpha. A white texture at the end is for the untextured draws.
fn wiiu_textures() {
    use gtxconv::{Format, Texture};

    let white = out_dir().join("white.png");
    let bitmap = image::Bitmap::new(4, 4, vec![255; 4 * 4 * 4]).expect("valid bitmap");
    fs::write(
        &white,
        image::encode(&bitmap, image::Format::Png).expect("failed to encode texture"),
    )
    .expect("failed to write texture");

    let mut textures = Vec::new();
    let mut alpha = Vec::new();
    for (index, (path, square)) in console_textures()
        .into_iter()
        .chain([(white, false)])
        .enumerate()
    {
        let (path, opaque) = load(&path, square);
        let format = if opaque { Format::Bc1 } else { Format::Rgba8 };
        let mipmaps = index < assets::TILES.len();
        textures.push(Texture::load(&path, format, mipmaps).expect("failed to convert texture"));
        if mipmaps {
            alpha.push(!opaque);
        }
    }
    fs::write(
        out_dir().join("textures.gtx"),
        gtxconv::write_gtx(&textures),
    )
    .expect("failed to write textures.gtx");

    let sky = assets::TILES.len();
    let characters = sky + assets::SKY.len();
    let font = characters + assets::PARTS.len();
    let code = format!(
        "pub(crate) const TILES: usize = 0;
pub(crate) const SKY: usize = {sky};
pub(crate) const CHARACTERS: usize = {characters};
pub(crate) const FONT: usize = {font};
pub(crate) const CURSORS: usize = {};
pub(crate) const MOUSE: usize = {};
pub(crate) const WHITE: usize = {};
pub(crate) const COUNT: usize = {};
pub(crate) const HAS_ALPHA: [bool; {sky}] = {alpha:?};
",
        font + 1,
        font + 5,
        font + 6,
        textures.len()
    );
    fs::write(out_dir().join("textures.rs"), code).expect("failed to write textures.rs");
}
