/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#[cfg(target_os = "macos")]
use objc2::runtime::{AnyObject as Object, Bool};
#[cfg(target_os = "macos")]
use objc2::{class, msg_send};

use super::context::PlatformCanvasContext;
use super::headers::*;
use crate::copy_rgba_premultiplied_to_straight;

pub(crate) struct PlatformOffscreenCanvas {
    context: CGContextRef,
    storage: Vec<u8>,
    output: Vec<u8>,
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
            }
        }
    }

    #[cfg(target_os = "ios")]
    pub(crate) fn begin_draw(&mut self) -> PlatformCanvasContext {
        unsafe {
            UIGraphicsPushContext(self.context);
            PlatformCanvasContext::new(self.context)
        }
    }

    #[cfg(target_os = "ios")]
    pub(crate) fn end_draw(&mut self) {
        unsafe { UIGraphicsPopContext() };
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn begin_draw(&mut self) -> PlatformCanvasContext {
        unsafe {
            let _: () = msg_send![class!(NSGraphicsContext), saveGraphicsState];
            let graphics: *mut Object = msg_send![class!(NSGraphicsContext),
                graphicsContextWithCGContext:self.context, flipped:Bool::YES];
            assert!(
                !graphics.is_null(),
                "Could not create bitmap graphics context"
            );
            let _: () = msg_send![class!(NSGraphicsContext), setCurrentContext:graphics];
            PlatformCanvasContext::new(self.context)
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn end_draw(&mut self) {
        unsafe {
            let _: () = msg_send![class!(NSGraphicsContext), restoreGraphicsState];
        }
    }

    pub(crate) fn pixels(&mut self) -> &[u8] {
        unsafe { CGContextFlush(self.context) };
        copy_rgba_premultiplied_to_straight(&self.storage, &mut self.output);
        &self.output
    }
}

impl Drop for PlatformOffscreenCanvas {
    fn drop(&mut self) {
        unsafe { CGContextRelease(self.context) };
    }
}
