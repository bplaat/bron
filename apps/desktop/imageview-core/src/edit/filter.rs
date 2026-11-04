/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use super::picture::{Picture, premultiply, unpremultiply};

/// A slider of a filter with its range and neutral value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Parameter {
    /// The name shown next to the slider.
    pub label: &'static str,
    /// The smallest value.
    pub min: f32,
    /// The largest value.
    pub max: f32,
    /// The value a filter starts with.
    pub default: f32,
    /// The heading of the group of sliders this one starts, or empty.
    pub section: &'static str,
    /// Whether the value is a length in pixels, which scales with the picture.
    pub pixels: bool,
}

const fn parameter(label: &'static str, min: f32, max: f32, default: f32) -> Parameter {
    Parameter {
        label,
        min,
        max,
        default,
        section: "",
        pixels: false,
    }
}

impl Parameter {
    // Starts a group of sliders with a heading.
    const fn starts(mut self, section: &'static str) -> Self {
        self.section = section;
        self
    }

    // Measures the value in pixels.
    pub(super) const fn in_pixels(mut self) -> Self {
        self.pixels = true;
        self
    }
}

/// A change to the colors or sharpness of every pixel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    /// The light and color sliders: exposure, brightness, contrast, highlights, shadows,
    /// vignette, saturation, vibrance, white balance and hue.
    Adjust,
    /// Stretches the tones to the full range.
    AutoEnhance,
    /// Shades of gray by luminance.
    Grayscale,
    /// Warm brown tones like an old photo.
    Sepia,
    /// The opposite colors.
    Invert,
    /// Black or white by luminance.
    Threshold,
    /// Inverts the colors brighter than a level.
    Solarize,
    /// A smooth blur.
    GaussianBlur,
    /// The average of a square around every pixel.
    BoxBlur,
    /// Streaks along a direction, like a moving camera.
    MotionBlur,
    /// Streaks toward the middle, like zooming during the exposure.
    RadialBlur,
    /// Stronger edges with an unsharp mask.
    Sharpen,
    /// Less grain, with the median of the pixels around every pixel.
    ReduceNoise,
    /// Large blocks of averaged color, which hide details like faces.
    Pixelate,
}

const ADJUST: &[Parameter] = &[
    parameter("Exposure", -3.0, 3.0, 0.0).starts("Light"),
    parameter("Brightness", -100.0, 100.0, 0.0),
    parameter("Contrast", -100.0, 100.0, 0.0),
    parameter("Highlights", -100.0, 100.0, 0.0),
    parameter("Shadows", -100.0, 100.0, 0.0),
    parameter("Vignette", -100.0, 100.0, 0.0),
    parameter("Saturation", -100.0, 100.0, 0.0).starts("Color"),
    parameter("Vibrance", -100.0, 100.0, 0.0),
    parameter("Warmth", -100.0, 100.0, 0.0),
    parameter("Tint", -100.0, 100.0, 0.0),
    parameter("Hue", -180.0, 180.0, 0.0),
];
const NOISE: &[Parameter] = &[parameter("Radius", 1.0, 3.0, 1.0).in_pixels()];
const PIXELATE: &[Parameter] = &[parameter("Size", 2.0, 100.0, 12.0).in_pixels()];
const LEVEL: &[Parameter] = &[parameter("Level", 0.0, 255.0, 128.0)];
const RADIUS: &[Parameter] = &[parameter("Radius", 0.0, 100.0, 4.0).in_pixels()];
const MOTION: &[Parameter] = &[
    parameter("Distance", 0.0, 200.0, 20.0).in_pixels(),
    parameter("Angle", -180.0, 180.0, 0.0),
];
const RADIAL: &[Parameter] = &[parameter("Amount", 0.0, 100.0, 20.0)];
const SHARPEN: &[Parameter] = &[
    parameter("Amount", 0.0, 300.0, 100.0),
    parameter("Radius", 0.5, 20.0, 2.0).in_pixels(),
];

// Luminance weights of sRGB.
const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

impl Filter {
    /// The name of the filter in menus and popovers.
    pub const fn title(self) -> &'static str {
        match self {
            Self::Adjust => "Adjust",
            Self::AutoEnhance => "Auto Enhance",
            Self::Grayscale => "Grayscale",
            Self::Sepia => "Sepia",
            Self::Invert => "Invert",
            Self::Threshold => "Threshold",
            Self::Solarize => "Solarize",
            Self::GaussianBlur => "Gaussian Blur",
            Self::BoxBlur => "Box Blur",
            Self::MotionBlur => "Motion Blur",
            Self::RadialBlur => "Radial Blur",
            Self::Sharpen => "Sharpen",
            Self::ReduceNoise => "Reduce Noise",
            Self::Pixelate => "Pixelate",
        }
    }

    /// The sliders of the filter, a filter without them applies right away.
    pub const fn parameters(self) -> &'static [Parameter] {
        match self {
            Self::Adjust => ADJUST,
            Self::AutoEnhance | Self::Grayscale | Self::Sepia | Self::Invert => &[],
            Self::Threshold | Self::Solarize => LEVEL,
            Self::GaussianBlur | Self::BoxBlur => RADIUS,
            Self::MotionBlur => MOTION,
            Self::RadialBlur => RADIAL,
            Self::Sharpen => SHARPEN,
            Self::ReduceNoise => NOISE,
            Self::Pixelate => PIXELATE,
        }
    }

    /// The starting values of the sliders.
    pub fn defaults(self) -> Vec<f32> {
        self.parameters()
            .iter()
            .map(|parameter| parameter.default)
            .collect()
    }

    /// Returns the filtered picture, with a value for every parameter.
    pub fn apply(self, picture: &Picture, values: &[f32]) -> Picture {
        let value = |index: usize| {
            values
                .get(index)
                .copied()
                .unwrap_or(self.parameters()[index].default)
        };
        match self {
            Self::Adjust => adjust(picture, &std::array::from_fn(value)),
            Self::AutoEnhance => auto_enhance(picture, picture),
            Self::Grayscale => {
                map_colors(picture, |[red, green, blue]| [luma(red, green, blue); 3])
            }
            Self::Sepia => map_colors(picture, |rgb| {
                matrix(
                    &[
                        [0.393, 0.769, 0.189],
                        [0.349, 0.686, 0.168],
                        [0.272, 0.534, 0.131],
                    ],
                    rgb,
                )
            }),
            Self::Invert => map_colors(picture, |rgb| rgb.map(|value| 1.0 - value)),
            Self::Threshold => {
                let level = value(0) / 255.0;
                map_colors(picture, |[red, green, blue]| {
                    [if luma(red, green, blue) >= level {
                        1.0
                    } else {
                        0.0
                    }; 3]
                })
            }
            Self::Solarize => {
                let level = value(0) / 255.0;
                map_colors(picture, |rgb| {
                    rgb.map(|value| if value > level { 1.0 - value } else { value })
                })
            }
            Self::GaussianBlur => gaussian_blur(picture, value(0) / 2.0),
            Self::BoxBlur => box_blur(picture, value(0).round() as usize),
            Self::MotionBlur => motion_blur(picture, value(0), value(1).to_radians()),
            Self::RadialBlur => radial_blur(picture, value(0) / 100.0),
            Self::Sharpen => sharpen(picture, value(0) / 100.0, value(1)),
            Self::ReduceNoise => reduce_noise(picture, value(0).round() as usize),
            Self::Pixelate => pixelate(picture, value(0).round().max(1.0) as usize),
        }
    }
}

fn luma(red: f32, green: f32, blue: f32) -> f32 {
    LUMA[0] * red + LUMA[1] * green + LUMA[2] * blue
}

fn matrix(matrix: &[[f32; 3]; 3], rgb: [f32; 3]) -> [f32; 3] {
    matrix.map(|row| row[0] * rgb[0] + row[1] * rgb[1] + row[2] * rgb[2])
}

fn multiply(a: &[[f32; 3]; 3], b: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    std::array::from_fn(|row| {
        std::array::from_fn(|column| (0..3).map(|index| a[row][index] * b[index][column]).sum())
    })
}

// Runs `work` on every row of `pixels` on all cores, with the row index.
fn for_rows<T: Send>(pixels: &mut [T], row: usize, work: impl Fn(usize, &mut [T]) + Sync) {
    let rows = pixels.len() / row.max(1);
    if rows == 0 {
        return;
    }
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    let per_thread = rows.div_ceil(threads);
    std::thread::scope(|scope| {
        for (chunk_index, chunk) in pixels.chunks_mut(per_thread * row).enumerate() {
            let work = &work;
            scope.spawn(move || {
                for (index, line) in chunk.chunks_mut(row).enumerate() {
                    work(chunk_index * per_thread + index, line);
                }
            });
        }
    });
}

// Changes the color of every pixel, in [0, 1] and keeping its alpha.
fn map_colors(picture: &Picture, map: impl Fn([f32; 3]) -> [f32; 3] + Sync) -> Picture {
    map_pixels(picture, |_, _, rgb| map(rgb))
}

// Changes the color of every pixel from its position and color in [0, 1], keeping its alpha.
fn map_pixels(
    picture: &Picture,
    map: impl Fn(usize, usize, [f32; 3]) -> [f32; 3] + Sync,
) -> Picture {
    let mut pixels = picture.pixels.clone();
    for_rows(&mut pixels, picture.width as usize * 4, |y, row| {
        for (x, pixel) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let rgb = map(
                x,
                y,
                [0, 1, 2].map(|channel| f32::from(pixel[channel]) / 255.0),
            );
            for (value, mapped) in pixel.iter_mut().zip(rgb) {
                *value = (mapped * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    });
    Picture::new(picture.width, picture.height, pixels)
}

// The adjust sliders: the light ones through a table of tones and a vignette, the color ones
// through a color matrix and a vibrance that saturates dull colors most.
fn adjust(picture: &Picture, values: &[f32; 11]) -> Picture {
    let [
        exposure,
        brightness,
        contrast,
        highlights,
        shadows,
        vignette,
        saturation,
        vibrance,
        warmth,
        tint,
        hue,
    ] = *values;
    let gain = exposure.exp2();
    let contrast = contrast / 100.0;
    let contrast = if contrast > 0.0 {
        1.0 / (1.0 - contrast * 0.99)
    } else {
        1.0 + contrast
    };
    let (highlights, shadows) = (highlights / 100.0, shadows / 100.0);
    let tones: [f32; 256] = std::array::from_fn(|value| {
        let light = image::srgb_to_linear(value as u8) * gain;
        let value = f32::from(image::linear_to_srgb(light)) / 255.0 + brightness / 200.0;
        let value = ((value - 0.5) * contrast + 0.5).clamp(0.0, 1.0);
        // Shadows change the dark tones most and highlights the bright ones, the ends stay.
        value
            + shadows * 1.5 * value * (1.0 - value).powi(2)
            + highlights * 1.5 * value.powi(2) * (1.0 - value)
    });

    // Saturation and hue rotation around the gray axis, like CSS filters.
    let saturation = 1.0 + saturation / 100.0;
    let saturate = [
        [
            LUMA[0] + (1.0 - LUMA[0]) * saturation,
            LUMA[1] * (1.0 - saturation),
            LUMA[2] * (1.0 - saturation),
        ],
        [
            LUMA[0] * (1.0 - saturation),
            LUMA[1] + (1.0 - LUMA[1]) * saturation,
            LUMA[2] * (1.0 - saturation),
        ],
        [
            LUMA[0] * (1.0 - saturation),
            LUMA[1] * (1.0 - saturation),
            LUMA[2] + (1.0 - LUMA[2]) * saturation,
        ],
    ];
    let (sin, cos) = hue.to_radians().sin_cos();
    let rotate = [
        [
            0.213 + cos * 0.787 - sin * 0.213,
            0.715 - cos * 0.715 - sin * 0.715,
            0.072 - cos * 0.072 + sin * 0.928,
        ],
        [
            0.213 - cos * 0.213 + sin * 0.143,
            0.715 + cos * 0.285 + sin * 0.140,
            0.072 - cos * 0.072 - sin * 0.283,
        ],
        [
            0.213 - cos * 0.213 - sin * 0.787,
            0.715 - cos * 0.715 + sin * 0.715,
            0.072 + cos * 0.928 + sin * 0.072,
        ],
    ];
    // Warmer is more red and less blue, a positive tint is more magenta.
    let (warmth, tint) = (warmth / 500.0, tint / 500.0);
    let balance = [
        [1.0 + warmth, 0.0, 0.0],
        [0.0, 1.0 - tint, 0.0],
        [0.0, 0.0, 1.0 - warmth],
    ];
    let colors = multiply(&balance, &multiply(&rotate, &saturate));
    let vibrance = vibrance / 100.0;
    let vignette = vignette / 100.0;
    let center = (picture.width as f32 / 2.0, picture.height as f32 / 2.0);
    map_pixels(picture, |x, y, rgb| {
        let rgb = matrix(
            &colors,
            rgb.map(|value| tones[(value * 255.0).round() as usize]),
        );
        let rgb = if vibrance == 0.0 {
            rgb
        } else {
            let gray = luma(rgb[0], rgb[1], rgb[2]);
            let dullness = 1.0
                - (rgb.iter().copied().fold(0.0, f32::max)
                    - rgb.iter().copied().fold(1.0, f32::min))
                .clamp(0.0, 1.0);
            rgb.map(|value| gray + (value - gray) * (1.0 + vibrance * dullness))
        };
        if vignette == 0.0 {
            return rgb;
        }
        // Positive vignettes darken the corners, negative ones lighten them.
        let distance = ((x as f32 + 0.5 - center.0) / center.0)
            .hypot((y as f32 + 0.5 - center.1) / center.1)
            / std::f32::consts::SQRT_2;
        let edge = ((distance - 0.3) / 0.7).clamp(0.0, 1.0);
        let weight = edge * edge * (3.0 - 2.0 * edge) * vignette.abs() * 0.85;
        rgb.map(|value| {
            if vignette > 0.0 {
                value * (1.0 - weight)
            } else {
                value + (1.0 - value) * weight
            }
        })
    })
}

/// Stretches the tones between the darkest and brightest half percent of the pixels of a
/// sample, like the selected part, to the full range, the same for every channel so colors
/// keep their hue.
pub(super) fn auto_enhance(picture: &Picture, sample: &Picture) -> Picture {
    let mut histogram = [0usize; 256];
    for pixel in sample.pixels.as_chunks::<4>().0 {
        let [red, green, blue] = [0, 1, 2].map(|channel| f32::from(pixel[channel]));
        histogram[luma(red, green, blue).round().clamp(0.0, 255.0) as usize] += 1;
    }
    let total: usize = histogram.iter().sum();
    let clip = total / 200;
    let level = |order: Vec<usize>| {
        let mut count = 0;
        order
            .into_iter()
            .find(|&value| {
                count += histogram[value];
                count > clip
            })
            .unwrap_or(0) as f32
            / 255.0
    };
    let (low, high) = (level((0..256).collect()), level((0..256).rev().collect()));
    if high - low < 0.04 {
        return picture.clone();
    }
    map_colors(picture, |rgb| rgb.map(|value| (value - low) / (high - low)))
}

// The median of every channel in a square around every pixel.
fn reduce_noise(picture: &Picture, radius: usize) -> Picture {
    let (width, height) = (picture.width as usize, picture.height as usize);
    if radius == 0 {
        return picture.clone();
    }
    let mut pixels = picture.pixels.clone();
    for_rows(&mut pixels, width * 4, |y, row| {
        let mut window = Vec::with_capacity((2 * radius + 1).pow(2));
        for x in 0..width {
            for channel in 0..3 {
                window.clear();
                for sample_y in y.saturating_sub(radius)..(y + radius + 1).min(height) {
                    for sample_x in x.saturating_sub(radius)..(x + radius + 1).min(width) {
                        window.push(picture.pixels[(sample_y * width + sample_x) * 4 + channel]);
                    }
                }
                let middle = window.len() / 2;
                row[x * 4 + channel] = *window.select_nth_unstable(middle).1;
            }
        }
    });
    Picture::new(picture.width, picture.height, pixels)
}

// Fills blocks of a size with their average color.
fn pixelate(picture: &Picture, size: usize) -> Picture {
    filter_premultiplied(picture, |pixels, width, height| {
        for top in (0..height).step_by(size) {
            for left in (0..width).step_by(size) {
                let (bottom, right) = ((top + size).min(height), (left + size).min(width));
                let mut sum = [0.0f32; 4];
                for y in top..bottom {
                    for x in left..right {
                        for channel in 0..4 {
                            sum[channel] += pixels[(y * width + x) * 4 + channel];
                        }
                    }
                }
                let average = sum.map(|value| value / ((bottom - top) * (right - left)) as f32);
                for y in top..bottom {
                    for x in left..right {
                        pixels[(y * width + x) * 4..][..4].copy_from_slice(&average);
                    }
                }
            }
        }
    })
}

/// Turns a picture by an angle in degrees around its middle, clockwise for positive angles,
/// enlarged just enough that it still fills its size.
pub(crate) fn straighten(picture: &Picture, degrees: f32) -> Picture {
    if degrees == 0.0 {
        return picture.clone();
    }
    let (width, height) = (picture.width as usize, picture.height as usize);
    let (sin, cos) = degrees.to_radians().sin_cos();
    let (w, h) = (width as f32, height as f32);
    let scale = ((w * cos.abs() + h * sin.abs()) / w).max((w * sin.abs() + h * cos.abs()) / h);
    let source = premultiply(&picture.pixels);
    let mut output = vec![0.0; source.len()];
    let sample = |x: f32, y: f32, channel: usize| {
        let (x, y) = (x.clamp(0.0, w - 1.0), y.clamp(0.0, h - 1.0));
        let (left, top) = (x.floor() as usize, y.floor() as usize);
        let (right, bottom) = ((left + 1).min(width - 1), (top + 1).min(height - 1));
        let (fx, fy) = (x - left as f32, y - top as f32);
        let at = |x: usize, y: usize| source[(y * width + x) * 4 + channel];
        (at(left, top) * (1.0 - fx) + at(right, top) * fx) * (1.0 - fy)
            + (at(left, bottom) * (1.0 - fx) + at(right, bottom) * fx) * fy
    };
    for_rows(&mut output, width * 4, |y, row| {
        for (x, pixel) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            // The source position of every pixel, turned back and shrunk by the scale.
            let (dx, dy) = (x as f32 + 0.5 - w / 2.0, y as f32 + 0.5 - h / 2.0);
            let source_x = (dx * cos + dy * sin) / scale + w / 2.0 - 0.5;
            let source_y = (-dx * sin + dy * cos) / scale + h / 2.0 - 0.5;
            *pixel = std::array::from_fn(|channel| sample(source_x, source_y, channel));
        }
    });
    Picture::new(picture.width, picture.height, unpremultiply(&output))
}

// Premultiplies a picture, filters it on all cores and converts it back.
fn filter_premultiplied(
    picture: &Picture,
    filter: impl FnOnce(&mut Vec<f32>, usize, usize),
) -> Picture {
    let mut pixels = premultiply(&picture.pixels);
    filter(&mut pixels, picture.width as usize, picture.height as usize);
    Picture::new(picture.width, picture.height, unpremultiply(&pixels))
}

// Swaps the rows and columns of premultiplied pixels.
fn transpose(pixels: &[f32], width: usize, height: usize) -> Vec<f32> {
    let mut output = vec![0.0; pixels.len()];
    for_rows(&mut output, height * 4, |x, row| {
        for (y, pixel) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let at = (y * width + x) * 4;
            pixel.copy_from_slice(&pixels[at..at + 4]);
        }
    });
    output
}

// Averages every pixel with `radius` pixels on both sides in its row, repeating the edges.
fn blur_rows(pixels: &mut [f32], width: usize, radius: usize) {
    if radius == 0 || width == 0 {
        return;
    }
    let scale = 1.0 / (2 * radius + 1) as f32;
    for_rows(pixels, width * 4, |_, row| {
        let source = row.to_vec();
        let at = |x: isize| x.clamp(0, width as isize - 1) as usize * 4;
        let mut sum = [0.0f32; 4];
        for x in -(radius as isize)..=radius as isize {
            for channel in 0..4 {
                sum[channel] += source[at(x) + channel];
            }
        }
        for x in 0..width {
            for channel in 0..4 {
                row[x * 4 + channel] = sum[channel] * scale;
            }
            let (enter, leave) = (
                at((x + radius + 1) as isize),
                at(x as isize - radius as isize),
            );
            for channel in 0..4 {
                sum[channel] += source[enter + channel] - source[leave + channel];
            }
        }
    });
}

// Blurs premultiplied pixels with box radii in both directions.
fn blur_both(pixels: &mut Vec<f32>, width: usize, height: usize, radii: &[usize]) {
    for &radius in radii {
        blur_rows(pixels, width, radius);
    }
    let mut transposed = transpose(pixels, width, height);
    for &radius in radii {
        blur_rows(&mut transposed, height, radius);
    }
    *pixels = transpose(&transposed, height, width);
}

fn box_blur(picture: &Picture, radius: usize) -> Picture {
    filter_premultiplied(picture, |pixels, width, height| {
        blur_both(pixels, width, height, &[radius]);
    })
}

// The radii of three box blurs that together look like a gaussian blur.
fn gaussian_radii(sigma: f32) -> [usize; 3] {
    let ideal = (12.0 * sigma * sigma / 3.0 + 1.0).sqrt();
    let mut lower = ideal.floor() as usize;
    if lower.is_multiple_of(2) {
        lower = lower.saturating_sub(1);
    }
    let upper = lower + 2;
    let lower_f = lower as f32;
    let count = ((12.0 * sigma * sigma - 3.0 * lower_f * lower_f - 12.0 * lower_f - 9.0)
        / (-4.0 * lower_f - 4.0))
        .round();
    std::array::from_fn(|index| {
        let size = if (index as f32) < count { lower } else { upper };
        size / 2
    })
}

fn gaussian_blur(picture: &Picture, sigma: f32) -> Picture {
    if sigma <= 0.0 {
        return picture.clone();
    }
    filter_premultiplied(picture, |pixels, width, height| {
        blur_both(pixels, width, height, &gaussian_radii(sigma));
    })
}

// Averages samples of premultiplied pixels for every pixel, at offsets from `samples`.
fn sample_blur(
    picture: &Picture,
    samples: usize,
    offset: impl Fn(f32, f32, f32) -> (f32, f32) + Sync,
) -> Picture {
    if samples < 2 {
        return picture.clone();
    }
    filter_premultiplied(picture, |pixels, width, height| {
        let source = std::mem::take(pixels);
        let mut output = vec![0.0; source.len()];
        for_rows(&mut output, width * 4, |y, row| {
            for (x, pixel) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let mut sum = [0.0f32; 4];
                for sample in 0..samples {
                    let progress = sample as f32 / (samples - 1) as f32;
                    let (sample_x, sample_y) = offset(x as f32, y as f32, progress);
                    let sample_x = (sample_x.round() as isize).clamp(0, width as isize - 1);
                    let sample_y = (sample_y.round() as isize).clamp(0, height as isize - 1);
                    let at = (sample_y as usize * width + sample_x as usize) * 4;
                    for channel in 0..4 {
                        sum[channel] += source[at + channel];
                    }
                }
                *pixel = sum.map(|value| value / samples as f32);
            }
        });
        *pixels = output;
    })
}

fn motion_blur(picture: &Picture, distance: f32, angle: f32) -> Picture {
    let (sin, cos) = angle.sin_cos();
    sample_blur(picture, distance.round() as usize + 1, |x, y, progress| {
        let along = (progress - 0.5) * distance;
        (x + cos * along, y - sin * along)
    })
}

fn radial_blur(picture: &Picture, amount: f32) -> Picture {
    let center = (picture.width as f32 / 2.0, picture.height as f32 / 2.0);
    let reach = center.0.hypot(center.1) * amount * 0.25;
    sample_blur(
        picture,
        reach.round().clamp(0.0, 64.0) as usize + 1,
        |x, y, progress| {
            let scale = 1.0 - amount * 0.25 * progress;
            (
                center.0 + (x - center.0) * scale,
                center.1 + (y - center.1) * scale,
            )
        },
    )
}

// Adds the difference with a blurred copy, which strengthens edges.
fn sharpen(picture: &Picture, amount: f32, radius: f32) -> Picture {
    let blurred = gaussian_blur(picture, radius);
    let mut pixels = picture.pixels.clone();
    for (pixel, blurred) in pixels
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(blurred.pixels.as_chunks::<4>().0)
    {
        for (value, blurred) in pixel[..3].iter_mut().zip(blurred) {
            let value_f = f32::from(*value);
            *value = (value_f + (value_f - f32::from(*blurred)) * amount)
                .round()
                .clamp(0.0, 255.0) as u8;
        }
    }
    Picture::new(picture.width, picture.height, pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    // A 4x4 picture with a white left half and black right half.
    fn halves() -> Picture {
        let pixels = (0..16)
            .flat_map(|index| {
                if index % 4 < 2 {
                    [255, 255, 255, 255]
                } else {
                    [0, 0, 0, 255]
                }
            })
            .collect();
        Picture::new(4, 4, pixels)
    }

    #[test]
    fn neutral_values_keep_the_picture() {
        let picture = Picture::new(2, 1, vec![12, 130, 250, 255, 200, 100, 50, 128]);
        for (filter, values) in [
            (Filter::Adjust, &Filter::Adjust.defaults()[..]),
            (Filter::GaussianBlur, &[0.0]),
            (Filter::BoxBlur, &[0.0]),
            (Filter::MotionBlur, &[0.0, 0.0]),
            (Filter::RadialBlur, &[0.0]),
            (Filter::Sharpen, &[0.0, 1.0]),
        ] {
            let filtered = filter.apply(&picture, values);
            assert!(
                filtered
                    .pixels
                    .iter()
                    .zip(&picture.pixels)
                    .all(|(a, b)| a.abs_diff(*b) <= 1),
                "{filter:?}"
            );
        }
    }

    #[test]
    fn color_filters_change_colors() {
        let picture = Picture::new(1, 1, vec![255, 0, 0, 200]);
        assert_eq!(
            Filter::Invert.apply(&picture, &[]).pixels,
            [0, 255, 255, 200]
        );
        assert_eq!(
            Filter::Grayscale.apply(&picture, &[]).pixels,
            [54, 54, 54, 200]
        );
        assert_eq!(
            Filter::Threshold.apply(&picture, &[128.0]).pixels,
            [0, 0, 0, 200]
        );
        assert_eq!(
            Filter::Solarize.apply(&picture, &[128.0]).pixels,
            [0, 0, 0, 200]
        );
        let adjust = |index: usize, value: f32| {
            let mut values = Filter::Adjust.defaults();
            values[index] = value;
            Filter::Adjust.apply(&picture, &values).pixels
        };
        assert_eq!(adjust(0, 1.0), [255, 0, 0, 200]);
        assert_eq!(adjust(6, -100.0), [54, 54, 54, 200]);
        let gray = Picture::new(1, 1, vec![64, 64, 64, 255]);
        let mut values = Filter::Adjust.defaults();
        values[4] = 100.0;
        assert!(Filter::Adjust.apply(&gray, &values).pixels[0] > 90);
        values[4] = 0.0;
        values[5] = 100.0;
        assert_eq!(Filter::Adjust.apply(&gray, &values).pixels[0], 64);
    }

    #[test]
    fn vignettes_change_the_corners() {
        let gray = Picture::new(9, 9, [128, 128, 128, 255].repeat(81));
        let mut values = Filter::Adjust.defaults();
        values[5] = 100.0;
        let darker = Filter::Adjust.apply(&gray, &values);
        assert_eq!(darker.pixel(4, 4)[0], 128);
        assert!(darker.pixel(0, 0)[0] < 100);
        values[5] = -100.0;
        assert!(Filter::Adjust.apply(&gray, &values).pixel(0, 0)[0] > 150);
    }

    #[test]
    fn auto_enhance_stretches_the_tones() {
        let pixels = [100, 100, 100, 255, 150, 150, 150, 255].repeat(4);
        let enhanced = Filter::AutoEnhance.apply(&Picture::new(8, 1, pixels), &[]);
        assert_eq!(enhanced.pixel(0, 0), [0, 0, 0, 255]);
        assert_eq!(enhanced.pixel(1, 0), [255, 255, 255, 255]);
        let flat = Picture::new(1, 1, vec![100, 100, 100, 255]);
        assert_eq!(Filter::AutoEnhance.apply(&flat, &[]), flat);
    }

    #[test]
    fn noise_and_pixelation_smooth_details() {
        let mut pixels = [0, 0, 0, 255].repeat(9);
        pixels[16..20].copy_from_slice(&[255, 255, 255, 255]);
        let speck = Picture::new(3, 3, pixels);
        assert!(
            Filter::ReduceNoise
                .apply(&speck, &[1.0])
                .pixels
                .chunks(4)
                .all(|pixel| pixel == [0, 0, 0, 255])
        );
        let blocks = Filter::Pixelate.apply(&halves(), &[4.0]);
        assert!(
            blocks
                .pixels
                .chunks(4)
                .all(|pixel| pixel == [128, 128, 128, 255])
        );
    }

    #[test]
    fn straightening_fills_the_picture() {
        let picture = halves();
        assert_eq!(straighten(&picture, 0.0), picture);
        let turned = straighten(&picture, 10.0);
        assert_eq!((turned.width, turned.height), (4, 4));
        assert!(turned.pixels.chunks(4).all(|pixel| pixel[3] == 255));
        // The white left half stays on the left.
        assert!(turned.pixel(0, 2)[0] > 200 && turned.pixel(3, 2)[0] < 50);
    }

    #[test]
    fn blurs_mix_neighbours() {
        for filter in [Filter::GaussianBlur, Filter::BoxBlur, Filter::MotionBlur] {
            let blurred = filter.apply(&halves(), &[4.0, 0.0]);
            let edge = blurred.pixel(1, 1)[0];
            assert!(edge > 0 && edge < 255, "{filter:?}: {edge}");
            assert_eq!(blurred.pixel(1, 1)[3], 255);
        }
        let sharpened = Filter::Sharpen.apply(&halves(), &[100.0, 1.0]);
        assert_eq!(sharpened.pixel(1, 1)[0], 255);
        assert_eq!(sharpened.pixel(2, 1)[0], 0);
    }

    #[test]
    fn gaussian_radii_grow_with_sigma() {
        assert_eq!(gaussian_radii(0.5), [0, 0, 0]);
        let radii = gaussian_radii(10.0);
        assert!(
            radii.iter().all(|radius| (8..=10).contains(radius)),
            "{radii:?}"
        );
    }
}
