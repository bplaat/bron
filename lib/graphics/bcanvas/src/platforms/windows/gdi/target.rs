/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::rc::Rc;

use super::context::{Context, Resources, Surface};
use crate::platforms::windows::dib::Dib;
use crate::platforms::windows::headers::*;

/// A back buffer of the client area of a window, copied to the window after each frame.
pub(crate) struct WindowTarget {
    resources: Rc<Resources>,
    back: Option<Dib>,
}

impl WindowTarget {
    pub(crate) fn new() -> Self {
        Self {
            resources: Resources::new(),
            back: None,
        }
    }

    /// Begin drawing the back buffer, which keeps the previous frame like a swap chain that
    /// is not cleared, or `None` when GDI cannot allocate it.
    pub(crate) fn begin(
        &mut self,
        _hwnd: HWND,
        width: u32,
        height: u32,
        scale: f32,
    ) -> Option<Context> {
        if self
            .back
            .as_ref()
            .is_none_or(|back| (back.width, back.height) != (width, height))
        {
            self.back = None;
            self.back = Some(Dib::new(width, height)?);
        }
        let back = self.back.as_ref()?;
        // The back buffer outlives the context, which `end` requires to be dropped.
        let surface = unsafe { Surface::new(back.as_mut_ptr(), width, height, back.dc) }?;
        Some(Context::new(surface, self.resources.clone(), scale))
    }

    /// Copy the back buffer to the window, after the frame context is dropped.
    pub(crate) fn end(&mut self, hwnd: HWND) -> bool {
        let Some(back) = &self.back else {
            return true;
        };
        unsafe {
            GdiFlush();
            let dc = GetDC(hwnd);
            if dc.is_null() {
                return false;
            }
            let copied = BitBlt(
                dc,
                0,
                0,
                back.width as i32,
                back.height as i32,
                back.dc,
                0,
                0,
                SRCCOPY,
            );
            ReleaseDC(hwnd, dc);
            copied != FALSE
        }
    }

    /// Forget the back buffer, the next frame creates a new one.
    pub(crate) fn lose(&mut self) {
        self.back = None;
    }
}

/// GDI+ drawing into the pixels of a bitmap.
pub(crate) struct OffscreenTarget {
    resources: Rc<Resources>,
    pixels: *mut u8,
    width: u32,
    height: u32,
    dc: HDC,
}

impl OffscreenTarget {
    /// The bitmap must outlive the target.
    pub(crate) fn new(dib: &Dib) -> Self {
        Self {
            resources: Resources::new(),
            pixels: dib.as_mut_ptr(),
            width: dib.width,
            height: dib.height,
            dc: dib.dc,
        }
    }

    pub(crate) fn begin(&mut self, scale: f32) -> Context {
        let surface = unsafe { Surface::new(self.pixels, self.width, self.height, self.dc) }
            .expect("GDI+ bitmap surface");
        Context::new(surface, self.resources.clone(), scale)
    }
}
