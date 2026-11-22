/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use crate::path::Path;
use crate::platforms::windows::direct2d;
#[cfg(any(target_vendor = "winxp", target_vendor = "win7", test))]
use crate::platforms::windows::gdi;
use crate::platforms::windows::image::PlatformImage;
use crate::{CanvasState, FillRule, TextExtents};

/// A drawing context of the backend that the process draws with.
pub(crate) enum PlatformCanvasContext {
    Direct2D(Box<direct2d::Context>),
    #[cfg(any(target_vendor = "winxp", target_vendor = "win7", test))]
    Gdi(Box<gdi::Context>),
}

macro_rules! forward {
    ($(fn $name:ident(&mut self $(, $arg:ident: $ty:ty)*) $(-> $ret:ty)?;)*) => {
        $(
            pub(crate) fn $name(&mut self $(, $arg: $ty)*) $(-> $ret)? {
                match self {
                    Self::Direct2D(context) => context.$name($($arg),*),
                    #[cfg(any(target_vendor = "winxp", target_vendor = "win7", test))]
                    Self::Gdi(context) => context.$name($($arg),*),
                }
            }
        )*
    };
}

impl PlatformCanvasContext {
    forward! {
        fn save(&mut self);
        fn restore(&mut self);
        fn begin_layer(&mut self, state: &CanvasState);
        fn begin_mask_layer(&mut self, state: &CanvasState);
        fn end_mask(&mut self);
        fn end_layer(&mut self);
        fn clear_rect(&mut self, x: f32, y: f32, w: f32, h: f32);
        fn fill_rect(&mut self, state: &CanvasState, x: f32, y: f32, w: f32, h: f32);
        fn stroke_rect(&mut self, state: &CanvasState, x: f32, y: f32, w: f32, h: f32);
        fn fill(&mut self, state: &CanvasState, path: &Path, rule: FillRule);
        fn stroke(&mut self, state: &CanvasState, path: &Path);
        fn draw_image(
            &mut self,
            state: &CanvasState,
            image: &PlatformImage,
            source: [f32; 4],
            dest: [f32; 4]
        );
        fn clip(&mut self, path: &Path, rule: FillRule);
        fn translate(&mut self, x: f32, y: f32);
        fn rotate(&mut self, angle: f32);
        fn scale(&mut self, x: f32, y: f32);
        fn transform(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32);
        fn set_transform(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32);
        fn reset_transform(&mut self);
        fn fill_text(&mut self, state: &CanvasState, text: &str, x: f32, y: f32);
        fn measure_text(&mut self, state: &CanvasState, text: &str) -> TextExtents;
    }
}
