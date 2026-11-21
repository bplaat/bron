/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The v86 guest integration for Windows XP: the host protocol the programs and drivers share,
//! with the bindings of the Windows APIs they use. The drivers build for the
//! `i686-winxp-windows-driver` target, which has no target environment, the programs for
//! `i686-winxp-windows-gnu` with std.

#![no_std]

#[cfg(all(windows, target_env = "gnu"))]
extern crate std;

pub mod protocol;

#[cfg(all(windows, not(target_env = "gnu")))]
pub mod ddk;
#[cfg(target_arch = "x86")]
pub mod port;
#[cfg(all(windows, target_env = "gnu"))]
pub mod win32;
