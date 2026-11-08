/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("README.md")]

use std::fs::OpenOptions;
use std::path::Path;

use copy_dir::copy_dir;

fn main() {
    const NPM: &str = if cfg!(windows) { "npm.cmd" } else { "npm" };

    // Install npm packages at the npm workspace root if needed
    println!("cargo:rerun-if-changed=../../../.npmrc");
    println!("cargo:rerun-if-changed=../../../package.json");
    println!("cargo:rerun-if-changed=../../../package-lock.json");
    {
        let npm_lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(std::env::temp_dir().join("bplaat-bron-npm-install.lock"))
            .expect("Failed to open npm install lock file");
        npm_lock
            .lock()
            .expect("Failed to lock npm install lock file");
        if !Path::new("../../../node_modules/.package-lock.json").exists() {
            let status = std::process::Command::new(NPM)
                .arg("ci")
                .current_dir("../../..")
                .status()
                .expect("Failed to run npm ci");
            assert!(status.success(), "npm ci failed with {status}");
        }
    }

    // Invalidate build when web assets change
    fn print_rerun(dir: &Path) {
        for entry in std::fs::read_dir(dir).expect("Failed to read dir") {
            let path = entry.expect("Failed to read entry").path();
            if path.is_dir() {
                let file_name = path.file_name().expect("Should have a file name");
                if file_name == "dist" || file_name == "node_modules" {
                    continue;
                }
                print_rerun(&path);
            } else {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
    print_rerun(Path::new("web"));

    // Build frontend
    let status = std::process::Command::new(NPM)
        .arg("run")
        .arg(if cfg!(debug_assertions) {
            "build-debug"
        } else {
            "build-release"
        })
        .current_dir("web")
        .status()
        .expect("Failed to run npm run build");
    assert!(status.success(), "npm run build failed with {status}");

    // Copy built assets to OUT_DIR/web
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    copy_dir("web/dist", Path::new(&out_dir).join("web"))
        .expect("Failed to copy web/dist files to $OUT_DIR");

    if std::env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set") == "windows" {
        build_utils::windows::compile_application(
            "BassieLight",
            "BassieLight",
            "Copyright © 2025 Bastiaan van der Plaat",
            "meta/windows/icon.ico",
            &[],
        )
        .expect("failed to compile Windows resources");
    }
}
