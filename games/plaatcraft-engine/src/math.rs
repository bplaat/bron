/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The float methods that std adds to core, without std they call the C library.
#![allow(unsafe_code)]

unsafe extern "C" {
    safe fn floor(x: f64) -> f64;
    safe fn ceil(x: f64) -> f64;
    safe fn round(x: f64) -> f64;
    safe fn sqrt(x: f64) -> f64;
    safe fn exp(x: f64) -> f64;
    safe fn sin(x: f64) -> f64;
    safe fn cos(x: f64) -> f64;
    safe fn tan(x: f64) -> f64;
    safe fn hypot(x: f64, y: f64) -> f64;
    safe fn log(x: f64) -> f64;
    safe fn pow(x: f64, y: f64) -> f64;
    safe fn floorf(x: f32) -> f32;
    safe fn ceilf(x: f32) -> f32;
    safe fn roundf(x: f32) -> f32;
    safe fn sqrtf(x: f32) -> f32;
    safe fn expf(x: f32) -> f32;
    safe fn sinf(x: f32) -> f32;
    safe fn cosf(x: f32) -> f32;
    safe fn tanf(x: f32) -> f32;
    safe fn hypotf(x: f32, y: f32) -> f32;
    safe fn logf(x: f32) -> f32;
    safe fn powf(x: f32, y: f32) -> f32;
}

pub(crate) trait Float: Sized {
    fn floor(self) -> Self;
    fn ceil(self) -> Self;
    fn round(self) -> Self;
    fn sqrt(self) -> Self;
    fn exp(self) -> Self;
    fn sin(self) -> Self;
    fn sin_cos(self) -> (Self, Self);
    fn tan(self) -> Self;
    fn hypot(self, other: Self) -> Self;
    fn ln(self) -> Self;
    fn powf(self, exponent: Self) -> Self;
    fn rem_euclid(self, rhs: Self) -> Self;
}

macro_rules! float {
    ($type:ty, $floor:ident, $ceil:ident, $round:ident, $sqrt:ident, $exp:ident, $sin:ident, $cos:ident, $tan:ident, $hypot:ident, $ln:ident, $pow:ident) => {
        impl Float for $type {
            fn floor(self) -> Self {
                $floor(self)
            }
            fn ceil(self) -> Self {
                $ceil(self)
            }
            fn round(self) -> Self {
                $round(self)
            }
            fn sqrt(self) -> Self {
                $sqrt(self)
            }
            fn exp(self) -> Self {
                $exp(self)
            }
            fn sin(self) -> Self {
                $sin(self)
            }
            fn sin_cos(self) -> (Self, Self) {
                ($sin(self), $cos(self))
            }
            fn tan(self) -> Self {
                $tan(self)
            }
            fn hypot(self, other: Self) -> Self {
                $hypot(self, other)
            }
            fn ln(self) -> Self {
                $ln(self)
            }
            fn powf(self, exponent: Self) -> Self {
                $pow(self, exponent)
            }
            fn rem_euclid(self, rhs: Self) -> Self {
                let r = self % rhs;
                if r < 0.0 { r + rhs.abs() } else { r }
            }
        }
    };
}

float!(
    f64, floor, ceil, round, sqrt, exp, sin, cos, tan, hypot, log, pow
);
float!(
    f32, floorf, ceilf, roundf, sqrtf, expf, sinf, cosf, tanf, hypotf, logf, powf
);
