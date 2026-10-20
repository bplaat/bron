/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

cfg_select! {
    any(
        target_os = "linux",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "openbsd",
        target_os = "netbsd"
    ) => {
        pub(crate) use crate::platforms::gtk::*;
        pub(crate) mod gtk;
    }
    target_os = "macos" => {
        pub(crate) use crate::platforms::macos::*;
        pub(crate) mod macos;
    }
    target_os = "ios" => {
        pub(crate) use crate::platforms::ios::*;
        pub(crate) mod ios;
    }
    windows => {
        pub(crate) use crate::platforms::windows::*;
        pub(crate) mod windows;
    }
    _ => {
        compile_error!("Unsupported platform");
    }
}
