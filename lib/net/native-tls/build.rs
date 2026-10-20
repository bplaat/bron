/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("README.md")]

use std::path::PathBuf;

fn main() {
    println!("cargo::rustc-check-cfg=cfg(openssl_v10x)");
    println!("cargo::rustc-check-cfg=cfg(openssl_v4xx)");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_vendor = std::env::var("CARGO_CFG_TARGET_VENDOR").unwrap_or_default();
    let vendored = std::env::var("CARGO_FEATURE_VENDORED").is_ok();

    // On Linux/other without vendored: dynamically link the system OpenSSL.
    // With vendored: rustls handles everything in pure Rust; no linker flags needed.
    if target_vendor != "apple" && target_os != "windows" && !vendored {
        link_openssl(&target_os);
    }
}

// Link OpenSSL and emit cfg flags based on the system OpenSSL version:
//   openssl_v10x    -- 1.0.x (uses BIO_new_bio_pair + SSLv23_client_method)
//   openssl_v4xx    -- 4.x+ (uses SSL_set1_dnsname instead of SSL_set1_host)
// Only called for dynamic system linking; vendored builds use rustls.
fn link_openssl(target_os: &str) {
    println!("cargo::rerun-if-env-changed=OPENSSL_LIB_DIR");

    let mut search_dirs = Vec::new();
    if let Some(path) = std::env::var_os("OPENSSL_LIB_DIR") {
        push_unique(&mut search_dirs, PathBuf::from(path));
    }

    // pkg-config knows non-standard system prefixes when it is available. We
    // only use it for discovery; linking below always uses the actual files.
    let pkg_config_version = command_stdout("pkg-config", &["--modversion", "openssl"]);
    if let Some(path) = command_stdout("pkg-config", &["--variable=libdir", "openssl"]) {
        push_unique(&mut search_dirs, PathBuf::from(path.trim()));
    }

    if target_os == "linux" {
        // Ask the target C compiler for its multiarch directory name. This also
        // handles cross builds when CC is configured for the target.
        if let Some(multiarch) = command_stdout("cc", &["-print-multiarch"]) {
            let multiarch = multiarch.trim();
            if !multiarch.is_empty() {
                push_unique(&mut search_dirs, PathBuf::from("/usr/lib").join(multiarch));
                push_unique(&mut search_dirs, PathBuf::from("/lib").join(multiarch));
            }
        }
    }

    for path in ["/usr/local/lib", "/usr/lib64", "/lib64", "/usr/lib", "/lib"] {
        push_unique(&mut search_dirs, PathBuf::from(path));
    }

    // OpenSSL 1.0 uses several distribution-specific SONAMEs. Prefer newer
    // versions, but retain every layout supported by the implementation.
    let suffixes = [
        ".so.4",
        ".so.3",
        ".so.1.1",
        ".so.1.0.2",
        ".so.1.0.0",
        ".so.1.0",
        ".so.10",
        ".so",
    ];
    for directory in &search_dirs {
        for suffix in suffixes {
            let ssl_name = format!("libssl{suffix}");
            let crypto_name = format!("libcrypto{suffix}");
            if directory.join(&ssl_name).is_file() && directory.join(&crypto_name).is_file() {
                println!("cargo::rustc-link-search=native={}", directory.display());
                println!("cargo::rustc-link-lib=dylib:+verbatim={ssl_name}");
                println!("cargo::rustc-link-lib=dylib:+verbatim={crypto_name}");
                let detected_version = std::fs::canonicalize(directory.join(&ssl_name))
                    .ok()
                    .and_then(|path| path.file_name()?.to_str().map(str::to_owned))
                    .or_else(|| pkg_config_version.clone())
                    .or_else(|| command_stdout("openssl", &["version"]))
                    .unwrap_or_else(|| suffix.to_owned());
                emit_version_cfg(&detected_version);
                return;
            }
        }
    }

    panic!(
        "could not find a supported system OpenSSL library pair (1.0.2, 1.1, 3, or 4); searched: {}. Set OPENSSL_LIB_DIR to the directory containing libssl and libcrypto, or enable the `vendored` feature",
        search_dirs
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
}

fn command_stdout(command: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(command)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy_owned(output.stdout))
}

fn push_unique(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !path.as_os_str().is_empty() && !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

fn emit_version_cfg(version: &str) {
    if version.contains("1.0.") || version.ends_with(".so.10") {
        println!("cargo::rustc-cfg=openssl_v10x");
    } else if version.starts_with("4.")
        || version.contains("OpenSSL 4.")
        || version.ends_with(".so.4")
    {
        println!("cargo::rustc-cfg=openssl_v4xx");
    }
    // 1.1.x / 3.x: no extra cfg needed; they use the default code path.
}
