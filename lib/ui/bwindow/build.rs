/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("README.md")]

use std::env;

use build_utils::{
    cfg_if_symbol, link_libraries, link_library, require_library, require_symbol, search_dirs,
};

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
        require_symbol(
            &gtk,
            b"gtk_widget_set_font_map",
            "bwindow requires GTK 3.18 or newer",
        );
        link_libraries(&search_dirs, &["gobject-2.0", "glib-2.0", "gio-2.0"]);
        link_library(&gtk);
        link_library(&gdk);

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
    }
}
