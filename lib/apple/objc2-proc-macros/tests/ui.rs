/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Compile-pass and compile-fail tests for the Objective-C macros.

#[test]
fn ui() {
    let tests = trybuild::TestCases::new();
    // The pass tests use the objc2 runtime crate, which only exists on Apple targets
    #[cfg(target_vendor = "apple")]
    tests.pass("tests/ui/pass/*.rs");
    tests.compile_fail("tests/ui/fail/*.rs");
}
