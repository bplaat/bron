// Copyright (c) 2026 Bastiaan van der Plaat
// SPDX-License-Identifier: MIT

//! Compiles the native game shader.

fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_BUNDLE");

    bwebgpu_shader_build::Builder::new()
        .shader("scene.wgsl", "src/scene.wgsl")
        .compile()
        .expect("failed to compile shaders");

    if std::env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set") == "macos"
        && std::env::var_os("CARGO_BUNDLE").is_none()
    {
        build_utils::macos::embed_info_plist("meta/apple/macos/Info.plist");
    }

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
}
