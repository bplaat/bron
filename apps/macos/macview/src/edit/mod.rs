/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Pure Rust image editing: adjustments, effects, selections and geometry transforms.

use std::thread;

pub(crate) use adjust::{Adjustments, Preset, apply};
pub(crate) use transform::{
    crop, flip_horizontal, flip_vertical, resize, resize_canvas, rotate, rotate_quarter,
};

mod adjust;
mod blur;
mod transform;

// Below this many elements work runs on the calling thread only.
const PARALLEL_THRESHOLD: usize = 1 << 16;

/// An editable straight-alpha RGBA8 sRGB bitmap.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Canvas {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: Vec<u8>,
}

impl Canvas {
    /// Creates a canvas from straight-alpha RGBA8 pixels.
    pub(crate) fn new(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        assert!(width > 0 && height > 0, "canvas must not be empty");
        assert_eq!(pixels.len(), width as usize * height as usize * 4);
        Self {
            width,
            height,
            pixels,
        }
    }

    /// Returns whether every pixel is fully opaque.
    pub(crate) fn is_opaque(&self) -> bool {
        self.pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 255)
    }

    /// Composites onto an opaque background color, used before saving formats without alpha (JPEG).
    pub(crate) fn flattened(&self, background: [u8; 3]) -> Canvas {
        let mut pixels = self.pixels.clone();
        for pixel in pixels.as_chunks_mut::<4>().0 {
            let alpha = u32::from(pixel[3]);
            for (channel, back) in pixel.iter_mut().zip(background) {
                *channel = ((u32::from(*channel) * alpha + u32::from(back) * (255 - alpha) + 127)
                    / 255) as u8;
            }
            pixel[3] = 255;
        }
        Canvas::new(self.width, self.height, pixels)
    }

    /// Returns a high-quality downscaled copy whose longest side is at most `max_side`, or a clone
    /// when it already fits. Also returns the scale factor (proxy / full, <= 1).
    pub(crate) fn proxy(&self, max_side: u32) -> (Canvas, f64) {
        let longest = self.width.max(self.height);
        if longest <= max_side {
            return (self.clone(), 1.0);
        }
        let factor = f64::from(max_side.max(1)) / f64::from(longest);
        let size = |side: u32| ((f64::from(side) * factor).round() as u32).max(1);
        (resize(self, size(self.width), size(self.height)), factor)
    }

    // Copies the given pixel rectangle, which must lie inside the canvas.
    fn region(&self, x: u32, y: u32, width: u32, height: u32) -> Canvas {
        let stride = self.width as usize * 4;
        let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
        for row in self
            .pixels
            .chunks_exact(stride)
            .skip(y as usize)
            .take(height as usize)
        {
            pixels.extend_from_slice(&row[x as usize * 4..(x + width) as usize * 4]);
        }
        Canvas::new(width, height, pixels)
    }

    // Converts to premultiplied 16-bit channels: colors as `color * alpha`, alpha as `alpha * 255`.
    fn premultiplied(&self) -> Vec<u16> {
        let mut plane = Vec::with_capacity(self.pixels.len());
        for pixel in self.pixels.as_chunks::<4>().0 {
            let alpha = u16::from(pixel[3]);
            plane.extend_from_slice(&[
                u16::from(pixel[0]) * alpha,
                u16::from(pixel[1]) * alpha,
                u16::from(pixel[2]) * alpha,
                alpha * 255,
            ]);
        }
        plane
    }

    // Converts premultiplied 16-bit channels back to a straight-alpha canvas.
    fn from_premultiplied(width: u32, height: u32, plane: &[u16]) -> Canvas {
        let mut pixels = vec![0; plane.len()];
        par_bands(&mut pixels, width as usize * 4, |first_row, band| {
            let start = first_row * width as usize * 4;
            for (pixel, source) in band
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(plane[start..].as_chunks::<4>().0.iter())
            {
                let alpha = u32::from(source[3]).min(65025);
                if alpha == 0 {
                    continue;
                }
                for (channel, &value) in pixel.iter_mut().zip(&source[..3]) {
                    *channel = ((u32::from(value) * 255 + alpha / 2) / alpha).min(255) as u8;
                }
                pixel[3] = ((alpha + 127) / 255) as u8;
            }
        });
        Canvas::new(width, height, pixels)
    }
}

// Runs `f` on bands of whole rows in parallel, passing each band's first row index.
fn par_bands<T: Send>(data: &mut [T], row_len: usize, f: impl Fn(usize, &mut [T]) + Sync) {
    let rows = data.len() / row_len;
    let threads = if data.len() < PARALLEL_THRESHOLD {
        1
    } else {
        thread::available_parallelism().map_or(1, |threads| threads.get())
    };
    let band_rows = rows.div_ceil(threads.min(rows).max(1)).max(1);
    if band_rows >= rows {
        f(0, data);
        return;
    }
    thread::scope(|scope| {
        for (index, band) in data.chunks_mut(band_rows * row_len).enumerate() {
            let f = &f;
            scope.spawn(move || f(index * band_rows, band));
        }
    });
}

// Smooth Hermite interpolation from 0 at `edge0` to 1 at `edge1`.
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The shape of a selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SelectionShape {
    /// An axis-aligned rectangle.
    Rectangle,
    /// An axis-aligned ellipse inscribed in the selection bounds.
    Ellipse,
}

/// A selected region in canvas pixel coordinates (top-left origin), with an optional soft edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Selection {
    pub(crate) shape: SelectionShape,
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
    pub(crate) feather: f64,
}

impl Selection {
    /// Scales position, size and feather, used to map a selection onto a proxy canvas.
    pub(crate) fn scaled(&self, factor: f64) -> Selection {
        Selection {
            shape: self.shape,
            x: self.x * factor,
            y: self.y * factor,
            width: self.width * factor,
            height: self.height * factor,
            feather: self.feather * factor,
        }
    }

    // Returns the signed distance in pixels from a point to the shape edge, positive inside.
    fn distance(&self, px: f64, py: f64) -> f64 {
        match self.shape {
            SelectionShape::Rectangle => (px - self.x)
                .min(self.x + self.width - px)
                .min(py - self.y)
                .min(self.y + self.height - py),
            SelectionShape::Ellipse => {
                let (rx, ry) = (self.width / 2.0, self.height / 2.0);
                let dx = (px - self.x - rx) / rx;
                let dy = (py - self.y - ry) / ry;
                (1.0 - dx.hypot(dy)) * rx.min(ry)
            }
        }
    }
}

// A soft selection coverage (0..=1) over a pixel rectangle of the canvas; zero elsewhere.
struct Mask {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    values: Vec<f32>,
}

impl Mask {
    // Rasterizes a selection for a canvas size, or None when it does not cover any pixel. The
    // edge transition is `feather` wide (at least 1px for antialiasing), centered on the edge.
    fn new(selection: &Selection, canvas_width: u32, canvas_height: u32) -> Option<Mask> {
        if selection.width <= 0.0 || selection.height <= 0.0 {
            return None;
        }
        let soft = selection.feather.max(1.0);
        let left = (selection.x - soft / 2.0).floor().max(0.0) as u32;
        let top = (selection.y - soft / 2.0).floor().max(0.0) as u32;
        let right = ((selection.x + selection.width + soft / 2.0).ceil())
            .min(f64::from(canvas_width))
            .max(0.0) as u32;
        let bottom = ((selection.y + selection.height + soft / 2.0).ceil())
            .min(f64::from(canvas_height))
            .max(0.0) as u32;
        if right <= left || bottom <= top {
            return None;
        }
        let (width, height) = (right - left, bottom - top);
        let mut values = Vec::with_capacity(width as usize * height as usize);
        for y in top..bottom {
            for x in left..right {
                let distance = selection.distance(f64::from(x) + 0.5, f64::from(y) + 0.5);
                values.push(smoothstep(-0.5, 0.5, (distance / soft) as f32));
            }
        }
        Some(Mask {
            x: left,
            y: top,
            width,
            height,
            values,
        })
    }

    // Returns the coverage at a canvas pixel.
    fn at(&self, x: u32, y: u32) -> f32 {
        if x < self.x || y < self.y || x >= self.x + self.width || y >= self.y + self.height {
            return 0.0;
        }
        self.values[((y - self.y) * self.width + x - self.x) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Builds a canvas filled with one color.
    pub(super) fn solid(width: u32, height: u32, color: [u8; 4]) -> Canvas {
        Canvas::new(width, height, color.repeat((width * height) as usize))
    }

    #[test]
    fn proxy_size_and_scale() {
        let canvas = solid(400, 200, [10, 20, 30, 255]);
        let (proxy, scale) = canvas.proxy(100);
        assert_eq!((proxy.width, proxy.height), (100, 50));
        assert!((scale - 0.25).abs() < 1e-9);
        assert!(
            proxy
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|&p| p == [10, 20, 30, 255])
        );
        let (same, scale) = canvas.proxy(400);
        assert_eq!(same, canvas);
        assert_eq!(scale, 1.0);
    }

    #[test]
    fn flattened_over_white() {
        let canvas = Canvas::new(2, 1, vec![0, 0, 0, 0, 255, 0, 0, 128]);
        let flat = canvas.flattened([255, 255, 255]);
        assert_eq!(flat.pixels, vec![255, 255, 255, 255, 255, 127, 127, 255]);
        assert!(flat.is_opaque());
        assert!(!canvas.is_opaque());
    }

    #[test]
    fn premultiplied_roundtrip() {
        let canvas = Canvas::new(2, 1, vec![200, 100, 50, 255, 30, 60, 90, 77]);
        let plane = canvas.premultiplied();
        assert_eq!(Canvas::from_premultiplied(2, 1, &plane), canvas);
    }

    #[test]
    fn selection_scaled() {
        let selection = Selection {
            shape: SelectionShape::Ellipse,
            x: 1.0,
            y: 2.0,
            width: 3.0,
            height: 4.0,
            feather: 5.0,
        };
        let scaled = selection.scaled(2.0);
        assert_eq!(
            (
                scaled.x,
                scaled.y,
                scaled.width,
                scaled.height,
                scaled.feather
            ),
            (2.0, 4.0, 6.0, 8.0, 10.0)
        );
        assert_eq!(scaled.shape, SelectionShape::Ellipse);
    }

    #[test]
    fn mask_coverage() {
        let selection = Selection {
            shape: SelectionShape::Rectangle,
            x: 2.0,
            y: 2.0,
            width: 4.0,
            height: 4.0,
            feather: 0.0,
        };
        let mask = Mask::new(&selection, 10, 10).unwrap();
        assert_eq!(mask.at(3, 3), 1.0);
        assert_eq!(mask.at(1, 3), 0.0);
        assert_eq!(mask.at(9, 9), 0.0);
    }
}
