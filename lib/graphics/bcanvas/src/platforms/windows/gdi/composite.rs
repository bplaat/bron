/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Composite operations in software, which GDI+ lacks apart from source over and copy.
//! Pixels are premultiplied BGRA.

use crate::CompositeOperation;

/// A device rectangle `[x, y, width, height]` in pixels of the canvas.
pub(super) type Rect = [i32; 4];

/// Pixels covering a device rectangle of the canvas.
#[derive(Clone, Copy)]
pub(super) struct Plane<'a> {
    pub(super) pixels: &'a [u8],
    pub(super) rect: Rect,
}

impl Plane<'_> {
    // The pixel at a device position, transparent outside the plane.
    fn at(&self, x: i32, y: i32) -> [u8; 4] {
        let [left, top, width, height] = self.rect;
        if x < left || y < top || x >= left + width || y >= top + height {
            return [0; 4];
        }
        let index = ((y - top) as usize * width as usize + (x - left) as usize) * 4;
        let pixel = &self.pixels[index..index + 4];
        [pixel[0], pixel[1], pixel[2], pixel[3]]
    }
}

/// Pixels that a composite changes.
pub(super) struct Target<'a> {
    pub(super) pixels: &'a mut [u8],
    pub(super) rect: Rect,
}

/// Composites `source` onto `destination` with an operation of the W3C compositing spec,
/// within the device rectangle `area`. `alpha` and the alpha of `mask` scale the source, the
/// alpha of `coverage` limits the change of the destination, like a clip.
pub(super) fn composite(
    operation: CompositeOperation,
    destination: Target<'_>,
    source: Plane<'_>,
    alpha: f32,
    mask: Option<Plane<'_>>,
    coverage: Option<Plane<'_>>,
    area: Rect,
) {
    let alpha = alpha.clamp(0.0, 1.0);
    let preserves = preserves_destination(operation);
    let [left, top, width, height] = destination.rect;
    let x0 = area[0].max(left);
    let y0 = area[1].max(top);
    let x1 = (area[0] + area[2]).min(left + width);
    let y1 = (area[1] + area[3]).min(top + height);
    // Source over at full coverage and alpha only needs integer math.
    let integer = operation == CompositeOperation::SourceOver && alpha == 1.0 && mask.is_none();
    for y in y0..y1 {
        let row = (y - top) as usize * width as usize;
        for x in x0..x1 {
            let index = (row + (x - left) as usize) * 4;
            let limit = coverage.map_or(255, |coverage| coverage.at(x, y)[3]);
            if limit == 0 {
                continue;
            }
            let source = source.at(x, y);
            let pixel = &mut destination.pixels[index..index + 4];
            if integer && limit == 255 {
                if source[3] == 0 {
                    continue;
                }
                let inverse = 255 - u32::from(source[3]);
                for channel in 0..4 {
                    let value = u32::from(source[channel])
                        + (u32::from(pixel[channel]) * inverse + 127) / 255;
                    pixel[channel] = value.min(255) as u8;
                }
                continue;
            }
            let scale = mask.map_or(alpha, |mask| alpha * f32::from(mask.at(x, y)[3]) / 255.0);
            if preserves && (source[3] == 0 || scale == 0.0) {
                continue;
            }
            let limit = f32::from(limit) / 255.0;
            let s = source.map(|channel| f32::from(channel) / 255.0 * scale);
            let d =
                [pixel[0], pixel[1], pixel[2], pixel[3]].map(|channel| f32::from(channel) / 255.0);
            let result = self::pixel(operation, s, d);
            for channel in 0..4 {
                let value = d[channel] + (result[channel] - d[channel]) * limit;
                pixel[channel] = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
    }
}

// Whether a transparent source leaves the destination unchanged.
const fn preserves_destination(operation: CompositeOperation) -> bool {
    use CompositeOperation::*;
    !matches!(
        operation,
        SourceIn | DestinationIn | SourceOut | DestinationAtop | Copy
    )
}

// One premultiplied BGRA pixel.
fn pixel(operation: CompositeOperation, s: [f32; 4], d: [f32; 4]) -> [f32; 4] {
    use CompositeOperation::*;
    let (sa, da) = (s[3], d[3]);
    let porter_duff = |fa: f32, fb: f32| std::array::from_fn(|i| s[i] * fa + d[i] * fb);
    match operation {
        SourceOver => porter_duff(1.0, 1.0 - sa),
        DestinationOver => porter_duff(1.0 - da, 1.0),
        SourceIn => porter_duff(da, 0.0),
        DestinationIn => porter_duff(0.0, sa),
        SourceOut => porter_duff(1.0 - da, 0.0),
        DestinationOut => porter_duff(0.0, 1.0 - sa),
        SourceAtop => porter_duff(da, 1.0 - sa),
        DestinationAtop => porter_duff(1.0 - da, sa),
        Xor => porter_duff(1.0 - da, 1.0 - sa),
        Lighter => std::array::from_fn(|i| (s[i] + d[i]).min(1.0)),
        Copy => s,
        _ => blend(operation, s, d),
    }
}

// A blend mode: co = cs (1 - ab) + cb (1 - as) + as ab B(Cb, Cs).
fn blend(operation: CompositeOperation, s: [f32; 4], d: [f32; 4]) -> [f32; 4] {
    let (sa, da) = (s[3], d[3]);
    if sa == 0.0 {
        return d;
    }
    if da == 0.0 {
        return s;
    }
    // Unpremultiplied RGB, the pixels are BGRA.
    let cs = [s[2] / sa, s[1] / sa, s[0] / sa];
    let cb = [d[2] / da, d[1] / da, d[0] / da];
    let mixed = blend_rgb(operation, cb, cs);
    let rgb = [2, 1, 0];
    let mut out = [0.0; 4];
    for (channel, &index) in rgb.iter().enumerate() {
        out[index] = s[index] * (1.0 - da) + d[index] * (1.0 - sa) + sa * da * mixed[channel];
    }
    out[3] = sa + da - sa * da;
    out
}

fn blend_rgb(operation: CompositeOperation, cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    use CompositeOperation::*;
    let separable = |f: fn(f32, f32) -> f32| std::array::from_fn(|i| f(cb[i], cs[i]));
    match operation {
        Multiply => separable(|b, s| b * s),
        Screen => separable(screen),
        Overlay => separable(|b, s| hard_light(s, b)),
        Darken => separable(f32::min),
        Lighten => separable(f32::max),
        ColorDodge => separable(|b, s| {
            if b == 0.0 {
                0.0
            } else if s >= 1.0 {
                1.0
            } else {
                (b / (1.0 - s)).min(1.0)
            }
        }),
        ColorBurn => separable(|b, s| {
            if b >= 1.0 {
                1.0
            } else if s == 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - b) / s).min(1.0)
            }
        }),
        HardLight => separable(hard_light),
        SoftLight => separable(|b, s| {
            if s <= 0.5 {
                b - (1.0 - 2.0 * s) * b * (1.0 - b)
            } else {
                let d = if b <= 0.25 {
                    ((16.0 * b - 12.0) * b + 4.0) * b
                } else {
                    b.sqrt()
                };
                b + (2.0 * s - 1.0) * (d - b)
            }
        }),
        Difference => separable(|b, s| (b - s).abs()),
        Exclusion => separable(|b, s| b + s - 2.0 * b * s),
        Hue => set_lum(set_sat(cs, sat(cb)), lum(cb)),
        Saturation => set_lum(set_sat(cb, sat(cs)), lum(cb)),
        Color => set_lum(cs, lum(cb)),
        Luminosity => set_lum(cb, lum(cs)),
        _ => cs,
    }
}

fn screen(b: f32, s: f32) -> f32 {
    b + s - b * s
}

fn hard_light(b: f32, s: f32) -> f32 {
    if s <= 0.5 {
        b * 2.0 * s
    } else {
        screen(b, 2.0 * s - 1.0)
    }
}

fn lum([r, g, b]: [f32; 3]) -> f32 {
    0.3 * r + 0.59 * g + 0.11 * b
}

fn clip_color(c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    c.map(|channel| {
        let mut channel = channel;
        if n < 0.0 && l != n {
            channel = l + (channel - l) * l / (l - n);
        }
        if x > 1.0 && x != l {
            channel = l + (channel - l) * (1.0 - l) / (x - l);
        }
        channel
    })
}

fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_color(c.map(|channel| channel + d))
}

fn sat([r, g, b]: [f32; 3]) -> f32 {
    r.max(g).max(b) - r.min(g).min(b)
}

fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let max = c[0].max(c[1]).max(c[2]);
    let min = c[0].min(c[1]).min(c[2]);
    if max <= min {
        return [0.0; 3];
    }
    c.map(|channel| {
        if channel == max {
            s
        } else if channel == min {
            0.0
        } else {
            (channel - min) * s / (max - min)
        }
    })
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    const ONE: Rect = [0, 0, 1, 1];

    fn one(operation: CompositeOperation, source: [u8; 4], destination: [u8; 4]) -> [u8; 4] {
        let mut destination = destination;
        composite(
            operation,
            Target {
                pixels: &mut destination,
                rect: ONE,
            },
            Plane {
                pixels: &source,
                rect: ONE,
            },
            1.0,
            None,
            None,
            ONE,
        );
        destination
    }

    #[test]
    fn porter_duff_operations() {
        let red = [0, 0, 255, 255];
        let blue = [255, 0, 0, 255];
        let clear = [0, 0, 0, 0];
        assert_eq!(one(CompositeOperation::SourceOver, red, blue), red);
        assert_eq!(one(CompositeOperation::DestinationOver, red, blue), blue);
        assert_eq!(one(CompositeOperation::SourceIn, red, clear), clear);
        assert_eq!(one(CompositeOperation::DestinationOut, red, blue), clear);
        assert_eq!(one(CompositeOperation::Copy, clear, blue), clear);
        assert_eq!(
            one(
                CompositeOperation::Lighter,
                [0, 0, 200, 255],
                [0, 0, 100, 255]
            ),
            [0, 0, 255, 255]
        );
    }

    #[test]
    fn blend_modes_mix_opaque_colors() {
        let gray = [128, 128, 128, 255];
        assert_eq!(
            one(CompositeOperation::Multiply, gray, [255, 255, 255, 255]),
            gray
        );
        assert_eq!(one(CompositeOperation::Screen, [0, 0, 0, 255], gray), gray);
        assert_eq!(
            one(CompositeOperation::Difference, gray, gray),
            [0, 0, 0, 255]
        );
        // Luminosity of black onto a color is black.
        assert_eq!(
            one(
                CompositeOperation::Luminosity,
                [0, 0, 0, 255],
                [10, 200, 30, 255]
            ),
            [0, 0, 0, 255]
        );
    }

    #[test]
    fn coverage_limits_the_change() {
        let mut destination = [255, 0, 0, 255, 255, 0, 0, 255];
        let row = [0, 0, 2, 1];
        composite(
            CompositeOperation::Copy,
            Target {
                pixels: &mut destination,
                rect: row,
            },
            Plane {
                pixels: &[0; 8],
                rect: row,
            },
            1.0,
            None,
            Some(Plane {
                pixels: &[0, 0, 0, 255, 0, 0, 0, 0],
                rect: row,
            }),
            row,
        );
        assert_eq!(destination, [0, 0, 0, 0, 255, 0, 0, 255]);
    }

    #[test]
    fn planes_at_other_origins_line_up() {
        // A one pixel source at (1, 0) copies onto the second pixel of the row.
        let mut destination = [9; 8];
        composite(
            CompositeOperation::Copy,
            Target {
                pixels: &mut destination,
                rect: [0, 0, 2, 1],
            },
            Plane {
                pixels: &[1, 2, 3, 255],
                rect: [1, 0, 1, 1],
            },
            1.0,
            None,
            None,
            [1, 0, 1, 1],
        );
        assert_eq!(destination, [9, 9, 9, 9, 1, 2, 3, 255]);
    }
}
