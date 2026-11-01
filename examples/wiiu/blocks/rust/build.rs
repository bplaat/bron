/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::path::Path;

#[path = "../../../../tools/wiiu-toolchain/build.rs"]
mod wiiu;

fn main() {
    let build_dir = wiiu::build("../../../../tools/wiiu-toolchain");
    wiiu::assemble_shader(
        &build_dir,
        Path::new("src/shader.vsh"),
        Path::new("src/shader.psh"),
    );
    wiiu::convert_textures(&build_dir, Path::new("src/assets/textures.toml"));
}
