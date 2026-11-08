// Copyright (c) 2026 Bastiaan van der Plaat
// SPDX-License-Identifier: MIT

//! Compiles the native game shader.

fn main() -> bwebgpu_shader_build::Result<()> {
    if std::env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set") == "windows" {
        println!("cargo:rerun-if-changed=meta/windows");

        build_utils::windows::compile_application(
            "Frogger",
            "Frogger",
            "Copyright (c) 2026 Bastiaan van der Plaat",
            "meta/windows/icon.ico",
            &[],
        )
        .expect("failed to compile Windows resources");
    }
    bwebgpu_shader_build::Builder::new()
        .shader("scene.wgsl", "src/scene.wgsl")
        .compile()
}
