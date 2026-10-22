/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]
#![allow(unsafe_code)]
#![allow(clippy::undocumented_unsafe_blocks)]

#[cfg(all(feature = "dialog", any(target_os = "ios", target_os = "android")))]
compile_error!("The `dialog` feature is unsupported on iOS and Android");

pub use crate::content::*;
#[cfg(feature = "dialog")]
pub use crate::dialog::*;
pub use crate::event::*;
pub use crate::event_loop::*;
pub use crate::input::*;
pub use crate::menu::*;
pub use crate::sizes::*;
pub use crate::window::*;

mod content;
#[cfg(feature = "dialog")]
mod dialog;
mod dispatch;
mod event;
mod event_loop;
mod input;
mod mailbox;
// GTK always builds its menus from the menu types, macOS with the menu feature only and
// Windows only uses their accelerators.
#[cfg_attr(
    not(any(
        all(target_os = "macos", feature = "menu"),
        not(any(target_vendor = "apple", target_os = "android", windows))
    )),
    allow(
        dead_code,
        reason = "the menu types are partially used on this platform"
    )
)]
mod menu;
mod platforms;
mod sizes;
mod window;

/// Platform-specific native ABI declarations for implementing content backends.
#[doc(hidden)]
#[allow(missing_docs)]
pub mod ffi {
    #[cfg(target_os = "android")]
    pub use crate::platforms::android::ffi::*;
    #[cfg(all(
        not(any(target_vendor = "apple", target_os = "android", windows)),
        feature = "file_drop"
    ))]
    pub use crate::platforms::gtk::file_drop::gtk_dropped_paths;
    #[cfg(all(target_os = "macos", feature = "file_drop"))]
    pub use crate::platforms::macos::file_drop::{perform_file_drop, register_dragged_types};
    #[cfg(windows)]
    pub use crate::platforms::windows::input::windows_translate_accelerator;
}
