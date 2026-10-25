/*
 * Copyright (c) 2024-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [libsqlite3-sys](https://crates.io/crates/libsqlite3-sys) crate.

fn main() {
    cfg_select! {
        // Compile and link the SQLite library from source
        feature = "bundled" => {
            let mut build = cc::Build::new();
            build
                .file("sqlite3/sqlite3.c")
                .define("SQLITE_THREADSAFE", "1")
                .define("SQLITE_ENABLE_COLUMN_METADATA", None);
            if cfg!(feature = "bundled-fts5") {
                build.define("SQLITE_ENABLE_FTS5", None);
            }
            build.compile("sqlite3");
        }
        // Or link to the system SQLite library
        _ => {
            if std::env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set")
                == "windows"
            {
                println!("cargo:rustc-link-lib=winsqlite3");
            } else {
                println!("cargo:rustc-link-lib=sqlite3");
            }
        }
    }
}
