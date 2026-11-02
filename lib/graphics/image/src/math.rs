/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Runtime math uses std when enabled and the C library otherwise.
#![allow(unsafe_code)]

#[cfg(not(feature = "std"))]
unsafe extern "C" {
    pub(crate) safe fn exp(x: f64) -> f64;
    pub(crate) safe fn pow(x: f64, y: f64) -> f64;
    #[cfg(feature = "gif")]
    pub(crate) safe fn round(x: f64) -> f64;
    #[cfg(feature = "jpeg")]
    pub(crate) safe fn roundf(x: f32) -> f32;
    #[cfg(feature = "jpeg")]
    pub(crate) safe fn truncf(x: f32) -> f32;
}

#[cfg(feature = "std")]
pub(crate) fn exp(x: f64) -> f64 {
    x.exp()
}

#[cfg(feature = "std")]
pub(crate) fn pow(x: f64, y: f64) -> f64 {
    x.powf(y)
}

#[cfg(all(feature = "std", feature = "gif"))]
pub(crate) const fn round(x: f64) -> f64 {
    x.round()
}

#[cfg(all(feature = "std", feature = "jpeg"))]
pub(crate) const fn roundf(x: f32) -> f32 {
    x.round()
}

#[cfg(all(feature = "std", feature = "jpeg"))]
pub(crate) const fn truncf(x: f32) -> f32 {
    x.trunc()
}
