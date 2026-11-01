/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::path::{Path, PathBuf};
use std::{env, fs};

#[path = "../../../../tools/wii-toolchain/build.rs"]
mod wii;

fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    println!("cargo:rerun-if-changed=src/assets/crate.jpg");
    let texture =
        gxtexconv::Texture::load(Path::new("src/assets/crate.jpg"), gxtexconv::Format::Rgba8)
            .expect("convert crate texture");
    fs::write(out.join("cube.bin"), texture.data).expect("write converted texture");
    fs::write(
        out.join("texture.rs"),
        format!(
            "const TEXTURE_WIDTH: u16 = {};\nconst TEXTURE_HEIGHT: u16 = {};\n",
            texture.width, texture.height
        ),
    )
    .expect("write texture dimensions");
    wii::build("../../../../tools/wii-toolchain");
}
