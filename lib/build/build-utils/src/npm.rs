/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fs::OpenOptions;
use std::path::Path;

use copy_dir::copy_dir;

/// Installs npm workspace dependencies if needed, builds a frontend, and copies its assets.
///
/// Paths are relative to the calling build script's working directory. Source files are
/// tracked recursively, excluding `dist` and `node_modules`. The frontend's `dist` directory
/// is copied to `OUT_DIR` under the frontend directory's name.
pub fn npm_build(workspace: impl AsRef<Path>, frontend: impl AsRef<Path>, script: &str) {
    const NPM: &str = if cfg!(windows) { "npm.cmd" } else { "npm" };

    let workspace = workspace.as_ref();
    let frontend = frontend.as_ref();

    for file in [".npmrc", "package.json", "package-lock.json"] {
        println!("cargo:rerun-if-changed={}", workspace.join(file).display());
    }
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
        if !workspace.join("node_modules/.package-lock.json").exists() {
            let status = std::process::Command::new(NPM)
                .arg("ci")
                .current_dir(workspace)
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
    print_rerun(frontend);

    // Build frontend
    let status = std::process::Command::new(NPM)
        .arg("run")
        .arg(script)
        .current_dir(frontend)
        .status()
        .expect("Failed to run npm run build");
    assert!(status.success(), "npm run build failed with {status}");

    // Copy built assets into the build output directory.
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    copy_dir(
        frontend.join("dist"),
        Path::new(&out_dir).join(frontend.file_name().expect("frontend directory has a name")),
    )
    .expect("Failed to copy frontend assets to OUT_DIR");
}
