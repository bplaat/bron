/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Builds the Wii U libraries, configures the Rust linker and returns the build directory.
pub fn build(toolchain: impl AsRef<Path>) -> PathBuf {
    let toolchain = toolchain
        .as_ref()
        .canonicalize()
        .expect("Wii U toolchain not found");
    let status = Command::new(toolchain.join("build.sh"))
        .status()
        .expect("failed to run build.sh");
    assert!(status.success(), "failed to build the toolchain");
    for path in [
        "build.rs",
        "build.sh",
        "decaf-emu",
        "libogc",
        "picolibc",
        "wiiu.ld",
        "wiiu.json",
        "wut",
        "wut-tools",
    ] {
        println!("cargo:rerun-if-changed={}", toolchain.join(path).display());
    }
    let build_dir = toolchain.join("../../target/wiiu-toolchain");
    // elf2rpl needs the relocations to make the imports of the system libraries, the linker
    // script includes the import sections from the build directory
    println!(
        "cargo:rustc-link-arg=-T{}",
        toolchain.join("wiiu.ld").display()
    );
    println!("cargo:rustc-link-arg=--emit-relocs");
    println!("cargo:rustc-link-arg=-znocopyreloc");
    // picolibc's libc.a comes first, so an old libc.a in the build directory is never used
    println!(
        "cargo:rustc-link-search=native={}",
        build_dir.join("sysroot/lib").display()
    );
    println!("cargo:rustc-link-search=native={}", build_dir.display());
    println!("cargo:rustc-link-lib=static=wut");
    println!("cargo:rustc-link-lib=static=gu");
    println!("cargo:rustc-link-lib=static=c");
    build_dir
}

/// Assembles a vertex and pixel shader with latte-assembler to `shader.gsh` in `OUT_DIR`.
pub fn assemble_shader(build_dir: &Path, vertex: &Path, pixel: &Path) -> PathBuf {
    println!("cargo:rerun-if-changed={}", vertex.display());
    println!("cargo:rerun-if-changed={}", pixel.display());
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR not set")).join("shader.gsh");
    let status = Command::new(build_dir.join("latte-assembler"))
        .arg("assemble")
        .arg(format!("--vsh={}", vertex.display()))
        .arg(format!("--psh={}", pixel.display()))
        .arg(&output)
        .status()
        .expect("failed to run latte-assembler");
    assert!(status.success(), "failed to assemble the shaders");
    output
}
