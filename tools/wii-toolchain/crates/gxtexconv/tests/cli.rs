/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(missing_docs)]

use std::fs;
use std::path::Path;
use std::process::Command;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

#[test]
fn writes_tpl() {
    let output = std::env::temp_dir().join(format!("gxtexconv-cli-{}.tpl", std::process::id()));
    let executable = env!("CARGO_BIN_EXE_gxtexconv");
    assert!(
        Command::new(executable)
            .arg("-s")
            .arg(Path::new(FIXTURES).join("textures.scf"))
            .arg("-o")
            .arg(&output)
            .status()
            .expect("run gxtexconv")
            .success()
    );
    let expected = fs::read(Path::new(FIXTURES).join("textures.tpl")).expect("read expected");
    assert!(fs::read(&output).expect("read output") == expected);
    fs::remove_file(&output).expect("remove output");
}

#[test]
fn prints_usage_without_arguments() {
    let output = Command::new(env!("CARGO_BIN_EXE_gxtexconv"))
        .output()
        .expect("run gxtexconv");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("Usage:"));
}
