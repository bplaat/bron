/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Command-line behavior for the sha1sum binary.

#![cfg(feature = "std")]

use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn hashes_standard_input() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_sha1sum"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("start sha1sum");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"abc")
        .expect("write input");
    let output = child.wait_with_output().expect("wait for sha1sum");
    assert!(output.status.success());
    assert_eq!(
        output.stdout,
        b"a9993e364706816aba3e25717850c26c9cd0d89d  -\n"
    );
}
