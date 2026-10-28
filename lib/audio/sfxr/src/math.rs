/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The float methods that std adds to core, without std they call the C library.
#![allow(unsafe_code)]

unsafe extern "C" {
    safe fn sin(x: f64) -> f64;
    safe fn sinf(x: f32) -> f32;
}

pub(crate) trait Float: Sized {
    fn sin(self) -> Self;
    fn powi(self, n: i32) -> Self;
}

macro_rules! float {
    ($type:ty, $sin:ident) => {
        impl Float for $type {
            fn sin(self) -> Self {
                $sin(self)
            }
            fn powi(self, n: i32) -> Self {
                (0..n).fold(1.0, |value, _| value * self)
            }
        }
    };
}

float!(f64, sin);
float!(f32, sinf);
