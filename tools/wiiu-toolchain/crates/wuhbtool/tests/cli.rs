/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(missing_docs)]

use std::fs;
use std::path::Path;
use std::process::Command;

use wuhbtool::Options;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

#[test]
fn writes_wuhb_with_wuhbtool_arguments() {
    let fixtures = Path::new(FIXTURES);
    let output = std::env::temp_dir().join(format!("wuhbtool-cli-{}.wuhb", std::process::id()));
    let status = Command::new(env!("CARGO_BIN_EXE_wuhbtool"))
        .arg(fixtures.join("program.rpx"))
        .arg(&output)
        .arg("--name=Test Program")
        .args(["--author", "Tester"])
        .arg(format!("--icon={}", fixtures.join("icon.png").display()))
        .arg(format!("--content={}", fixtures.join("content").display()))
        .status()
        .expect("run wuhbtool");
    assert!(status.success());
    let expected = wuhbtool::build(&Options {
        rpx: fixtures.join("program.rpx"),
        name: Some("Test Program".to_owned()),
        author: Some("Tester".to_owned()),
        icon: Some(fixtures.join("icon.png")),
        content: Some(fixtures.join("content")),
        ..Options::default()
    })
    .expect("build bundle");
    assert!(fs::read(&output).expect("read output") == expected.data);
    fs::remove_file(&output).expect("remove output");
}

#[test]
fn prints_help_without_arguments() {
    let output = Command::new(env!("CARGO_BIN_EXE_wuhbtool"))
        .output()
        .expect("run wuhbtool");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("Usage:"));
}

#[test]
fn rejects_unknown_options() {
    let output = Command::new(env!("CARGO_BIN_EXE_wuhbtool"))
        .args(["a.rpx", "a.wuhb", "--color=red"])
        .output()
        .expect("run wuhbtool");
    assert!(!output.status.success());
}
