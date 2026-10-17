/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Command-line behavior for the sha256sum binary.

#![cfg(feature = "std")]

use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn hashes_standard_input() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_sha256sum"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("start sha256sum");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"abc")
        .expect("write input");
    let output = child.wait_with_output().expect("wait for sha256sum");
    assert!(output.status.success());
    assert_eq!(
        output.stdout,
        b"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  -\n"
    );
}
