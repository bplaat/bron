/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("README.md")]

use std::env;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo::rustc-check-cfg=cfg(gtk3_20)");
    println!("cargo::rustc-check-cfg=cfg(gtk3_22)");
    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set");
    println!("cargo::rustc-check-cfg=cfg(webkit2gtk_4_1)");
    println!("cargo::rustc-check-cfg=cfg(webkit2gtk_4_0)");
    println!("cargo::rustc-check-cfg=cfg(webkit2gtk_4_0_jsc_glib)");
    let target_vendor =
        env::var("CARGO_CFG_TARGET_VENDOR").expect("CARGO_CFG_TARGET_VENDOR not set");
    if target_vendor != "apple" && target_os != "windows" {
        let search_dirs = search_dirs();
        gtk_cfgs(&search_dirs);
        if let Some(webkit) = find_library(&search_dirs, "webkit2gtk-4.1") {
            link_library(&webkit);
            link_library(&require_library(&search_dirs, "javascriptcoregtk-4.1"));
            link_library(&require_library(&search_dirs, "soup-3.0"));
            println!("cargo::rustc-cfg=webkit2gtk_4_1");
            // WebKitGTK 4.1 starts at 2.40 and therefore requires GTK 3.22.
            println!("cargo::rustc-cfg=gtk3_20");
            println!("cargo::rustc-cfg=gtk3_22");
        } else if let Some(webkit) = find_library(&search_dirs, "webkit2gtk-4.0") {
            // webkit_cookie_manager_get_cookies was added in WebKitGTK 2.20
            // and serves as a runtime-only minimum-version marker.
            if !library_has_symbol(&webkit, b"webkit_cookie_manager_get_cookies") {
                panic!("bwebview requires WebKitGTK 4.0 version 2.20 or newer");
            }
            link_library(&webkit);
            link_library(&require_library(&search_dirs, "javascriptcoregtk-4.0"));
            link_library(&require_library(&search_dirs, "soup-2.4"));
            if library_has_symbol(&webkit, b"webkit_javascript_result_get_js_value") {
                println!("cargo::rustc-cfg=webkit2gtk_4_0_jsc_glib");
            }
            println!("cargo::rustc-cfg=webkit2gtk_4_0");
        } else {
            panic!(
                "could not find the WebKitGTK runtime library (webkit2gtk-4.1 or webkit2gtk-4.0); searched: {}",
                display_paths(&search_dirs)
            );
        }
    }
}

fn find_library(search_dirs: &[PathBuf], name: &str) -> Option<PathBuf> {
    let prefix = format!("lib{name}.so");
    for directory in search_dirs {
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        let mut candidates = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|file| file.to_str())
                    .is_some_and(|file| file == prefix || file.starts_with(&format!("{prefix}.")))
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|path| path.file_name().is_some_and(|file| file == prefix.as_str()));
        if let Some(path) = candidates.into_iter().find(|path| path.is_file()) {
            return Some(path);
        }
    }
    None
}

fn require_library(search_dirs: &[PathBuf], name: &str) -> PathBuf {
    find_library(search_dirs, name).unwrap_or_else(|| {
        panic!(
            "could not find the {name} runtime library; searched: {}",
            display_paths(search_dirs)
        )
    })
}

fn link_library(path: &Path) {
    let directory = path
        .parent()
        .expect("library should have a parent directory");
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("library filename should be valid UTF-8");
    println!("cargo::rustc-link-search=native={}", directory.display());
    println!("cargo::rustc-link-lib=dylib:+verbatim={file_name}");
}

fn library_has_symbol(path: &Path, symbol: &[u8]) -> bool {
    std::fs::read(path)
        .is_ok_and(|bytes| bytes.windows(symbol.len()).any(|window| window == symbol))
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

fn display_paths(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn search_dirs() -> Vec<PathBuf> {
    let mut search_dirs = Vec::new();
    if let Some(multiarch) = command_stdout("cc", &["-print-multiarch"]) {
        let multiarch = multiarch.trim();
        if !multiarch.is_empty() {
            push_unique(&mut search_dirs, PathBuf::from("/usr/lib").join(multiarch));
            push_unique(&mut search_dirs, PathBuf::from("/lib").join(multiarch));
        }
    }
    for path in [
        "/usr/local/lib",
        "/usr/pkg/lib",
        "/usr/lib64",
        "/lib64",
        "/usr/lib",
        "/lib",
    ] {
        push_unique(&mut search_dirs, PathBuf::from(path));
    }
    search_dirs
}

fn gtk_cfgs(search_dirs: &[PathBuf]) {
    if let Some(gtk) = find_library(search_dirs, "gtk-3")
        && library_has_symbol(&gtk, b"gtk_file_chooser_native_new")
    {
        println!("cargo::rustc-cfg=gtk3_20");
    }
    if let Some(gdk) = find_library(search_dirs, "gdk-3")
        && library_has_symbol(&gdk, b"gdk_display_get_n_monitors")
    {
        println!("cargo::rustc-cfg=gtk3_22");
    }
}
