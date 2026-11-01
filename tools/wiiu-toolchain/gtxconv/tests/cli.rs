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
fn writes_gtx() {
    let output = std::env::temp_dir().join(format!("gtxconv-cli-{}.gtx", std::process::id()));
    let config = Path::new(FIXTURES).join("textures.toml");
    assert!(
        Command::new(env!("CARGO_BIN_EXE_gtxconv"))
            .arg("-c")
            .arg(&config)
            .arg("-o")
            .arg(&output)
            .status()
            .expect("run gtxconv")
            .success()
    );
    let expected = gtxconv::convert_config(&config).expect("convert config");
    assert!(fs::read(&output).expect("read output") == expected);
    fs::remove_file(&output).expect("remove output");
}

#[test]
fn prints_usage_without_config() {
    let output = Command::new(env!("CARGO_BIN_EXE_gtxconv"))
        .args(["-o", "textures.gtx"])
        .output()
        .expect("run gtxconv");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("Usage:"));
}
