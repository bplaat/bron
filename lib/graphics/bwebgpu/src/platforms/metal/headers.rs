/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Local Metal and QuartzCore ABI declarations.
#![allow(unreachable_pub)]

use std::ffi::c_void;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{Encode, Encoding};

pub(super) type Object = Retained<AnyObject>;
#[link(name = "Metal", kind = "framework")]
unsafe extern "C" {
    pub(super) fn MTLCreateSystemDefaultDevice() -> *mut AnyObject;
}
#[link(name = "System")]
unsafe extern "C" {
    pub(super) fn dispatch_data_create(
        buffer: *const c_void,
        size: usize,
        queue: *mut c_void,
        destructor: *mut c_void,
    ) -> *mut AnyObject;
}
#[cfg(feature = "window")]
#[link(name = "QuartzCore", kind = "framework")]
unsafe extern "C" {}

macro_rules! structure {
    ($name:ident, $encoding:literal, $($field:ident: $ty:ty),+ $(,)?) => {
        #[repr(C)] #[derive(Clone, Copy)] pub(super) struct $name { $(pub(super) $field: $ty),+ }
        unsafe impl Encode for $name { const ENCODING: Encoding = Encoding::Struct($encoding, &[$(<$ty>::ENCODING),+]); }
    };
}
#[cfg(feature = "window")]
structure!(Size, "CGSize", width: f64, height: f64);
#[cfg(feature = "window")]
structure!(Point, "CGPoint", x: f64, y: f64);
#[cfg(feature = "window")]
structure!(Rect, "CGRect", origin: Point, size: Size);
structure!(MSize, "?", width: usize, height: usize, depth: usize);
structure!(MOrigin, "?", x: usize, y: usize, z: usize);
structure!(Viewport, "?", x: f64, y: f64, width: f64, height: f64, near: f64, far: f64);
structure!(Scissor, "?", x: usize, y: usize, width: usize, height: usize);
structure!(Clear, "?", r: f64, g: f64, b: f64, a: f64);
