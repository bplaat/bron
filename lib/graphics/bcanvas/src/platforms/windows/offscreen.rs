/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use crate::copy_bgra_premultiplied_to_rgba;
use crate::platforms::windows::dib::Dib;
#[cfg(any(target_vendor = "winxp", target_vendor = "win7", test))]
use crate::platforms::windows::gdi;
use crate::platforms::windows::{Backend, PlatformCanvasContext, backend, direct2d};

enum Target {
    Direct2D(direct2d::OffscreenTarget),
    #[cfg(any(target_vendor = "winxp", target_vendor = "win7", test))]
    Gdi(gdi::OffscreenTarget),
}

pub(crate) struct PlatformOffscreenCanvas {
    // Dropped before the bitmap it draws into.
    target: Target,
    dib: Dib,
    output: Vec<u8>,
    dirty: bool,
}

// SAFETY: The single-threaded Direct2D factory and its objects may be used from any thread
// when access is serialized, as are the shared DirectWrite factory, GDI+ objects and the GDI
// bitmap. The shared resources only leave the canvas in the contexts of `draw`, which
// borrows it mutably.
unsafe impl Send for PlatformOffscreenCanvas {}

impl PlatformOffscreenCanvas {
    pub(crate) fn new(width: u32, height: u32, length: usize) -> Self {
        let dib = Dib::new(width, height).expect("Could not create bitmap storage");
        debug_assert_eq!(dib.len(), length);
        let target = match backend() {
            Backend::Direct2D => Target::Direct2D(direct2d::OffscreenTarget::new(&dib)),
            #[cfg(any(target_vendor = "winxp", target_vendor = "win7", test))]
            Backend::Gdi => Target::Gdi(gdi::OffscreenTarget::new(&dib)),
        };
        Self {
            target,
            dib,
            output: vec![0; length],
            dirty: false,
        }
    }

    pub(crate) fn begin_draw(&mut self, scale: f32) -> PlatformCanvasContext {
        match &mut self.target {
            Target::Direct2D(target) => {
                PlatformCanvasContext::Direct2D(Box::new(target.begin(scale)))
            }
            #[cfg(any(target_vendor = "winxp", target_vendor = "win7", test))]
            Target::Gdi(target) => PlatformCanvasContext::Gdi(Box::new(target.begin(scale))),
        }
    }

    pub(crate) fn end_draw(&mut self) {
        match &mut self.target {
            Target::Direct2D(target) => target.end(),
            #[cfg(any(target_vendor = "winxp", target_vendor = "win7", test))]
            Target::Gdi(_) => {}
        }
        self.dirty = true;
    }

    pub(crate) fn pixels(&mut self) -> &[u8] {
        if std::mem::take(&mut self.dirty) {
            copy_bgra_premultiplied_to_rgba(self.dib.pixels(), &mut self.output);
        }
        &self.output
    }
}
