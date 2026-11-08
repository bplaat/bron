/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("README.md")]

use std::env;

use build_utils::{gtk_cfgs, library_has_symbol, link_library, require_library, search_dirs};

fn main() {
    println!("cargo::rustc-check-cfg=cfg(gtk3_20)");
    println!("cargo::rustc-check-cfg=cfg(gtk3_22)");
    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set");
    let target_vendor =
        env::var("CARGO_CFG_TARGET_VENDOR").expect("CARGO_CFG_TARGET_VENDOR not set");
    if target_vendor != "apple" && !matches!(target_os.as_str(), "windows" | "android") {
        let search_dirs = search_dirs();
        let gtk = require_library(&search_dirs, "gtk-3");
        let gdk = require_library(&search_dirs, "gdk-3");
        // gtk_widget_set_font_map was added in GTK 3.18 and serves as a
        // runtime-only minimum-version marker.
        if !library_has_symbol(&gtk, b"gtk_widget_set_font_map") {
            panic!("bwindow requires GTK 3.18 or newer");
        }
        for name in ["gobject-2.0", "glib-2.0", "gio-2.0"] {
            link_library(&require_library(&search_dirs, name));
        }
        link_library(&gtk);
        link_library(&gdk);

        gtk_cfgs(&search_dirs);
    }
}
