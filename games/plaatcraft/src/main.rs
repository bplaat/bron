/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#![allow(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

#[cfg(not(any(target_os = "wii", target_os = "wiiu")))]
mod desktop;
#[cfg(target_os = "wii")]
mod wii;
#[cfg(target_os = "wiiu")]
mod wiiu;

#[cfg(not(any(target_os = "wii", target_os = "wiiu")))]
use desktop::main;
#[cfg(target_os = "wii")]
use wii::main;
#[cfg(target_os = "wiiu")]
use wiiu::main;
