/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::sync::Arc;

use crate::platforms::windows::direct2d;

/// The premultiplied BGRA pixels of a canvas image, with the native bitmap of a backend.
/// GDI+ wraps the pixels without copying them on every draw, so only Direct2D caches one.
pub(crate) struct PlatformImage {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: Arc<[u8]>,
    pub(crate) direct2d: direct2d::ImageCache,
}

impl PlatformImage {
    pub(crate) const fn new(width: u32, height: u32, pixels: Arc<[u8]>) -> Self {
        Self {
            width,
            height,
            pixels,
            direct2d: direct2d::ImageCache::new(),
        }
    }
}
