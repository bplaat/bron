/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Compiles the shaders.

fn main() {
    bwebgpu_shader_build::Builder::new()
        .shader("blocks.wgsl", "src/blocks.wgsl")
        .shader("sky.wgsl", "src/sky.wgsl")
        .compile()
        .expect("failed to compile shaders");
}
