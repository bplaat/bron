/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Gradients painted in software. GDI+ gradient brushes cannot pad a linear gradient or
//! paint a radial gradient outside its circle, so the canvas paints the gradient pixels of
//! the drawn area into a texture instead.

use crate::path::Transform;
use crate::{Gradient, GradientShape, GradientSpread};

const STEPS: usize = 1024;

/// The premultiplied BGRA pixels of a gradient over a device rectangle `[x, y, width,
/// height]`, with `to_device` mapping gradient space to device pixels.
pub(super) fn paint(
    gradient: &Gradient,
    alpha: f32,
    to_device: Transform,
    [left, top, width, height]: [i32; 4],
) -> Option<Vec<u8>> {
    let inverse = to_device.inverse()?;
    let table = color_table(gradient, alpha)?;
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    let (rows, _) = pixels.as_chunks_mut::<4>();
    for y in 0..height {
        for x in 0..width {
            let point = inverse.point([(left + x) as f32 + 0.5, (top + y) as f32 + 0.5]);
            let Some(t) = offset(&gradient.shape, point) else {
                continue;
            };
            let t = match gradient.spread {
                GradientSpread::Pad => t.clamp(0.0, 1.0),
                GradientSpread::Repeat => t - t.floor(),
                GradientSpread::Reflect => {
                    let t = t.rem_euclid(2.0);
                    if t > 1.0 { 2.0 - t } else { t }
                }
            };
            let index = ((t * (STEPS - 1) as f32).round() as usize).min(STEPS - 1);
            rows[(y * width + x) as usize] = table[index];
        }
    }
    Some(pixels)
}

// The gradient offset of a point, `None` where a radial gradient is not painted.
fn offset(shape: &GradientShape, [x, y]: [f32; 2]) -> Option<f32> {
    match *shape {
        GradientShape::Linear { start, end } => {
            let (dx, dy) = (end.0 - start.0, end.1 - start.1);
            let length = dx * dx + dy * dy;
            if length == 0.0 {
                return None;
            }
            Some(((x - start.0) * dx + (y - start.1) * dy) / length)
        }
        GradientShape::Radial {
            focal,
            center,
            radius,
        } => {
            // The circle at t has center focal + t (center - focal) and radius t r, take
            // the largest t with a nonnegative radius whose circle passes through the point.
            if radius <= 0.0 {
                return None;
            }
            let (cx, cy) = (center.0 - focal.0, center.1 - focal.1);
            let (px, py) = (x - focal.0, y - focal.1);
            let a = cx * cx + cy * cy - radius * radius;
            let b = px * cx + py * cy;
            let c = px * px + py * py;
            if a.abs() < 1e-6 {
                return (b != 0.0).then_some(c / (2.0 * b)).filter(|t| *t >= 0.0);
            }
            let discriminant = b * b - a * c;
            if discriminant < 0.0 {
                return None;
            }
            let root = discriminant.sqrt();
            let (t1, t2) = ((b + root) / a, (b - root) / a);
            let t = t1.max(t2);
            if t >= 0.0 {
                Some(t)
            } else {
                let t = t1.min(t2);
                (t >= 0.0).then_some(t)
            }
        }
    }
}

// Premultiplied BGRA colors at evenly spaced offsets, interpolated between the stops.
fn color_table(gradient: &Gradient, alpha: f32) -> Option<Vec<[u8; 4]>> {
    let premultiplied = |color: crate::Color| {
        let (r, g, b, a) = color.components(alpha);
        [b * a, g * a, r * a, a].map(|channel| channel as f32)
    };
    // Each stop premultiplied once, the steps walk through them in order
    let mut stops: Vec<(f32, [f32; 4])> = gradient
        .stops
        .iter()
        .map(|&(offset, color)| (offset, premultiplied(color)))
        .collect();
    stops.sort_by(|a, b| a.0.total_cmp(&b.0));
    let first = *stops.first()?;
    let last = *stops.last()?;
    let mut next = 0;
    Some(
        (0..STEPS)
            .map(|step| {
                let t = step as f32 / (STEPS - 1) as f32;
                let color = if t <= first.0 {
                    first.1
                } else if t >= last.0 {
                    last.1
                } else {
                    while stops[next].0 <= t {
                        next += 1;
                    }
                    let (from, to) = (stops[next - 1], stops[next]);
                    let span = to.0 - from.0;
                    let k = if span > 0.0 { (t - from.0) / span } else { 1.0 };
                    let (a, b) = (from.1, to.1);
                    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * k)
                };
                color.map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8)
            })
            .collect(),
    )
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Color;

    fn gradient(shape: GradientShape, spread: GradientSpread) -> Gradient {
        Gradient {
            shape,
            stops: vec![(0.0, Color::rgb(0, 0, 0)), (1.0, Color::rgb(255, 255, 255))],
            spread,
            transform: Transform::IDENTITY.0,
        }
    }

    #[test]
    fn linear_gradient_pads_and_reflects() {
        let shape = GradientShape::Linear {
            start: (0.0, 0.0),
            end: (2.0, 0.0),
        };
        let pad = paint(
            &gradient(shape, GradientSpread::Pad),
            1.0,
            Transform::IDENTITY,
            [-2, 0, 6, 1],
        )
        .expect("pixels");
        assert_eq!(pad[0], 0);
        assert_eq!(pad[5 * 4], 255);
        let reflect = paint(
            &gradient(shape, GradientSpread::Reflect),
            1.0,
            Transform::IDENTITY,
            [0, 0, 4, 1],
        )
        .expect("pixels");
        // Offsets 0.25, 0.75, 1.25 -> 0.75, 1.75 -> 0.25.
        assert_eq!(reflect[4], reflect[2 * 4]);
        assert_eq!(reflect[0], reflect[3 * 4]);
    }

    #[test]
    fn radial_gradient_pads_outside_its_circle() {
        let shape = GradientShape::Radial {
            focal: (5.0, 5.0),
            center: (5.0, 5.0),
            radius: 2.0,
        };
        let pixels = paint(
            &gradient(shape, GradientSpread::Pad),
            1.0,
            Transform::IDENTITY,
            [0, 0, 10, 10],
        )
        .expect("pixels");
        assert!(pixels[(5 * 10 + 5) * 4] < 100);
        assert_eq!(pixels[0], 255);
        assert_eq!(pixels[3], 255);
    }
}
