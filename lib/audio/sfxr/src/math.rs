/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The float methods that std adds to core, without std they call the C library.
#![allow(dead_code)] // A dependency may link std and supply inherent float methods.
#![allow(unsafe_code)]

unsafe extern "C" {
    safe fn sinf(x: f32) -> f32;
}

pub(crate) trait Float: Sized {
    fn sin(self) -> Self;
    fn powi(self, n: i32) -> Self;
}

impl Float for f32 {
    fn sin(self) -> Self {
        sinf(self)
    }

    fn powi(self, n: i32) -> Self {
        (0..n).fold(1.0, |value, _| value * self)
    }
}
