/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use super::context::{PlatformCanvasContext, TextStyleCache};
use super::headers::*;
use crate::copy_rgba_premultiplied_to_straight;

pub(crate) struct PlatformOffscreenCanvas {
    context: CGContextRef,
    storage: Vec<u8>,
    output: Vec<u8>,
    dirty: bool,
    text_styles: TextStyleCache,
}

impl PlatformOffscreenCanvas {
    pub(crate) fn new(width: u32, height: u32, length: usize) -> Self {
        let mut storage = vec![0; length];
        unsafe {
            let space = CGColorSpaceCreateDeviceRGB();
            assert!(!space.is_null(), "Could not create bitmap color space");
            let context = CGBitmapContextCreate(
                storage.as_mut_ptr().cast(),
                width as usize,
                height as usize,
                8,
                width as usize * 4,
                space,
                (4 << 12) | 1,
            );
            CGColorSpaceRelease(space);
            assert!(!context.is_null(), "Could not create bitmap canvas");
            CGContextTranslateCTM(context, 0.0, f64::from(height));
            CGContextScaleCTM(context, 1.0, -1.0);
            Self {
                context,
                storage,
                output: vec![0; length],
                dirty: false,
                text_styles: TextStyleCache::default(),
            }
        }
    }

    pub(crate) fn begin_draw(&mut self) -> PlatformCanvasContext {
        self.dirty = true;
        unsafe { PlatformCanvasContext::new(self.context, self.text_styles.clone()) }
    }

    pub(crate) const fn end_draw(&mut self) {}

    pub(crate) fn pixels(&mut self) -> &[u8] {
        if self.dirty {
            self.dirty = false;
            unsafe { CGContextFlush(self.context) };
            copy_rgba_premultiplied_to_straight(&self.storage, &mut self.output);
        }
        &self.output
    }
}

impl Drop for PlatformOffscreenCanvas {
    fn drop(&mut self) {
        unsafe { CGContextRelease(self.context) };
    }
}
