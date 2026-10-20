/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]
#![allow(unused)]
#![allow(unsafe_code)]
#![allow(clippy::undocumented_unsafe_blocks)]

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
mod menu;
mod platforms;
mod sizes;
mod window;

/// Platform-specific native ABI declarations for implementing content backends.
#[doc(hidden)]
#[allow(missing_docs)]
pub mod ffi {
    #[cfg(all(not(any(target_vendor = "apple", windows)), feature = "file_drop"))]
    pub use crate::platforms::gtk::file_drop::*;
    #[cfg(not(any(target_vendor = "apple", windows)))]
    pub use crate::platforms::gtk::input::*;
    #[cfg(target_os = "ios")]
    pub use crate::platforms::ios::input::*;
    #[cfg(all(target_os = "macos", feature = "file_drop"))]
    pub use crate::platforms::macos::file_drop::{perform_file_drop, register_dragged_types};
    #[cfg(target_os = "macos")]
    pub use crate::platforms::macos::input::*;
    pub use crate::platforms::native_headers::*;
    #[cfg(windows)]
    pub use crate::platforms::windows::input::*;
}
