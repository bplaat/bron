/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("README.md")]

use std::env;
use std::path::{Path, PathBuf};

use build_utils::{
    cfg_if_symbol, display_paths, find_library, library_has_symbol, link_libraries, link_library,
    require_symbol, search_dirs,
};

fn main() {
    println!("cargo::rustc-check-cfg=cfg(gtk3_20)");
    println!("cargo::rustc-check-cfg=cfg(gtk3_22)");
    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set");
    println!("cargo::rustc-check-cfg=cfg(webkit2gtk_4_1)");
    println!("cargo::rustc-check-cfg=cfg(webkit2gtk_4_0)");
    println!("cargo::rustc-check-cfg=cfg(webkit2gtk_4_0_jsc_glib)");
    if target_os == "android" {
        // The WebView callbacks are Java classes, embedded as dex and loaded at runtime.
        let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR not set"));
        build_utils::android::compile_dex(
            Path::new("src/platforms/android"),
            &out_dir,
            build_utils::android::MIN_SDK,
        )
        .unwrap_or_else(|error| panic!("{error}"));
    }
    let target_vendor =
        env::var("CARGO_CFG_TARGET_VENDOR").expect("CARGO_CFG_TARGET_VENDOR not set");
    if target_vendor != "apple" && !matches!(target_os.as_str(), "windows" | "android") {
        let search_dirs = search_dirs();
        cfg_if_symbol(
            &search_dirs,
            "gtk-3",
            b"gtk_file_chooser_native_new",
            "gtk3_20",
        );
        cfg_if_symbol(
            &search_dirs,
            "gdk-3",
            b"gdk_display_get_n_monitors",
            "gtk3_22",
        );
        if let Some(webkit) = find_library(&search_dirs, "webkit2gtk-4.1") {
            link_library(&webkit);
            link_libraries(&search_dirs, &["javascriptcoregtk-4.1", "soup-3.0"]);
            println!("cargo::rustc-cfg=webkit2gtk_4_1");
            // WebKitGTK 4.1 starts at 2.40 and therefore requires GTK 3.22.
            println!("cargo::rustc-cfg=gtk3_20");
            println!("cargo::rustc-cfg=gtk3_22");
        } else if let Some(webkit) = find_library(&search_dirs, "webkit2gtk-4.0") {
            // webkit_cookie_manager_get_cookies was added in WebKitGTK 2.20
            // and serves as a runtime-only minimum-version marker.
            require_symbol(
                &webkit,
                b"webkit_cookie_manager_get_cookies",
                "bwebview requires WebKitGTK 4.0 version 2.20 or newer",
            );
            link_library(&webkit);
            link_libraries(&search_dirs, &["javascriptcoregtk-4.0", "soup-2.4"]);
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
