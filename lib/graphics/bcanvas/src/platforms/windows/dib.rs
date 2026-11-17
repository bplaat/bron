/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ptr::{NonNull, null_mut};

use crate::platforms::windows::headers::*;

/// A top-down 32-bit premultiplied BGRA GDI bitmap selected into its own memory device
/// context, whose pixels both backends draw into.
pub(crate) struct Dib {
    pub(crate) dc: HDC,
    pub(crate) width: u32,
    pub(crate) height: u32,
    bitmap: HANDLE,
    previous: HANDLE,
    storage: NonNull<u8>,
}

impl Dib {
    /// A transparent bitmap, `None` when GDI cannot allocate it.
    pub(crate) fn new(width: u32, height: u32) -> Option<Self> {
        let length = width.checked_mul(height)?.checked_mul(4)?;
        unsafe {
            let dc = CreateCompatibleDC(null_mut());
            if dc.is_null() {
                return None;
            }
            let info = BITMAPINFO {
                header: BITMAPINFOHEADER {
                    size: size_of::<BITMAPINFOHEADER>() as u32,
                    width: width as i32,
                    height: -(height as i32),
                    planes: 1,
                    bit_count: 32,
                    size_image: length,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = null_mut();
            let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            let Some(storage) = NonNull::new(bits.cast()).filter(|_| !bitmap.is_null()) else {
                DeleteDC(dc);
                return None;
            };
            let previous = SelectObject(dc, bitmap);
            Some(Self {
                dc,
                width,
                height,
                bitmap,
                previous,
                storage,
            })
        }
    }

    pub(crate) const fn len(&self) -> usize {
        self.width as usize * self.height as usize * 4
    }

    #[cfg(any(target_vendor = "winxp", target_vendor = "win7", test))]
    pub(crate) const fn as_mut_ptr(&self) -> *mut u8 {
        self.storage.as_ptr()
    }

    /// The pixels, after batched GDI drawing has finished.
    pub(crate) fn pixels(&self) -> &[u8] {
        unsafe {
            GdiFlush();
            std::slice::from_raw_parts(self.storage.as_ptr(), self.len())
        }
    }
}

impl Drop for Dib {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}
