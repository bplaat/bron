/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Compiles the shaders.

fn main() -> bwebgpu_shader_build::Result<()> {
    bwebgpu_shader_build::Builder::new()
        .shader("blocks.wgsl", "src/blocks.wgsl")
        .shader("sky.wgsl", "src/sky.wgsl")
        .shader("overlay.wgsl", "src/overlay.wgsl")
        .compile()
}
