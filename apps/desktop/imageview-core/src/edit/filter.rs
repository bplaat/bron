/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use super::picture::{Picture, premultiply, to_byte, unpremultiply};

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

    /// The text of a value next to its slider, with a decimal for small ranges.
    pub fn value_text(&self, value: f32) -> String {
        if self.max - self.min <= 10.0 {
            format!("{value:.1}")
        } else {
            format!("{}", value.round() as i64)
        }
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
            Self::Invert => map_channels(picture, |value| 1.0 - value),
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
                map_channels(
                    picture,
                    |value| {
                        if value > level { 1.0 - value } else { value }
                    },
                )
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

// The number of workers for a size in bytes. Small previews cost less to process directly than
// to start threads, so each worker gets at least 256 KiB.
fn worker_count(bytes: usize) -> usize {
    match bytes / (256 * 1024) {
        0 | 1 => 1,
        workers => std::thread::available_parallelism()
            .map_or(1, usize::from)
            .min(workers),
    }
}

// Runs `work` on every part on all cores, with the part index.
fn for_parts<T: Send>(parts: Vec<T>, work: impl Fn(usize, T) + Sync) {
    if let [_] = parts[..] {
        parts.into_iter().for_each(|part| work(0, part));
        return;
    }
    std::thread::scope(|scope| {
        for (index, part) in parts.into_iter().enumerate() {
            let work = &work;
            scope.spawn(move || work(index, part));
        }
    });
}

/// Runs `work` on every row of `pixels` on all cores, with the row index; the last row may be
/// shorter.
pub(super) fn for_rows<T: Send>(
    pixels: &mut [T],
    row: usize,
    work: impl Fn(usize, &mut [T]) + Sync,
) {
    if pixels.is_empty() {
        return;
    }
    let per_worker = pixels
        .len()
        .div_ceil(row)
        .div_ceil(worker_count(size_of_val(pixels)));
    let parts = pixels.chunks_mut(per_worker * row).collect();
    for_parts(parts, |part, rows| {
        for (index, line) in rows.chunks_mut(row).enumerate() {
            work(part * per_worker + index, line);
        }
    });
}

// Changes the color of every pixel, in [0, 1] and keeping its alpha.
fn map_colors(picture: &Picture, map: impl Fn([f32; 3]) -> [f32; 3] + Sync) -> Picture {
    map_pixels(picture, |_, _, rgb| {
        map(rgb.map(|value| f32::from(value) / 255.0))
    })
}

// Changes the color of every pixel from its position and samples to one in [0, 1], keeping its
// alpha.
fn map_pixels(
    picture: &Picture,
    map: impl Fn(usize, usize, [u8; 3]) -> [f32; 3] + Sync,
) -> Picture {
    let mut pixels = picture.pixels.clone();
    for_rows(&mut pixels, picture.width as usize * 4, |y, row| {
        for (x, pixel) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let rgb = map(x, y, [pixel[0], pixel[1], pixel[2]]);
            for (value, mapped) in pixel.iter_mut().zip(rgb) {
                *value = to_byte(mapped);
            }
        }
    });
    Picture::new(picture.width, picture.height, pixels)
}

// Changes every color channel on its own, in [0, 1] and keeping alpha, through a table of the
// 256 possible samples.
fn map_channels(picture: &Picture, map: impl Fn(f32) -> f32) -> Picture {
    let table: [u8; 256] = std::array::from_fn(|value| to_byte(map(value as f32 / 255.0)));
    let mut pixels = picture.pixels.clone();
    for_rows(&mut pixels, picture.width as usize * 4, |_, row| {
        for pixel in row.as_chunks_mut::<4>().0 {
            for value in &mut pixel[..3] {
                *value = table[usize::from(*value)];
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
        let rgb = matrix(&colors, rgb.map(|value| tones[usize::from(value)]));
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
        histogram[(luma(red, green, blue) + 0.5).clamp(0.0, 255.0) as usize] += 1;
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
    map_channels(picture, |value| (value - low) / (high - low))
}

// A histogram of the samples of a window that follows its median, which moves little as the
// window slides, so the cost doesn't grow with the window.
struct Median {
    histogram: [u32; 256],
    median: usize,
    // The samples below the median.
    below: u32,
}

impl Median {
    fn add(&mut self, sample: u8) {
        self.histogram[usize::from(sample)] += 1;
        self.below += u32::from(usize::from(sample) < self.median);
    }

    fn remove(&mut self, sample: u8) {
        self.histogram[usize::from(sample)] -= 1;
        self.below -= u32::from(usize::from(sample) < self.median);
    }

    // The sample at a rank of the sorted window.
    const fn at(&mut self, rank: u32) -> u8 {
        while self.below > rank {
            self.median -= 1;
            self.below -= self.histogram[self.median];
        }
        while self.below + self.histogram[self.median] <= rank {
            self.below += self.histogram[self.median];
            self.median += 1;
        }
        self.median as u8
    }
}

// The median of every channel in a square around every pixel.
fn reduce_noise(picture: &Picture, radius: usize) -> Picture {
    let (width, height) = (picture.width as usize, picture.height as usize);
    if radius == 0 {
        return picture.clone();
    }
    let source = picture.pixels.as_chunks::<4>().0;
    let mut pixels = picture.pixels.clone();
    for_rows(&mut pixels, width * 4, |y, row| {
        let lines = y.saturating_sub(radius)..(y + radius + 1).min(height);
        let line_count = lines.len();
        let column = |x: usize| lines.clone().map(move |line| &source[line * width + x]);
        let mut channels = [(); 3].map(|()| Median {
            histogram: [0; 256],
            median: 0,
            below: 0,
        });
        for sample in (0..(radius + 1).min(width)).flat_map(column) {
            for (median, &value) in channels.iter_mut().zip(sample) {
                median.add(value);
            }
        }
        for (x, pixel) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            if x > 0 {
                if x + radius < width {
                    for sample in column(x + radius) {
                        for (median, &value) in channels.iter_mut().zip(sample) {
                            median.add(value);
                        }
                    }
                }
                if x > radius {
                    for sample in column(x - radius - 1) {
                        for (median, &value) in channels.iter_mut().zip(sample) {
                            median.remove(value);
                        }
                    }
                }
            }
            let columns = (x + radius + 1).min(width) - x.saturating_sub(radius);
            let rank = (columns * line_count / 2) as u32;
            for (value, median) in pixel.iter_mut().zip(&mut channels) {
                *value = median.at(rank);
            }
        }
    });
    Picture::new(picture.width, picture.height, pixels)
}

// Fills blocks of a size with their average color.
fn pixelate(picture: &Picture, size: usize) -> Picture {
    filter_premultiplied(picture, |pixels, width, _| {
        for_rows(pixels, width * size * 4, |_, block| {
            let block = block.as_chunks_mut::<4>().0;
            let rows = block.len() / width;
            for left in (0..width).step_by(size) {
                let right = (left + size).min(width);
                let mut sum = [0.0f32; 4];
                for row in block.chunks(width) {
                    for pixel in &row[left..right] {
                        for channel in 0..4 {
                            sum[channel] += pixel[channel];
                        }
                    }
                }
                let average = sum.map(|value| value / (rows * (right - left)) as f32);
                for row in block.chunks_mut(width) {
                    row[left..right].fill(average);
                }
            }
        });
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
    let corners = source.as_chunks::<4>().0;
    let sample = |x: f32, y: f32| {
        let (x, y) = (x.clamp(0.0, w - 1.0), y.clamp(0.0, h - 1.0));
        let (left, top) = (x.floor() as usize, y.floor() as usize);
        let (right, bottom) = ((left + 1).min(width - 1), (top + 1).min(height - 1));
        let (fx, fy) = (x - left as f32, y - top as f32);
        let at = |x: usize, y: usize| &corners[y * width + x];
        let (a, b) = (at(left, top), at(right, top));
        let (c, d) = (at(left, bottom), at(right, bottom));
        let mut pixel = [0.0; 4];
        for channel in 0..4 {
            pixel[channel] = (a[channel] * (1.0 - fx) + b[channel] * fx) * (1.0 - fy)
                + (c[channel] * (1.0 - fx) + d[channel] * fx) * fy;
        }
        pixel
    };
    for_rows(&mut output, width * 4, |y, row| {
        for (x, pixel) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            // The source position of every pixel, turned back and shrunk by the scale.
            let (dx, dy) = (x as f32 + 0.5 - w / 2.0, y as f32 + 0.5 - h / 2.0);
            let source_x = (dx * cos + dy * sin) / scale + w / 2.0 - 0.5;
            let source_y = (-dx * sin + dy * cos) / scale + h / 2.0 - 0.5;
            *pixel = sample(source_x, source_y);
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

// Averages every pixel with `radius` pixels on both sides in its row, repeating the edges.
fn blur_rows(source: &[f32], output: &mut [f32], width: usize, radius: usize) {
    let scale = 1.0 / (2 * radius + 1) as f32;
    for_rows(output, width * 4, |y, row| {
        let line = source[y * width * 4..][..width * 4].as_chunks::<4>().0;
        let mut sum = [0.0f32; 4];
        for offset in 0..=2 * radius {
            let pixel = &line[offset.saturating_sub(radius).min(width - 1)];
            for channel in 0..4 {
                sum[channel] += pixel[channel];
            }
        }
        for (x, out) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            *out = sum.map(|value| value * scale);
            let enter = &line[(x + radius + 1).min(width - 1)];
            let leave = &line[x.saturating_sub(radius)];
            for channel in 0..4 {
                sum[channel] += enter[channel] - leave[channel];
            }
        }
    });
}

// Averages every pixel with `radius` pixels on both sides in its column, repeating the edges.
// Every worker has a strip of columns and slides a whole row of sums down it, which keeps the
// memory access in rows and leaves independent samples to vectorize.
fn blur_columns(source: &[f32], output: &mut [f32], width: usize, radius: usize) {
    let height = output.len() / (width * 4);
    let scale = 1.0 / (2 * radius + 1) as f32;
    let per_worker = width.div_ceil(worker_count(size_of_val(output)));
    let mut strips: Vec<Vec<&mut [f32]>> = (0..width.div_ceil(per_worker))
        .map(|_| Vec::with_capacity(height))
        .collect();
    for row in output.chunks_mut(width * 4) {
        for (strip, part) in strips.iter_mut().zip(row.chunks_mut(per_worker * 4)) {
            strip.push(part);
        }
    }
    for_parts(strips, |index, mut rows| {
        let length = rows[0].len();
        let line = |y: usize| &source[(y * width + index * per_worker) * 4..][..length];
        let mut sums = vec![0.0f32; length];
        for offset in 0..=2 * radius {
            let samples = line(offset.saturating_sub(radius).min(height - 1));
            for (sum, &value) in sums.iter_mut().zip(samples) {
                *sum += value;
            }
        }
        for (y, row) in rows.iter_mut().enumerate() {
            for (out, &sum) in row.iter_mut().zip(&sums) {
                *out = sum * scale;
            }
            let enter = line((y + radius + 1).min(height - 1));
            let leave = line(y.saturating_sub(radius));
            for ((sum, &enter), &leave) in sums.iter_mut().zip(enter).zip(leave) {
                *sum += enter - leave;
            }
        }
    });
}

// Blurs premultiplied pixels with box radii in both directions.
fn blur_both(pixels: &mut Vec<f32>, width: usize, radii: &[usize]) {
    let mut scratch = vec![0.0; pixels.len()];
    for &radius in radii.iter().filter(|&&radius| radius > 0) {
        blur_rows(pixels, &mut scratch, width, radius);
        std::mem::swap(pixels, &mut scratch);
    }
    for &radius in radii.iter().filter(|&&radius| radius > 0) {
        blur_columns(pixels, &mut scratch, width, radius);
        std::mem::swap(pixels, &mut scratch);
    }
}

fn box_blur(picture: &Picture, radius: usize) -> Picture {
    filter_premultiplied(picture, |pixels, width, _| {
        blur_both(pixels, width, &[radius]);
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
    filter_premultiplied(picture, |pixels, width, _| {
        blur_both(pixels, width, &gaussian_radii(sigma));
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

// Adds the difference with a blurred copy, which strengthens edges. The blurred copy becomes
// the result, so no other picture is allocated.
fn sharpen(picture: &Picture, amount: f32, radius: f32) -> Picture {
    let mut sharpened = gaussian_blur(picture, radius);
    let row = picture.width as usize * 4;
    for_rows(&mut sharpened.pixels, row, |y, line| {
        let original = &picture.pixels[y * row..][..row];
        for (pixel, original) in line
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(original.as_chunks::<4>().0)
        {
            for (value, original) in pixel[..3].iter_mut().zip(original) {
                let original = f32::from(*original);
                *value = (original + (original - f32::from(*value)) * amount + 0.5)
                    .clamp(0.0, 255.0) as u8;
            }
            pixel[3] = original[3];
        }
    });
    sharpened
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_workers_visit_each_row_with_its_global_index() {
        for rows in [0, 1, 100, 2115, 40000] {
            let mut pixels = vec![255; rows * 31 + 5];
            for_rows(&mut pixels, 31, |index, row| row.fill((index % 251) as u8));
            for (index, row) in pixels.chunks(31).enumerate() {
                assert!(row.iter().all(|&value| value == (index % 251) as u8));
            }
        }
    }

    // Deterministic noise with a varying alpha.
    fn noise(width: u32, height: u32) -> Picture {
        let mut state = 0x1234_5678u32;
        let pixels = (0..width * height * 4)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as u8
            })
            .collect();
        Picture::new(width, height, pixels)
    }

    #[test]
    fn medians_match_a_sorted_window() {
        for (width, height) in [(1, 1), (1, 9), (9, 1), (4, 3), (23, 17)] {
            let picture = noise(width, height);
            for radius in [1, 2, 3, 30] {
                let (w, h) = (width as usize, height as usize);
                let mut expected = picture.pixels.clone();
                for y in 0..h {
                    for x in 0..w {
                        for channel in 0..3 {
                            let mut window = Vec::new();
                            for sample_y in y.saturating_sub(radius)..(y + radius + 1).min(h) {
                                for sample_x in x.saturating_sub(radius)..(x + radius + 1).min(w) {
                                    window.push(
                                        picture.pixels[(sample_y * w + sample_x) * 4 + channel],
                                    );
                                }
                            }
                            window.sort_unstable();
                            expected[(y * w + x) * 4 + channel] = window[window.len() / 2];
                        }
                    }
                }
                assert_eq!(
                    reduce_noise(&picture, radius).pixels,
                    expected,
                    "{width}x{height}, radius={radius}"
                );
            }
        }
    }

    #[test]
    fn blurs_match_a_pixel_at_a_time_reference() {
        fn reference(
            pixels: &[f32],
            stride: usize,
            step: usize,
            lines: usize,
            radius: usize,
        ) -> Vec<f32> {
            let scale = 1.0 / (2 * radius + 1) as f32;
            let length = pixels.len() / 4 / lines;
            let mut output = pixels.to_vec();
            for line in 0..lines {
                let at = |index: isize, channel: usize| {
                    let index = index.clamp(0, length as isize - 1) as usize;
                    pixels[(line * stride + index * step) * 4 + channel]
                };
                let mut sum = [0.0f32; 4];
                for index in -(radius as isize)..=radius as isize {
                    for (channel, sum) in sum.iter_mut().enumerate() {
                        *sum += at(index, channel);
                    }
                }
                for index in 0..length {
                    for (channel, sum) in sum.iter_mut().enumerate() {
                        output[(line * stride + index * step) * 4 + channel] = *sum * scale;
                        *sum += at((index + radius + 1) as isize, channel)
                            - at(index as isize - radius as isize, channel);
                    }
                }
            }
            output
        }

        // The large size exercises the workers.
        for (width, height) in [(1, 1), (5, 1), (1, 5), (17, 9), (300, 200)] {
            let source = premultiply(&noise(width, height).pixels);
            let (width, height) = (width as usize, height as usize);
            for radius in [1, 2, 7, 40] {
                let mut output = vec![0.0; source.len()];
                blur_rows(&source, &mut output, width, radius);
                assert_eq!(output, reference(&source, width, 1, height, radius));
                blur_columns(&source, &mut output, width, radius);
                assert_eq!(output, reference(&source, 1, width, width, radius));
            }
        }
    }

    #[test]
    fn channel_tables_match_mapping_the_colors() {
        let picture = noise(16, 16);
        let filters: [fn(f32) -> f32; 3] = [
            |value| 1.0 - value,
            |value| if value > 0.5 { 1.0 - value } else { value },
            |value| (value - 0.2) / 0.5,
        ];
        for filter in filters {
            assert_eq!(
                map_channels(&picture, filter),
                map_colors(&picture, |rgb| rgb.map(filter))
            );
        }
    }

    #[test]
    fn pixelation_covers_blocks_taller_than_the_picture() {
        let blocks = Filter::Pixelate.apply(&halves(), &[9.0]);
        assert!(
            blocks
                .pixels
                .chunks(4)
                .all(|pixel| pixel[0] == blocks.pixels[0])
        );
    }

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
