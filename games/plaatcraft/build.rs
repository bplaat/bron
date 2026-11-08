/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("README.md")]

fn main() -> bwebgpu_shader_build::Result<()> {
    println!("cargo:rerun-if-env-changed=CARGO_BUNDLE");

    println!("cargo:rerun-if-changed=assets");

    bwebgpu_shader_build::Builder::new()
        .shader("terrain.wgsl", "src/shaders/terrain.wgsl")
        .shader("liquid.wgsl", "src/shaders/liquid.wgsl")
        .shader("sky.wgsl", "src/shaders/sky.wgsl")
        .shader("crosshair.wgsl", "src/shaders/crosshair.wgsl")
        .shader("selection.wgsl", "src/shaders/selection.wgsl")
        .shader("preview.wgsl", "src/shaders/preview.wgsl")
        .shader("player.wgsl", "src/shaders/player.wgsl")
        .shader("held.wgsl", "src/shaders/held.wgsl")
        .shader("hud.wgsl", "src/shaders/hud.wgsl")
        .compile()?;

    if std::env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set") == "macos"
        && std::env::var_os("CARGO_BUNDLE").is_none()
    {
        build_utils::macos::embed_info_plist("meta/apple/macos/Info.plist");
    }

    if std::env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set") == "windows" {
        build_utils::windows::compile_application(
            "PlaatCraft",
            "PlaatCraft voxel sandbox",
            "Copyright (c) 2026 Bastiaan van der Plaat",
            "meta/windows/icon.ico",
            &[],
        )
        .expect("failed to compile Windows resources");
    }

    Ok(())
}
