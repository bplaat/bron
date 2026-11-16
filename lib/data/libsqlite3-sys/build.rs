/*
 * Copyright (c) 2024-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [libsqlite3-sys](https://crates.io/crates/libsqlite3-sys) crate.

fn main() {
    let env = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("{name} not set"));
    // Windows 7 has no system winsqlite3.dll. Inspect the Cargo target rather than
    // cfg!(windows), which describes the host compiling this build script.
    let win7 = env("CARGO_CFG_TARGET_OS") == "windows" && env("CARGO_CFG_TARGET_VENDOR") == "win7";
    if cfg!(feature = "bundled") || win7 {
        let mut build = cc::Build::new();
        build.file("sqlite3/sqlite3.c");
        if cfg!(feature = "column-metadata") {
            build.define("SQLITE_ENABLE_COLUMN_METADATA", None);
        }
        if cfg!(feature = "fts5") {
            build.define("SQLITE_ENABLE_FTS5", None);
        }
        if env("CARGO_CFG_TARGET_OS") == "none" {
            // Bare metal targets with a POSIX C library, like the Wii, run one process without
            // threads or file locks. Cargo names custom targets after their JSON file, which
            // cc can't parse, and their C compiler (TARGET_CC) brings all flags.
            build
                .target(&format!("{}-unknown-none", env("CARGO_CFG_TARGET_ARCH")))
                .no_default_flags(true)
                .warnings(false)
                .define("SQLITE_THREADSAFE", "0")
                .define("SQLITE_OS_UNIX", "1")
                .define("SQLITE_DEFAULT_UNIX_VFS", "\"unix-none\"")
                .define("SQLITE_TEMP_STORE", "3")
                .define("SQLITE_DISABLE_LFS", None)
                .define("SQLITE_DISABLE_DIRSYNC", None)
                .define("SQLITE_OMIT_WAL", None)
                .define("SQLITE_OMIT_LOAD_EXTENSION", None)
                .define("SQLITE_OMIT_LOCALTIME", None)
                .define("SQLITE_DEFAULT_MEMSTATUS", "0")
                .define("SQLITE_MAX_MMAP_SIZE", "0");
        } else {
            build.define("SQLITE_THREADSAFE", "1");
        }
        build.compile("sqlite3");
    } else if env("CARGO_CFG_TARGET_OS") == "windows" {
        println!("cargo:rustc-link-lib=winsqlite3");
    } else {
        println!("cargo:rustc-link-lib=sqlite3");
    }
}
