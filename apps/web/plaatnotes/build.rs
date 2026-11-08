/*
 * Copyright (c) 2024-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("README.md")]

use std::env;
use std::path::PathBuf;

fn main() {
    // Generate openapi file
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));
    openapi_generator::generate_schemas_build(
        "openapi.yaml",
        out_dir.join("api.rs"),
        openapi_generator::Generator::Rust,
    );
    openapi_generator::generate_schemas_build(
        "openapi.yaml",
        "web/src-gen/api.ts",
        openapi_generator::Generator::TypeScript,
    );

    // Database migrations
    println!("cargo:rerun-if-changed=src/migrations");

    build_utils::npm_build(
        "../../..",
        "web",
        if cfg!(debug_assertions) {
            "build-debug"
        } else {
            "build-release"
        },
    );
}
