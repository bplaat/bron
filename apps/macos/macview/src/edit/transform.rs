/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Geometry transforms: rotating, flipping, cropping and resizing.

use super::{Canvas, Mask, Selection, SelectionShape, par_bands};

// Builds a canvas of the given size where each pixel is copied from the source pixel that
// `source` maps its coordinates to.
fn remap(
    canvas: &Canvas,
    width: u32,
    height: u32,
    source: impl Fn(u32, u32) -> (u32, u32) + Sync,
) -> Canvas {
    let mut pixels = vec![0; width as usize * height as usize * 4];
    par_bands(&mut pixels, width as usize * 4, |first_row, band| {
        for (index, pixel) in band.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let x = index as u32 % width;
            let y = first_row as u32 + index as u32 / width;
            let (sx, sy) = source(x, y);
            let offset = (sy as usize * canvas.width as usize + sx as usize) * 4;
            pixel.copy_from_slice(&canvas.pixels[offset..offset + 4]);
        }
    });
    Canvas::new(width, height, pixels)
}

/// Rotates clockwise by `quarter_turns` * 90 degrees (1 = right, 2 = 180, 3 = left).
pub(crate) fn rotate_quarter(canvas: &Canvas, quarter_turns: u8) -> Canvas {
    let (width, height) = (canvas.width, canvas.height);
    match quarter_turns % 4 {
        1 => remap(canvas, height, width, |x, y| (y, height - 1 - x)),
        2 => remap(canvas, width, height, |x, y| {
            (width - 1 - x, height - 1 - y)
        }),
        3 => remap(canvas, height, width, |x, y| (width - 1 - y, x)),
        _ => canvas.clone(),
    }
}

/// Mirrors the canvas left to right.
pub(crate) fn flip_horizontal(canvas: &Canvas) -> Canvas {
    let width = canvas.width;
    remap(canvas, width, canvas.height, |x, y| (width - 1 - x, y))
}

/// Mirrors the canvas top to bottom.
pub(crate) fn flip_vertical(canvas: &Canvas) -> Canvas {
    let height = canvas.height;
    remap(canvas, canvas.width, height, |x, y| (x, height - 1 - y))
}

/// Crops to the selection's pixel bounds (rounded outward, clamped, at least 1px). For an ellipse
/// (or any feathered selection) the pixels outside the shape become transparent using the soft
/// mask.
pub(crate) fn crop(canvas: &Canvas, selection: &Selection) -> Canvas {
    let edge = |start: f64, size: f64, limit: u32| {
        let first = (start.floor().max(0.0) as u32).min(limit - 1);
        let last = ((start + size).ceil().min(f64::from(limit)) as u32).max(first + 1);
        (first, last - first)
    };
    let (x, width) = edge(selection.x, selection.width, canvas.width);
    let (y, height) = edge(selection.y, selection.height, canvas.height);
    let mut cropped = canvas.region(x, y, width, height);
    if selection.shape == SelectionShape::Ellipse || selection.feather > 0.0 {
        let mask = Mask::new(selection, canvas.width, canvas.height);
        for (index, pixel) in cropped.pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let (px, py) = (x + index as u32 % width, y + index as u32 / width);
            let coverage = mask.as_ref().map_or(0.0, |mask| mask.at(px, py));
            pixel[3] = (f32::from(pixel[3]) * coverage).round() as u8;
        }
    }
    cropped
}

/// Rotates clockwise by an arbitrary angle with bilinear sampling. When `crop_to_fit` is false,
/// the canvas grows to hold the whole rotated image and the corners are transparent. When true
/// (straighten), the result is the largest centered rectangle with the original aspect ratio that
/// lies entirely inside the rotated image.
pub(crate) fn rotate(canvas: &Canvas, degrees: f64, crop_to_fit: bool) -> Canvas {
    let (sin, cos) = degrees.to_radians().sin_cos();
    let (w, h) = (f64::from(canvas.width), f64::from(canvas.height));
    let (c, s) = (cos.abs(), sin.abs());
    let (width, height) = if crop_to_fit {
        let k = (w / (w * c + h * s)).min(h / (w * s + h * c));
        ((k * w).round(), (k * h).round())
    } else {
        ((w * c + h * s - 1e-6).ceil(), (w * s + h * c - 1e-6).ceil())
    };
    let (width, height) = (width.max(1.0) as u32, height.max(1.0) as u32);

    let plane = canvas.premultiplied();
    let (source_width, source_height) = (canvas.width as i64, canvas.height as i64);
    // Returns the premultiplied pixel at a source coordinate, extending or transparent outside.
    let fetch = |x: i64, y: i64| -> [f32; 4] {
        let inside = (0..source_width).contains(&x) && (0..source_height).contains(&y);
        if !inside && !crop_to_fit {
            return [0.0; 4];
        }
        let x = x.clamp(0, source_width - 1) as usize;
        let y = y.clamp(0, source_height - 1) as usize;
        let offset = (y * canvas.width as usize + x) * 4;
        std::array::from_fn(|channel| f32::from(plane[offset + channel]))
    };

    let mut output = vec![0u16; width as usize * height as usize * 4];
    par_bands(&mut output, width as usize * 4, |first_row, band| {
        for (index, pixel) in band.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let dx = (index % width as usize) as f64 + 0.5 - f64::from(width) / 2.0;
            let dy = (first_row + index / width as usize) as f64 + 0.5 - f64::from(height) / 2.0;
            let sx = cos * dx + sin * dy + w / 2.0 - 0.5;
            let sy = -sin * dx + cos * dy + h / 2.0 - 0.5;
            let (x0, y0) = (sx.floor(), sy.floor());
            let (tx, ty) = ((sx - x0) as f32, (sy - y0) as f32);
            let (x0, y0) = (x0 as i64, y0 as i64);
            let taps = [
                (fetch(x0, y0), (1.0 - tx) * (1.0 - ty)),
                (fetch(x0 + 1, y0), tx * (1.0 - ty)),
                (fetch(x0, y0 + 1), (1.0 - tx) * ty),
                (fetch(x0 + 1, y0 + 1), tx * ty),
            ];
            for (channel, value) in pixel.iter_mut().enumerate() {
                let sum: f32 = taps.iter().map(|(tap, weight)| tap[channel] * weight).sum();
                *value = sum.round().clamp(0.0, 65025.0) as u16;
            }
        }
    });
    Canvas::from_premultiplied(width, height, &output)
}

// The source span and normalized weights that make up one resampled output coordinate.
struct Taps {
    start: usize,
    weights: Vec<f32>,
}

// Computes the resampling taps from `source` to `target` samples: an area filter when shrinking
// and linear interpolation when enlarging.
fn taps(source: usize, target: usize) -> Vec<Taps> {
    let scale = target as f64 / source as f64;
    (0..target)
        .map(|index| {
            if scale < 1.0 {
                let low = index as f64 / scale;
                let high = ((index + 1) as f64 / scale).min(source as f64);
                let start = low.floor() as usize;
                let end = (high.ceil() as usize).clamp(start + 1, source);
                let weights = (start..end)
                    .map(|j| {
                        (high.min(j as f64 + 1.0) - low.max(j as f64)) as f32 / (high - low) as f32
                    })
                    .collect();
                Taps { start, weights }
            } else {
                let center = ((index as f64 + 0.5) / scale - 0.5).clamp(0.0, (source - 1) as f64);
                let start = (center.floor() as usize).min(source.saturating_sub(2));
                let t = (center - start as f64) as f32;
                let weights = if source == 1 {
                    vec![1.0]
                } else {
                    vec![1.0 - t, t]
                };
                Taps { start, weights }
            }
        })
        .collect()
}

/// High-quality resample to a new size (separable: an area filter when shrinking, linear
/// interpolation when enlarging) on premultiplied channels.
pub(crate) fn resize(canvas: &Canvas, width: u32, height: u32) -> Canvas {
    assert!(width > 0 && height > 0, "canvas must not be empty");
    if (width, height) == (canvas.width, canvas.height) {
        return canvas.clone();
    }
    let (source_width, source_height) = (canvas.width as usize, canvas.height as usize);
    let (width, height) = (width as usize, height as usize);
    let plane = canvas.premultiplied();

    // Resample each row to the new width
    let horizontal = if width == source_width {
        plane
    } else {
        let columns = taps(source_width, width);
        let mut output = vec![0u16; width * source_height * 4];
        par_bands(&mut output, width * 4, |first_row, band| {
            for (out, row) in band
                .chunks_exact_mut(width * 4)
                .zip(plane[first_row * source_width * 4..].chunks_exact(source_width * 4))
            {
                for (pixel, column) in out.as_chunks_mut::<4>().0.iter_mut().zip(&columns) {
                    let mut sum = [0f32; 4];
                    for (source, &weight) in row[column.start * 4..]
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .zip(&column.weights)
                    {
                        for (sum, &value) in sum.iter_mut().zip(source) {
                            *sum += f32::from(value) * weight;
                        }
                    }
                    for (value, sum) in pixel.iter_mut().zip(sum) {
                        *value = sum.round() as u16;
                    }
                }
            }
        });
        output
    };

    // Resample the columns to the new height, accumulating whole rows
    let output = if height == source_height {
        horizontal
    } else {
        let rows = taps(source_height, height);
        let stride = width * 4;
        let mut output = vec![0u16; stride * height];
        par_bands(&mut output, stride, |first_row, band| {
            let mut sums = vec![0f32; stride];
            for (out, row) in band.chunks_exact_mut(stride).zip(&rows[first_row..]) {
                sums.fill(0.0);
                for (source, &weight) in horizontal[row.start * stride..]
                    .chunks_exact(stride)
                    .zip(&row.weights)
                {
                    for (sum, &value) in sums.iter_mut().zip(source) {
                        *sum += f32::from(value) * weight;
                    }
                }
                for (value, &sum) in out.iter_mut().zip(&sums) {
                    *value = sum.round() as u16;
                }
            }
        });
        output
    };
    Canvas::from_premultiplied(width as u32, height as u32, &output)
}

/// Changes the canvas size keeping the image centered; new area is transparent, overflowing
/// parts are cut off.
pub(crate) fn resize_canvas(canvas: &Canvas, width: u32, height: u32) -> Canvas {
    let mut pixels = vec![0; width as usize * height as usize * 4];
    let offset_x = (i64::from(width) - i64::from(canvas.width)) / 2;
    let offset_y = (i64::from(height) - i64::from(canvas.height)) / 2;
    // The copied span along one axis: (source start, target start, length).
    let span = |offset: i64, source: u32, target: u32| {
        let source_start = (-offset).max(0);
        let target_start = offset.max(0);
        let length = (i64::from(source) - source_start).min(i64::from(target) - target_start);
        (
            source_start as usize,
            target_start as usize,
            length.max(0) as usize,
        )
    };
    let (source_x, target_x, columns) = span(offset_x, canvas.width, width);
    let (source_y, target_y, rows) = span(offset_y, canvas.height, height);
    let (source_stride, target_stride) = (canvas.width as usize * 4, width as usize * 4);
    for row in 0..rows {
        let source = (source_y + row) * source_stride + source_x * 4;
        let target = (target_y + row) * target_stride + target_x * 4;
        pixels[target..target + columns * 4]
            .copy_from_slice(&canvas.pixels[source..source + columns * 4]);
    }
    Canvas::new(width, height, pixels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::tests::solid;

    // Builds a canvas whose pixel at (x, y) is [x, y, index, 255].
    fn labeled(width: u32, height: u32) -> Canvas {
        let pixels = (0..width * height)
            .flat_map(|i| [(i % width) as u8, (i / width) as u8, i as u8, 255])
            .collect();
        Canvas::new(width, height, pixels)
    }

    // Returns the pixel at (x, y).
    fn at(canvas: &Canvas, x: u32, y: u32) -> [u8; 4] {
        let offset = ((y * canvas.width + x) * 4) as usize;
        canvas.pixels[offset..offset + 4].try_into().unwrap()
    }

    #[test]
    fn rotate_quarter_turns() {
        let canvas = labeled(2, 3);
        let right = rotate_quarter(&canvas, 1);
        assert_eq!((right.width, right.height), (3, 2));
        // The bottom-left pixel moves to the top-left, the top-left to the top-right
        assert_eq!(at(&right, 0, 0), at(&canvas, 0, 2));
        assert_eq!(at(&right, 2, 0), at(&canvas, 0, 0));
        assert_eq!(at(&right, 2, 1), at(&canvas, 1, 0));
        let mut turned = canvas.clone();
        for _ in 0..4 {
            turned = rotate_quarter(&turned, 1);
        }
        assert_eq!(turned, canvas);
        assert_eq!(rotate_quarter(&rotate_quarter(&canvas, 1), 3), canvas);
        assert_eq!(
            rotate_quarter(&canvas, 2),
            flip_vertical(&flip_horizontal(&canvas))
        );
    }

    #[test]
    fn flips() {
        let canvas = labeled(3, 2);
        let horizontal = flip_horizontal(&canvas);
        assert_eq!(at(&horizontal, 0, 0), at(&canvas, 2, 0));
        assert_eq!(at(&horizontal, 2, 1), at(&canvas, 0, 1));
        let vertical = flip_vertical(&canvas);
        assert_eq!(at(&vertical, 1, 0), at(&canvas, 1, 1));
        assert_eq!(flip_horizontal(&horizontal), canvas);
    }

    #[test]
    fn crop_rectangle() {
        let canvas = labeled(5, 4);
        let selection = Selection {
            shape: SelectionShape::Rectangle,
            x: 1.2,
            y: 1.0,
            width: 2.5,
            height: 2.0,
            feather: 0.0,
        };
        let cropped = crop(&canvas, &selection);
        assert_eq!((cropped.width, cropped.height), (3, 2));
        assert_eq!(at(&cropped, 0, 0), at(&canvas, 1, 1));
        assert_eq!(at(&cropped, 2, 1), at(&canvas, 3, 2));
    }

    #[test]
    fn crop_ellipse() {
        let canvas = solid(30, 30, [9, 9, 9, 255]);
        let selection = Selection {
            shape: SelectionShape::Ellipse,
            x: 5.0,
            y: 5.0,
            width: 20.0,
            height: 20.0,
            feather: 0.0,
        };
        let cropped = crop(&canvas, &selection);
        assert_eq!((cropped.width, cropped.height), (20, 20));
        assert_eq!(at(&cropped, 0, 0)[3], 0);
        assert_eq!(at(&cropped, 19, 19)[3], 0);
        assert_eq!(at(&cropped, 10, 10), [9, 9, 9, 255]);
    }

    #[test]
    fn rotate_matches_quarter() {
        let canvas = labeled(3, 2);
        let rotated = rotate(&canvas, 90.0, false);
        let quarter = rotate_quarter(&canvas, 1);
        assert_eq!(
            (rotated.width, rotated.height),
            (quarter.width, quarter.height)
        );
        for (a, b) in rotated.pixels.iter().zip(&quarter.pixels) {
            assert!(a.abs_diff(*b) <= 1);
        }
        let grown = rotate(&solid(10, 10, [1, 2, 3, 255]), 45.0, false);
        assert_eq!((grown.width, grown.height), (15, 15));
        assert_eq!(at(&grown, 0, 0)[3], 0);
        assert_eq!(at(&grown, 7, 7), [1, 2, 3, 255]);
    }

    #[test]
    fn straighten_size() {
        let canvas = solid(40, 20, [5, 6, 7, 255]);
        let straight = rotate(&canvas, 10.0, true);
        let (c, s) = (10f64.to_radians().cos(), 10f64.to_radians().sin());
        let k = (40.0 / (40.0 * c + 20.0 * s)).min(20.0 / (40.0 * s + 20.0 * c));
        assert_eq!(straight.width, (k * 40.0).round() as u32);
        assert_eq!(straight.height, (k * 20.0).round() as u32);
        assert!(
            straight
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|&p| p == [5, 6, 7, 255])
        );
    }

    #[test]
    fn resize_averages() {
        let pixels = (0..16)
            .flat_map(|i| {
                let value = if (i % 4 + i / 4) % 2 == 0 { 0 } else { 200 };
                [value, value, value, 255]
            })
            .collect();
        let resized = resize(&Canvas::new(4, 4, pixels), 2, 2);
        assert_eq!((resized.width, resized.height), (2, 2));
        assert!(
            resized
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|&p| p == [100, 100, 100, 255])
        );
        let enlarged = resize(&solid(3, 2, [8, 16, 32, 128]), 7, 5);
        assert!(
            enlarged
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|&p| p == [8, 16, 32, 128])
        );
    }

    #[test]
    fn resize_canvas_centers() {
        let canvas = labeled(2, 2);
        let grown = resize_canvas(&canvas, 4, 4);
        assert_eq!(at(&grown, 0, 0), [0; 4]);
        assert_eq!(at(&grown, 1, 1), at(&canvas, 0, 0));
        assert_eq!(at(&grown, 2, 2), at(&canvas, 1, 1));
        let shrunk = resize_canvas(&labeled(5, 3), 3, 1);
        assert_eq!((shrunk.width, shrunk.height), (3, 1));
        assert_eq!(at(&shrunk, 0, 0), at(&labeled(5, 3), 1, 1));
    }
}
