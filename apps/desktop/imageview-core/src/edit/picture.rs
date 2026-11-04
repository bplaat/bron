/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::path::Path;
use std::sync::Arc;

use image::{Bitmap, Compression, EncodeOptions, Format, LoopCount, Orientation};

use super::filter::for_rows;
use crate::media::premultiplied_bgra;
use crate::{Frame, Media, Raster, render_media};

/// The JPEG quality photos are saved with by default, which keeps them close to the original.
pub const DEFAULT_JPEG_QUALITY: u8 = 92;

/// An editable still image with straight-alpha RGBA8 pixels in sRGB.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Row-major RGBA8 pixels.
    pub pixels: Vec<u8>,
}

impl Picture {
    /// Creates a picture from tightly packed RGBA8 pixels.
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        debug_assert_eq!(pixels.len(), width as usize * height as usize * 4);
        Self {
            width,
            height,
            pixels,
        }
    }

    /// Takes a frame of a bitmap, or draws a vector at its natural size.
    pub fn from_media(media: &Media, frame: usize) -> Option<Self> {
        match media {
            Media::Raster(raster) => {
                let frame = raster.frames.get(frame).or(raster.frames.first())?;
                Some(Self::new(
                    raster.width,
                    raster.height,
                    straight_rgba(&frame.pixels),
                ))
            }
            Media::Vector(_) => Self::render(media, 1.0),
        }
    }

    /// Draws media at a scale of its natural size, vectors stay sharp at any scale.
    pub fn render(media: &Media, scale: f64) -> Option<Self> {
        let bitmap = render_media(media.clone(), scale)?;
        Some(Self::new(
            bitmap.width(),
            bitmap.height(),
            bitmap.data().to_vec(),
        ))
    }

    /// The picture as still media to draw.
    pub fn to_media(&self) -> Media {
        Media::Raster(Raster {
            width: self.width,
            height: self.height,
            frames: vec![Frame {
                pixels: Arc::from(premultiplied_bgra(&self.pixels)),
                delay: std::time::Duration::ZERO,
            }],
            loop_count: LoopCount::Finite(1),
        })
    }

    /// Whether every pixel is opaque, which formats without transparency need.
    pub fn is_opaque(&self) -> bool {
        self.pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 255)
    }

    // The pixel at a position.
    #[cfg(test)]
    pub(crate) fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let index = (y as usize * self.width as usize + x as usize) * 4;
        let mut pixel = [0; 4];
        pixel.copy_from_slice(&self.pixels[index..index + 4]);
        pixel
    }

    // Creates a picture of a size from the source pixel of every pixel.
    fn remap(
        &self,
        width: u32,
        height: u32,
        source: impl Fn(u32, u32) -> (u32, u32) + Sync,
    ) -> Self {
        let from = self.pixels.as_chunks::<4>().0;
        let mut pixels = vec![0; width as usize * height as usize * 4];
        for_rows(&mut pixels, width as usize * 4, |y, row| {
            for (x, pixel) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let (x, y) = source(x as u32, y as u32);
                *pixel = from[y as usize * self.width as usize + x as usize];
            }
        });
        Self::new(width, height, pixels)
    }

    /// Turns the picture a quarter counterclockwise.
    pub fn rotate_left(&self) -> Self {
        self.remap(self.height, self.width, |x, y| (self.width - 1 - y, x))
    }

    /// Turns the picture a quarter clockwise.
    pub fn rotate_right(&self) -> Self {
        self.remap(self.height, self.width, |x, y| (y, self.height - 1 - x))
    }

    /// Turns the picture by an orientation.
    pub fn orient(&self, orientation: Orientation) -> Self {
        match orientation {
            Orientation::Normal => self.clone(),
            Orientation::FlipHorizontal => self.flip_horizontal(),
            Orientation::Rotate180 => self.rotate_right().rotate_right(),
            Orientation::FlipVertical => self.flip_vertical(),
            Orientation::Transpose => self.rotate_left().flip_vertical(),
            Orientation::Rotate90 => self.rotate_right(),
            Orientation::Transverse => self.rotate_right().flip_vertical(),
            Orientation::Rotate270 => self.rotate_left(),
        }
    }

    /// Mirrors the picture left to right.
    pub fn flip_horizontal(&self) -> Self {
        self.remap(self.width, self.height, |x, y| (self.width - 1 - x, y))
    }

    /// Mirrors the picture top to bottom.
    pub fn flip_vertical(&self) -> Self {
        self.remap(self.width, self.height, |x, y| (x, self.height - 1 - y))
    }

    /// Keeps the part inside a rectangle, which is limited to the picture.
    pub fn crop(&self, x: u32, y: u32, width: u32, height: u32) -> Self {
        let x = x.min(self.width - 1);
        let y = y.min(self.height - 1);
        let width = width.clamp(1, self.width - x);
        let height = height.clamp(1, self.height - y);
        self.remap(width, height, |column, row| (x + column, y + row))
    }

    /// Copies another picture over this one at a position, as far as it fits.
    pub(crate) fn paste(&mut self, other: &Self, x: u32, y: u32) {
        let width = other.width.min(self.width.saturating_sub(x)) as usize * 4;
        for row in 0..other.height.min(self.height.saturating_sub(y)) {
            let from = row as usize * other.width as usize * 4;
            let to = ((y + row) as usize * self.width as usize + x as usize) * 4;
            self.pixels[to..to + width].copy_from_slice(&other.pixels[from..from + width]);
        }
    }

    /// Scales the picture to a size, averaging the pixels that shrink together and blending
    /// the ones that are enlarged.
    pub fn resize(&self, width: u32, height: u32) -> Self {
        let (width, height) = (width.max(1), height.max(1));
        let premultiplied = premultiply(&self.pixels);
        let premultiplied = resample(premultiplied, self.width, self.height, width, true);
        let premultiplied = resample(premultiplied, width, self.height, height, false);
        Self::new(width, height, unpremultiply(&premultiplied))
    }

    /// The picture shrunk to fit a length on its longest side, and the scale it is shown at.
    pub fn proxy(&self, longest: u32) -> (Self, f64) {
        let scale = (f64::from(longest) / f64::from(self.width.max(self.height))).min(1.0);
        if scale >= 1.0 {
            return (self.clone(), 1.0);
        }
        let size = |length: u32| ((f64::from(length) * scale).round() as u32).max(1);
        (self.resize(size(self.width), size(self.height)), scale)
    }

    /// Encodes the picture in the format of a file name extension as small as the encoder
    /// can, JPEG with a quality from 1 to 100.
    pub fn encode_for(&self, path: &Path, jpeg_quality: u8) -> Result<Vec<u8>, String> {
        let format = save_format(path).ok_or("ImageView cannot save this file type")?;
        let bitmap = Bitmap::new(self.width, self.height, self.pixels.clone())
            .map_err(|error| error.to_string())?;
        image::encode_with_options(
            &bitmap,
            format,
            EncodeOptions {
                compression: compression(self.width, self.height),
                jpeg_quality: jpeg_quality.clamp(1, 100),
                ..EncodeOptions::default()
            },
        )
        .map_err(|error| error.to_string())
    }
}

/// The formats pictures can be saved in, as a name and file name extension.
pub const SAVE_FORMATS: &[(&str, &str)] = &[
    ("BMP", "bmp"),
    ("GIF", "gif"),
    ("JPEG", "jpg"),
    ("PNG", "png"),
    ("QOI", "qoi"),
    ("TGA", "tga"),
];

// The most pixels that are encoded as small as possible. The smallest encoding tries many
// encodings and takes about 3 microseconds per pixel, a 1024x1024 picture about 3 seconds, while
// the normal one takes a 20th of that.
const MAX_COMPRESSION_PIXELS: u64 = 1024 * 1024;

// The encoding of a picture of a size: the smallest one for small pictures, which stays quick.
fn compression(width: u32, height: u32) -> Compression {
    if u64::from(width) * u64::from(height) <= MAX_COMPRESSION_PIXELS {
        Compression::Max
    } else {
        Compression::Normal
    }
}

/// The file name extension pictures are saved with when their own format cannot be saved.
pub const DEFAULT_SAVE_EXTENSION: &str = "png";

/// Whether a file of this name is saved as JPEG, which has a quality.
pub fn is_jpeg(path: &Path) -> bool {
    save_format(path) == Some(Format::Jpeg)
}

/// Whether a file of this name can be saved.
pub fn can_save(path: &Path) -> bool {
    save_format(path).is_some()
}

// The format a file is saved in, from its extension.
pub(super) fn save_format(path: &Path) -> Option<Format> {
    Format::from_extension(path.extension()?.to_str()?).filter(|format| format.can_encode())
}

// Converts premultiplied BGRA8 pixels to straight-alpha RGBA8 pixels.
fn straight_rgba(pixels: &[u8]) -> Vec<u8> {
    let mut output = pixels.to_vec();
    for pixel in output.as_chunks_mut::<4>().0 {
        let [blue, green, red, alpha] = *pixel;
        let unpremultiply = |value: u8| match alpha {
            0 => 0,
            255 => value,
            _ => {
                ((u16::from(value) * 255 + u16::from(alpha) / 2) / u16::from(alpha)).min(255) as u8
            }
        };
        *pixel = [
            unpremultiply(red),
            unpremultiply(green),
            unpremultiply(blue),
            alpha,
        ];
    }
    output
}

// The pixels of a block of the alpha conversions, which fan out over the workers.
const BLOCK: usize = 4096;

/// Converts straight-alpha RGBA8 pixels to premultiplied RGBA in [0, 1], so blending does
/// not bleed the colors of transparent pixels.
pub(crate) fn premultiply(pixels: &[u8]) -> Vec<f32> {
    let source = pixels.as_chunks::<4>().0;
    let mut output = vec![0.0; source.len() * 4];
    for_rows(&mut output, BLOCK * 4, |block, line| {
        let line = line.as_chunks_mut::<4>().0;
        for (pixel, out) in source[block * BLOCK..].iter().zip(line) {
            let alpha = f32::from(pixel[3]) / 255.0;
            *out = [
                f32::from(pixel[0]) / 255.0 * alpha,
                f32::from(pixel[1]) / 255.0 * alpha,
                f32::from(pixel[2]) / 255.0 * alpha,
                alpha,
            ];
        }
    });
    output
}

/// Rounds a sample in [0, 1] to a byte. Adding a half instead of calling `round` keeps the loops
/// around it vectorizable on x86 without SSE4.1, and the clamp spares them a saturating cast.
pub(crate) fn to_byte(value: f32) -> u8 {
    (value * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}

/// Converts premultiplied RGBA in [0, 1] back to straight-alpha RGBA8 pixels.
pub(crate) fn unpremultiply(pixels: &[f32]) -> Vec<u8> {
    let source = pixels.as_chunks::<4>().0;
    let mut output = vec![0; source.len() * 4];
    for_rows(&mut output, BLOCK * 4, |block, line| {
        let line = line.as_chunks_mut::<4>().0;
        for (&[red, green, blue, alpha], out) in source[block * BLOCK..].iter().zip(line) {
            *out = if alpha <= 0.0 {
                [0; 4]
            } else {
                [
                    to_byte(red / alpha),
                    to_byte(green / alpha),
                    to_byte(blue / alpha),
                    to_byte(alpha),
                ]
            };
        }
    });
    output
}

// Resamples premultiplied pixels along one axis with a tent filter, widened when shrinking so
// every source pixel counts.
fn resample(pixels: Vec<f32>, width: u32, height: u32, length: u32, horizontal: bool) -> Vec<f32> {
    let (width, height, length) = (width as usize, height as usize, length as usize);
    let source = if horizontal { width } else { height };
    if source == length {
        return pixels;
    }
    let scale = source as f64 / length as f64;
    let support = scale.max(1.0);
    // The source pixels and their weights for every destination pixel.
    let taps: Vec<Vec<(usize, f32)>> = (0..length)
        .map(|index| {
            let center = (index as f64 + 0.5) * scale;
            let first = (center - support).floor().max(0.0) as usize;
            let last = ((center + support).ceil() as usize).min(source);
            let mut taps: Vec<(usize, f32)> = (first..last)
                .map(|tap| {
                    let distance = ((tap as f64 + 0.5 - center) / support).abs();
                    (tap, (1.0 - distance).max(0.0) as f32)
                })
                .filter(|(_, weight)| *weight > 0.0)
                .collect();
            if taps.is_empty() {
                taps.push(((center as usize).min(source - 1), 1.0));
            }
            let total: f32 = taps.iter().map(|(_, weight)| weight).sum();
            taps.iter_mut().for_each(|(_, weight)| *weight /= total);
            taps
        })
        .collect();
    let (out_width, out_height) = if horizontal {
        (length, height)
    } else {
        (width, length)
    };
    let mut output = vec![0.0; out_width * out_height * 4];
    if !horizontal {
        // Accumulate whole rows so the inner loop has contiguous independent samples.
        // Keep the tap order for each sample to preserve floating-point rounding.
        for_rows(&mut output, width * 4, |y, row| {
            for &(tap, weight) in &taps[y] {
                let source = &pixels[tap * width * 4..][..width * 4];
                for (out, &value) in row.iter_mut().zip(source) {
                    *out += value * weight;
                }
            }
        });
        return output;
    }
    for_rows(&mut output, length * 4, |y, row| {
        resample_row(
            pixels[y * width * 4..][..width * 4].as_chunks::<4>().0,
            row.as_chunks_mut::<4>().0,
            &taps,
        );
    });
    output
}

// Arguments tell the compiler that the slices don't overlap, which it needs to use SIMD for the
// channels of a pixel.
fn resample_row(source: &[[f32; 4]], row: &mut [[f32; 4]], taps: &[Vec<(usize, f32)>]) {
    for (pixel, taps) in row.iter_mut().zip(taps) {
        for &(tap, weight) in taps {
            let sample = &source[tap];
            for channel in 0..4 {
                pixel[channel] += sample[channel] * weight;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_conversion_matches_scalar_reference() {
        let pixels: Vec<u8> = (0..=255u8)
            .flat_map(|alpha| {
                (0..=255u8).flat_map(move |value| [value, 255 - value, value / 2, alpha])
            })
            .collect();
        let expected: Vec<f32> = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|pixel| {
                let alpha = f32::from(pixel[3]) / 255.0;
                [
                    f32::from(pixel[0]) / 255.0 * alpha,
                    f32::from(pixel[1]) / 255.0 * alpha,
                    f32::from(pixel[2]) / 255.0 * alpha,
                    alpha,
                ]
            })
            .collect();
        assert_eq!(premultiply(&pixels), expected);
        let expected: Vec<u8> = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|pixel| if pixel[3] == 0 { [0; 4] } else { *pixel })
            .collect();
        assert_eq!(unpremultiply(&premultiply(&pixels)), expected);
    }

    #[test]
    fn resampling_matches_scalar_tent_filter() {
        // An independent pixel-at-a-time reference covers nonintegral scales and edges.
        fn reference(
            pixels: &[f32],
            width: usize,
            height: usize,
            length: usize,
            horizontal: bool,
        ) -> Vec<f32> {
            let source = if horizontal { width } else { height };
            let (out_width, out_height) = if horizontal {
                (length, height)
            } else {
                (width, length)
            };
            let scale = source as f64 / length as f64;
            let support = scale.max(1.0);
            let mut output = Vec::new();
            for y in 0..out_height {
                for x in 0..out_width {
                    let center = (if horizontal { x } else { y }) as f64 * scale + 0.5 * scale;
                    let mut taps = Vec::new();
                    for tap in 0..source {
                        let weight =
                            (1.0 - ((tap as f64 + 0.5 - center) / support).abs()).max(0.0) as f32;
                        if weight > 0.0 {
                            taps.push((tap, weight));
                        }
                    }
                    let total: f32 = taps.iter().map(|(_, weight)| weight).sum();
                    let mut pixel = [0.0; 4];
                    for (tap, weight) in taps {
                        let index = if horizontal {
                            y * width + tap
                        } else {
                            tap * width + x
                        };
                        for channel in 0..4 {
                            pixel[channel] += pixels[index * 4 + channel] * (weight / total);
                        }
                    }
                    output.extend(pixel);
                }
            }
            output
        }

        for (width, height) in [(1, 1), (3, 5), (17, 9)] {
            let pixels: Vec<f32> = (0..width * height * 4)
                .map(|index| (index * 37 % 256) as f32 / 255.0)
                .collect();
            for length in [1, 2, 3, 5, 7, 9, 17, 19] {
                for horizontal in [false, true] {
                    let actual = resample(
                        pixels.clone(),
                        width as u32,
                        height as u32,
                        length as u32,
                        horizontal,
                    );
                    let expected = reference(&pixels, width, height, length, horizontal);
                    assert_eq!(
                        actual, expected,
                        "{width}x{height}, length={length}, horizontal={horizontal}"
                    );
                }
            }
        }
    }

    #[test]
    fn small_pictures_are_encoded_smallest() {
        assert_eq!(compression(1024, 1024), Compression::Max);
        assert_eq!(compression(1, 1024 * 1024), Compression::Max);
        assert_eq!(compression(1025, 1024), Compression::Normal);
        assert_eq!(compression(u32::MAX, u32::MAX), Compression::Normal);
    }

    // A 2x1 picture of a red and a blue pixel.
    fn picture() -> Picture {
        Picture::new(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 255])
    }

    #[test]
    fn media_round_trips() {
        let picture = Picture::new(2, 1, vec![255, 128, 0, 128, 10, 20, 30, 255]);
        let back = Picture::from_media(&picture.to_media(), 0).expect("raster");
        assert_eq!(back.width, 2);
        assert_eq!(back.pixels[4..], picture.pixels[4..]);
        assert!(
            back.pixels[..4]
                .iter()
                .zip(&picture.pixels[..4])
                .all(|(back, original)| back.abs_diff(*original) <= 1)
        );
    }

    #[test]
    fn rotations_and_flips_move_pixels() {
        let picture = picture();
        let left = picture.rotate_left();
        assert_eq!((left.width, left.height), (1, 2));
        assert_eq!(left.pixel(0, 0), [0, 0, 255, 255]);
        assert_eq!(picture.rotate_right().pixel(0, 0), [255, 0, 0, 255]);
        assert_eq!(picture.flip_horizontal().pixel(0, 0), [0, 0, 255, 255]);
        assert_eq!(picture.flip_vertical(), picture);
        assert_eq!(left.rotate_right(), picture);
    }

    #[test]
    fn orientations_turn_like_their_combinations() {
        use Orientation::*;
        let picture = Picture::new(3, 2, (0..24).collect());
        let all = [
            Normal,
            FlipHorizontal,
            Rotate180,
            FlipVertical,
            Transpose,
            Rotate90,
            Transverse,
            Rotate270,
        ];
        for first in all {
            for second in all {
                assert_eq!(
                    picture.orient(first.then(second)),
                    picture.orient(first).orient(second),
                    "{first:?} then {second:?}"
                );
            }
        }
    }

    #[test]
    fn opacity_is_found() {
        assert!(picture().is_opaque());
        assert!(!Picture::new(1, 1, vec![0, 0, 0, 254]).is_opaque());
    }

    #[test]
    fn crops_stay_inside_the_picture() {
        let cropped = picture().crop(1, 0, 10, 10);
        assert_eq!((cropped.width, cropped.height), (1, 1));
        assert_eq!(cropped.pixel(0, 0), [0, 0, 255, 255]);
    }

    #[test]
    fn resizing_averages_and_keeps_solid_colors() {
        let shrunk = picture().resize(1, 1);
        assert_eq!(shrunk.pixel(0, 0), [128, 0, 128, 255]);
        let solid = Picture::new(1, 1, vec![10, 20, 30, 255]).resize(3, 2);
        assert_eq!((solid.width, solid.height), (3, 2));
        assert!(
            solid
                .pixels
                .chunks(4)
                .all(|pixel| pixel == [10, 20, 30, 255])
        );
    }

    #[test]
    fn saving_follows_the_extension() {
        assert!(can_save(Path::new("a.JPG")));
        assert!(!can_save(Path::new("a.svg")));
        // JPEG has no transparency.
        let transparent = Picture::new(1, 1, vec![0, 0, 0, 0]);
        assert!(
            transparent
                .encode_for(Path::new("a.jpg"), DEFAULT_JPEG_QUALITY)
                .is_err()
        );
        let png = picture()
            .encode_for(Path::new("a.png"), DEFAULT_JPEG_QUALITY)
            .expect("png");
        assert_eq!(
            image::decode(&png).expect("decode").pixels(),
            picture().pixels
        );
        assert!(
            picture()
                .encode_for(Path::new("a.svg"), DEFAULT_JPEG_QUALITY)
                .is_err()
        );
        assert!(is_jpeg(Path::new("a.JPEG")) && !is_jpeg(Path::new("a.png")));
        assert!(!can_save(Path::new("a.ico")));
        assert!(SAVE_FORMATS.is_sorted());
        assert!(
            SAVE_FORMATS
                .iter()
                .all(|(_, extension)| can_save(Path::new(&format!("a.{extension}"))))
        );
        // A lower quality makes a smaller file.
        let noise = Picture::new(
            64,
            64,
            (0..64 * 64 * 4)
                .map(|index| {
                    if index % 4 == 3 {
                        255
                    } else {
                        (index * 7919 % 251) as u8
                    }
                })
                .collect(),
        );
        let small = noise.encode_for(Path::new("a.jpg"), 20).expect("jpeg");
        let large = noise.encode_for(Path::new("a.jpg"), 95).expect("jpeg");
        assert!(small.len() < large.len());
    }
}
