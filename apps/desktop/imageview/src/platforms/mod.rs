/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Each platform provides how to browse folders and remember recent files, and a `MediaView`:
//! a native scroll view filling a window that zooms and pans the media drawn with bcanvas,
//! following the system light or dark theme.

cfg_select! {
    target_os = "macos" => {
        pub(crate) use self::macos::*;
        mod macos;
    }
    windows => {
        pub(crate) use self::shared::*;
        pub(crate) use self::windows::*;
        mod shared;
        mod windows;
    }
    _ => {
        pub(crate) use self::gtk::*;
        pub(crate) use self::shared::*;
        mod gtk;
        mod shared;
    }
}
