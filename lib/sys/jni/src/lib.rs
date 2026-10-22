/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [jni](https://crates.io/crates/jni) crate

#![allow(unsafe_code)]
#![allow(clippy::undocumented_unsafe_blocks)]

pub use crate::env::{Env, EnvOutcome, EnvUnowned, NativeMethod};
pub use crate::objects::{JValue, JValueOwned};

mod env;
pub mod errors;
mod macros;
pub mod objects;
pub mod refs;
pub mod signature;
pub mod strings;
pub mod sys;
pub mod vm;
