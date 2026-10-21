/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::c_void;
use std::ptr::null_mut;

use objc2::ffi::{objc_autoreleasePoolPop, objc_autoreleasePoolPush};
#[cfg(target_os = "macos")]
use objc2::runtime::{AnyObject as Object, Bool};
#[cfg(target_os = "macos")]
use objc2::{class, msg_send};

use super::context::{PlatformCanvasContext, TextStyleCache};
use super::headers::*;

pub(crate) struct PlatformOffscreenCanvas {
    context: CGContextRef,
    storage: Vec<u8>,
    dirty: bool,
    pool: *mut c_void,
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
                dirty: false,
                pool: null_mut(),
                text_styles: TextStyleCache::default(),
            }
        }
    }

    pub(crate) fn begin_draw(&mut self) -> PlatformCanvasContext {
        // Offscreen drawing can run outside an event loop, so drain its autoreleased text objects.
        self.pool = unsafe { objc_autoreleasePoolPush() };
        self.dirty = true;
        unsafe {
            self.push_graphics_context();
            PlatformCanvasContext::new(self.context, self.text_styles.clone())
        }
    }

    pub(crate) fn end_draw(&mut self) {
        unsafe {
            self.pop_graphics_context();
            objc_autoreleasePoolPop(self.pool);
        }
    }

    #[cfg(target_os = "ios")]
    unsafe fn push_graphics_context(&self) {
        unsafe { UIGraphicsPushContext(self.context) };
    }

    #[cfg(target_os = "ios")]
    unsafe fn pop_graphics_context(&self) {
        unsafe { UIGraphicsPopContext() };
    }

    #[cfg(target_os = "macos")]
    unsafe fn push_graphics_context(&self) {
        unsafe {
            let _: () = msg_send![class!(NSGraphicsContext), saveGraphicsState];
            let graphics: *mut Object = msg_send![class!(NSGraphicsContext),
                graphicsContextWithCGContext:self.context, flipped:Bool::YES];
            assert!(
                !graphics.is_null(),
                "Could not create bitmap graphics context"
            );
            let _: () = msg_send![class!(NSGraphicsContext), setCurrentContext:graphics];
        }
    }

    #[cfg(target_os = "macos")]
    unsafe fn pop_graphics_context(&self) {
        let _: () = unsafe { msg_send![class!(NSGraphicsContext), restoreGraphicsState] };
    }

    pub(crate) fn pixels(&mut self) -> &[u8] {
        if std::mem::take(&mut self.dirty) {
            unsafe { CGContextFlush(self.context) };
        }
        &self.storage
    }
}

impl Drop for PlatformOffscreenCanvas {
    fn drop(&mut self) {
        unsafe { CGContextRelease(self.context) };
    }
}
