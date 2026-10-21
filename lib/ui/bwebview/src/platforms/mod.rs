/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

pub(crate) use crate::platforms::scripts::*;

mod scripts;

cfg_select! {
    any(
        target_os = "linux",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "openbsd",
        target_os = "netbsd"
    ) => {
        pub(crate) use crate::platforms::gtk::*;
        mod gtk;
    }
    target_vendor = "apple" => {
        pub(crate) use crate::platforms::apple::*;
        mod apple;
    }
    windows => {
        pub(crate) use crate::platforms::windows::*;
        mod windows;
    }
    target_os = "android" => {
        pub(crate) use crate::platforms::android::*;
        mod android;
    }
    _ => {
        compile_error!("Unsupported platform");
    }
}
