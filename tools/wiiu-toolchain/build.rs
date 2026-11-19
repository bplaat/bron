/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Converts the textures of a gtxconv config to `textures.gtx` in `OUT_DIR`.
#[allow(dead_code, reason = "not every program has textures")]
pub fn convert_textures(build_dir: &Path, config: &Path) -> PathBuf {
    let tool = build_dir.join("bin/gtxconv");
    println!("cargo:rerun-if-changed={}", tool.display());
    println!("cargo:rerun-if-changed={}", config.display());
    let directory = config.parent().expect("config has a directory");
    for entry in std::fs::read_dir(directory).expect("failed to read the textures directory") {
        println!(
            "cargo:rerun-if-changed={}",
            entry.expect("failed to read entry").path().display()
        );
    }
    let output =
        PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR not set")).join("textures.gtx");
    let status = Command::new(tool)
        .arg("-c")
        .arg(config)
        .arg("-o")
        .arg(&output)
        .status()
        .expect("failed to run gtxconv");
    assert!(status.success(), "failed to convert the textures");
    output
}

/// Assembles a vertex and pixel shader with latte-assembler to `shader.gsh` in `OUT_DIR`.
pub fn assemble_shader(build_dir: &Path, vertex: &Path, pixel: &Path) -> PathBuf {
    let tool = build_dir.join("bin/latte-assembler");
    println!("cargo:rerun-if-changed={}", tool.display());
    println!("cargo:rerun-if-changed={}", vertex.display());
    println!("cargo:rerun-if-changed={}", pixel.display());
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR not set")).join("shader.gsh");
    let status = Command::new(tool)
        .arg("assemble")
        .arg(format!("--vsh={}", vertex.display()))
        .arg(format!("--psh={}", pixel.display()))
        .arg(&output)
        .status()
        .expect("failed to run latte-assembler");
    assert!(status.success(), "failed to assemble the shaders");
    output
}
