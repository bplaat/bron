/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The page in a native scroll view: a Win32 child window with scroll bars, an AppKit
//! NSScrollView or a GTK scrolled window. The view scrolls by itself, with its scroll bar, the
//! wheel and the keyboard, and draws with bcanvas. It paints through the window's
//! `RedrawRequested` and forwards the pointer as the window's events, so the app handles both
//! like for any window content.

#![allow(unsafe_code)]

/// The distance of a scroll bar arrow, an arrow key or a wheel notch in logical pixels.
const LINE: f32 = 40.0;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub(crate) use macos::ScrollView;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::ScrollView;

#[cfg(not(any(target_vendor = "apple", windows, target_os = "android")))]
mod gtk;
#[cfg(not(any(target_vendor = "apple", windows, target_os = "android")))]
pub(crate) use gtk::ScrollView;
