/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]
#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};
use core::time::Duration;

mod encoder;
pub use encoder::{
    EncodeError, EncodeOptions, EncodingStyle, encode, encode_animation,
    encode_animation_with_options, encode_with_options,
};

#[cfg(feature = "bmp")]
mod bmp;
#[cfg(feature = "gif")]
mod gif;
#[cfg(feature = "ico")]
mod ico;
#[cfg(feature = "jpeg")]
mod jpeg;
#[cfg(feature = "png")]
mod png;
#[cfg(feature = "qoi")]
mod qoi;
#[cfg(all(
    test,
    any(
        feature = "qoi",
        feature = "jpeg",
        feature = "png",
        feature = "gif",
        feature = "bmp",
        feature = "ico"
    )
))]
mod test_support;

/// The encoded format detected from an image's file signature.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    /// Quite OK Image.
    #[cfg(feature = "qoi")]
    Qoi,
    /// JPEG.
    #[cfg(feature = "jpeg")]
    Jpeg,
    /// PNG, including APNG animations.
    #[cfg(feature = "png")]
    Png,
    /// Graphics Interchange Format.
    #[cfg(feature = "gif")]
    Gif,
    /// Windows or OS/2 bitmap.
    #[cfg(feature = "bmp")]
    Bmp,
    /// Windows icon.
    #[cfg(feature = "ico")]
    Ico,
}

/// The interpretation of decoded color channels. Alpha is always linear.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColorSpace {
    /// Standard RGB.
    Srgb,
    /// Linear extended sRGB.
    LinearSrgb,
}

/// The number of times an animation is played.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoopCount {
    /// The animation repeats indefinitely.
    Infinite,
    /// The animation plays this many times in total.
    Finite(u32),
}

/// A row-major, straight-alpha RGBA8 bitmap.
#[derive(Debug, Eq, PartialEq)]
pub struct Bitmap {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl Bitmap {
    /// Creates a bitmap from tightly packed RGBA8 pixels.
    pub fn new(width: u32, height: u32, data: Vec<u8>) -> core::result::Result<Self, EncodeError> {
        encoder::validate_pixels(width, height, &data)?;
        Ok(Self {
            width,
            height,
            data,
        })
    }

    /// Returns the width in pixels.
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Returns the height in pixels.
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Returns the row-major, straight-alpha RGBA8 pixels.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Takes the row-major, straight-alpha RGBA8 pixels without copying them.
    pub fn into_data(self) -> Vec<u8> {
        self.data
    }
}

/// A complete displayed frame, after animation blending and before disposal.
#[derive(Debug, Eq, PartialEq)]
pub struct Frame {
    bitmap: Bitmap,
    delay: Duration,
}

impl Frame {
    /// Creates a displayed frame with its duration.
    pub const fn new(bitmap: Bitmap, delay: Duration) -> Self {
        Self { bitmap, delay }
    }

    /// Returns the complete displayed bitmap.
    pub const fn bitmap(&self) -> &Bitmap {
        &self.bitmap
    }

    /// Returns canvas-sized, row-major, straight-alpha RGBA8 pixels.
    pub fn pixels(&self) -> &[u8] {
        self.bitmap.data()
    }

    /// Takes the complete displayed bitmap without copying it.
    pub fn into_bitmap(self) -> Bitmap {
        self.bitmap
    }

    /// Returns the encoded display duration; static images have zero duration.
    pub const fn delay(&self) -> Duration {
        self.delay
    }
}

/// A decoded image. JPEG orientation is already applied to dimensions and pixels.
#[derive(Debug, Eq, PartialEq)]
pub struct Image {
    format: Format,
    color_space: ColorSpace,
    frames: Vec<Frame>,
    is_animated: bool,
    loop_count: LoopCount,
}

impl Image {
    /// Encodes the image in the requested raster format.
    pub fn encode(&self, format: Format) -> core::result::Result<Vec<u8>, EncodeError> {
        self.encode_with_options(format, EncodeOptions::default())
    }

    /// Encodes the image in the requested raster format with options.
    pub fn encode_with_options(
        &self,
        format: Format,
        options: EncodeOptions,
    ) -> core::result::Result<Vec<u8>, EncodeError> {
        if self.is_animated {
            encode_animation_with_options(format, &self.frames, self.loop_count, options)
        } else {
            encode_with_options(&self.frames[0].bitmap, format, options)
        }
    }

    /// Returns the encoded format.
    pub const fn format(&self) -> Format {
        self.format
    }

    /// Returns the displayed canvas width in pixels.
    pub fn width(&self) -> u32 {
        self.frames[0].bitmap.width()
    }

    /// Returns the displayed canvas height in pixels.
    pub fn height(&self) -> u32 {
        self.frames[0].bitmap.height()
    }

    /// Returns the interpretation of the decoded color channels.
    pub const fn color_space(&self) -> ColorSpace {
        self.color_space
    }

    /// Returns one or more fully composited frames.
    pub fn frames(&self) -> &[Frame] {
        &self.frames
    }

    /// Takes the fully composited frames without copying their pixels.
    pub fn into_frames(self) -> Vec<Frame> {
        self.frames
    }

    /// Returns the first displayed frame's RGBA8 pixels.
    pub fn pixels(&self) -> &[u8] {
        self.frames[0].pixels()
    }

    /// Returns whether the image has animation frames or playback metadata.
    pub const fn is_animated(&self) -> bool {
        self.is_animated
    }

    /// Returns the total animation plays.
    pub const fn loop_count(&self) -> LoopCount {
        self.loop_count
    }

    #[cfg(any(feature = "qoi", feature = "jpeg", feature = "png", feature = "bmp"))]
    fn still(format: Format, width: u32, height: u32, pixels: Vec<u8>) -> Self {
        Self {
            format,
            color_space: ColorSpace::Srgb,
            frames: alloc::vec![Frame {
                bitmap: Bitmap {
                    width,
                    height,
                    data: pixels,
                },
                delay: Duration::ZERO,
            }],
            is_animated: false,
            loop_count: LoopCount::Finite(1),
        }
    }
}

/// A decoding failure. Invalid input never intentionally panics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeError {
    /// No supported file signature was found.
    InvalidMagic,
    /// Invalid image dimensions or header fields.
    InvalidHeader,
    /// Truncated, corrupt or inconsistent encoded data.
    InvalidData,
    /// A recognized format uses an unsupported feature.
    UnsupportedFeature,
    /// Decoded storage exceeds 1 GiB, arithmetic overflows, or allocation fails.
    ImageTooLarge,
}

impl Display for DecodeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidMagic => "unsupported image format",
            Self::InvalidHeader => "invalid image header",
            Self::InvalidData => "invalid or truncated image data",
            Self::UnsupportedFeature => "unsupported image feature",
            Self::ImageTooLarge => "image exceeds decoding resource limits",
        })
    }
}

impl core::error::Error for DecodeError {}

type Result<T> = core::result::Result<T, DecodeError>;

/// Decodes a complete image within 1 GiB, excluding uncommon JPEG/BMP variants.
#[cfg_attr(
    not(any(
        feature = "qoi",
        feature = "jpeg",
        feature = "png",
        feature = "gif",
        feature = "bmp"
    )),
    allow(clippy::missing_const_for_fn)
)]
pub fn decode(_data: &[u8]) -> Result<Image> {
    #[cfg(feature = "qoi")]
    if _data.starts_with(b"qoif") {
        return qoi::decode(_data);
    }
    #[cfg(feature = "jpeg")]
    if _data.starts_with(b"\xff\xd8") {
        return jpeg::decode(_data);
    }
    #[cfg(feature = "png")]
    if _data.starts_with(b"\x89PNG\r\n\x1a\n") {
        return png::decode(_data);
    }
    #[cfg(feature = "gif")]
    if _data.starts_with(b"GIF87a") || _data.starts_with(b"GIF89a") {
        return gif::decode(_data);
    }
    #[cfg(feature = "bmp")]
    if _data.starts_with(b"BM") {
        return bmp::decode(_data);
    }
    #[cfg(feature = "bmp")]
    if let Some(length) = _data.first_chunk::<4>()
        && matches!(
            u32::from_le_bytes(*length),
            12 | 40 | 52 | 56 | 64 | 108 | 124
        )
    {
        return bmp::decode_dib(_data);
    }
    #[cfg(feature = "ico")]
    if _data.starts_with(b"\0\0\x01\0") {
        return ico::decode(_data);
    }
    Err(DecodeError::InvalidMagic)
}

#[cfg(any(
    feature = "qoi",
    feature = "jpeg",
    feature = "png",
    feature = "gif",
    feature = "bmp",
    feature = "ico"
))]
const MAX_BYTES: usize = 1024 * 1024 * 1024;

// Count cumulative allocations conservatively, including buffers later discarded.
#[derive(Default)]
#[cfg(any(
    feature = "qoi",
    feature = "jpeg",
    feature = "png",
    feature = "gif",
    feature = "bmp",
    feature = "ico"
))]
struct Budget {
    used: usize,
}

#[cfg(any(
    feature = "qoi",
    feature = "jpeg",
    feature = "png",
    feature = "gif",
    feature = "bmp"
))]
impl Budget {
    fn claim(&mut self, bytes: usize) -> Result<()> {
        self.used = self
            .used
            .checked_add(bytes)
            .filter(|n| *n <= MAX_BYTES)
            .ok_or(DecodeError::ImageTooLarge)?;
        Ok(())
    }

    #[cfg(any(
        feature = "qoi",
        feature = "jpeg",
        feature = "png",
        feature = "gif",
        feature = "bmp"
    ))]
    fn zeroed<T: Default + Clone>(&mut self, len: usize) -> Result<Vec<T>> {
        self.claim(
            len.checked_mul(size_of::<T>())
                .ok_or(DecodeError::ImageTooLarge)?,
        )?;
        let mut out = Vec::new();
        out.try_reserve_exact(len)
            .map_err(|_| DecodeError::ImageTooLarge)?;
        out.resize(len, T::default());
        Ok(out)
    }

    #[cfg(any(feature = "png", feature = "gif"))]
    fn copy(&mut self, bytes: &[u8]) -> Result<Vec<u8>> {
        self.claim(bytes.len())?;
        let mut out = Vec::new();
        out.try_reserve_exact(bytes.len())
            .map_err(|_| DecodeError::ImageTooLarge)?;
        out.extend_from_slice(bytes);
        Ok(out)
    }

    #[cfg(any(feature = "png", feature = "gif"))]
    fn append<T: Copy>(&mut self, out: &mut Vec<T>, bytes: &[T]) -> Result<()> {
        self.claim(size_of_val(bytes))?;
        out.try_reserve(bytes.len())
            .map_err(|_| DecodeError::ImageTooLarge)?;
        out.extend_from_slice(bytes);
        Ok(())
    }

    #[cfg(any(feature = "png", feature = "gif"))]
    fn frame(
        &mut self,
        frames: &mut Vec<Frame>,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
        delay: Duration,
    ) -> Result<()> {
        self.claim(size_of::<Frame>())?;
        frames
            .try_reserve(1)
            .map_err(|_| DecodeError::ImageTooLarge)?;
        frames.push(Frame {
            bitmap: Bitmap {
                width,
                height,
                data: pixels,
            },
            delay,
        });
        Ok(())
    }
}

#[cfg(any(feature = "jpeg", feature = "png", feature = "gif", feature = "bmp"))]
fn pixel_len(width: u32, height: u32) -> Result<usize> {
    if width == 0 || height == 0 {
        return Err(DecodeError::InvalidHeader);
    }
    usize::try_from(
        (u64::from(width) * u64::from(height))
            .checked_mul(4)
            .ok_or(DecodeError::ImageTooLarge)?,
    )
    .ok()
    .filter(|n| *n <= MAX_BYTES)
    .ok_or(DecodeError::ImageTooLarge)
}

#[cfg(any(feature = "jpeg", feature = "png", feature = "gif", feature = "bmp"))]
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

#[cfg(any(feature = "jpeg", feature = "png", feature = "gif", feature = "bmp"))]
impl<'a> Reader<'a> {
    const fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(len).ok_or(DecodeError::InvalidData)?;
        let bytes = self
            .data
            .get(self.pos..end)
            .ok_or(DecodeError::InvalidData)?;
        self.pos = end;
        Ok(bytes)
    }

    fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    #[cfg(any(feature = "jpeg", feature = "png"))]
    fn be16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().expect("two bytes"),
        ))
    }

    #[cfg(feature = "png")]
    fn be32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }

    #[cfg(any(feature = "gif", feature = "bmp"))]
    fn le16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().expect("two bytes"),
        ))
    }

    #[cfg(feature = "bmp")]
    fn le32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }

    #[cfg(any(feature = "jpeg", feature = "png", feature = "bmp"))]
    const fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }
}

#[derive(Clone, Copy)]
#[cfg(any(feature = "png", feature = "gif"))]
struct Area {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}

#[cfg(any(feature = "png", feature = "gif"))]
impl Area {
    fn validate(self, width: u32, height: u32) -> Result<Self> {
        if self.width == 0
            || self.height == 0
            || self
                .x
                .checked_add(self.width)
                .is_none_or(|n| n > width as usize)
            || self
                .y
                .checked_add(self.height)
                .is_none_or(|n| n > height as usize)
        {
            return Err(DecodeError::InvalidData);
        }
        Ok(self)
    }

    fn clear(self, canvas: &mut [u8], stride: usize, pixel: [u8; 4]) {
        for y in self.y..self.y + self.height {
            canvas[(y * stride + self.x) * 4..(y * stride + self.x + self.width) * 4]
                .as_chunks_mut::<4>()
                .0
                .fill(pixel);
        }
    }

    fn snapshot(self, canvas: &[u8], stride: usize, budget: &mut Budget) -> Result<Vec<u8>> {
        let row_len = self
            .width
            .checked_mul(4)
            .ok_or(DecodeError::ImageTooLarge)?;
        let len = row_len
            .checked_mul(self.height)
            .ok_or(DecodeError::ImageTooLarge)?;
        budget.claim(len)?;
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(len)
            .map_err(|_| DecodeError::ImageTooLarge)?;
        for row in 0..self.height {
            let start = ((self.y + row) * stride + self.x) * 4;
            pixels.extend_from_slice(&canvas[start..start + row_len]);
        }
        Ok(pixels)
    }

    fn restore(self, canvas: &mut [u8], stride: usize, pixels: &[u8]) {
        let row_len = self.width * 4;
        for (row, source) in pixels.chunks_exact(row_len).enumerate() {
            let start = ((self.y + row) * stride + self.x) * 4;
            canvas[start..start + row_len].copy_from_slice(source);
        }
    }
}

// MARK: Rounding
// FIXME: Use `f32::round`, `f32::trunc` and `f64::round` once the `core_float_math` feature
// brings them to `core` on stable Rust.
// `core` has no float rounding; codecs only round values far inside the `i64` range, where
// truncation is an integer cast.

/// Rounds toward zero, like `f32::trunc`.
#[cfg(feature = "jpeg")]
const fn truncf(x: f32) -> f32 {
    x as i64 as f32
}

/// Rounds half away from zero, like `f32::round`.
#[cfg(feature = "jpeg")]
fn roundf(x: f32) -> f32 {
    let truncated = truncf(x);
    if (x - truncated).abs() >= 0.5 {
        truncated + 1f32.copysign(x)
    } else {
        truncated
    }
}

/// Rounds half away from zero, like `f64::round`.
#[cfg(feature = "gif")]
fn round(x: f64) -> f64 {
    let truncated = x as i64 as f64;
    if (x - truncated).abs() >= 0.5 {
        truncated + 1f64.copysign(x)
    } else {
        truncated
    }
}

// MARK: Tests
#[cfg(all(
    test,
    any(
        feature = "qoi",
        feature = "jpeg",
        feature = "png",
        feature = "gif",
        feature = "bmp",
        feature = "ico"
    )
))]
mod tests {
    use super::*;

    #[cfg(feature = "png")]
    #[test]
    fn owned_accessors_take_pixels_without_copying() {
        let bitmap = Bitmap::new(1, 1, vec![1, 2, 3, 4]).unwrap();
        let pixels = bitmap.data().as_ptr();
        let image = Image {
            format: Format::Png,
            color_space: ColorSpace::Srgb,
            frames: vec![Frame::new(bitmap, Duration::ZERO)],
            is_animated: false,
            loop_count: LoopCount::Infinite,
        };
        let data = image.into_frames().remove(0).into_bitmap().into_data();
        assert_eq!(
            (data.as_ptr(), data.as_slice()),
            (pixels, [1, 2, 3, 4].as_slice())
        );
    }

    #[cfg(any(feature = "jpeg", feature = "gif"))]
    #[test]
    fn rounding_matches_std() {
        // Exact halves, values beside them, and signs of both.
        let values = (-2000..=2000).flat_map(|i| {
            let half = f64::from(i) / 2.0;
            [half, half + 1e-6, half - 1e-6, f64::from(i) / 7.0]
        });
        for value in values {
            #[cfg(feature = "gif")]
            assert_eq!(round(value), value.round(), "{value}");
            #[cfg(feature = "jpeg")]
            {
                let value = value as f32;
                assert_eq!(roundf(value), value.round(), "{value}");
                assert_eq!(truncf(value), value.trunc(), "{value}");
            }
        }
    }

    #[cfg(feature = "qoi")]
    #[test]
    fn qoi_reference_corpus() {
        test_support::run_raster_format("qoi");
    }

    #[cfg(feature = "jpeg")]
    #[test]
    fn jpeg_reference_corpus() {
        test_support::run_raster_format("jpeg");
    }

    #[cfg(feature = "png")]
    #[test]
    fn png_reference_corpus() {
        test_support::run_raster_format("png");
    }

    #[cfg(feature = "gif")]
    #[test]
    fn gif_reference_corpus() {
        test_support::run_raster_format("gif");
    }

    #[cfg(feature = "bmp")]
    #[test]
    fn bmp_reference_corpus() {
        test_support::run_raster_format("bmp");
    }

    #[cfg(feature = "ico")]
    #[test]
    fn ico_reference_corpus() {
        test_support::run_raster_format("ico");
    }
    #[cfg(any(
        feature = "qoi",
        feature = "jpeg",
        feature = "png",
        feature = "gif",
        feature = "bmp",
        feature = "ico"
    ))]
    #[test]
    fn every_truncation_is_rejected() {
        for (name, encoded) in test_support::raster_inputs() {
            for end in 0..encoded.len().min(32) {
                assert!(
                    decode(&encoded[..end]).is_err(),
                    "{} at {end}",
                    name.to_string_lossy()
                );
            }
        }
    }

    #[cfg(any(
        feature = "qoi",
        feature = "jpeg",
        feature = "png",
        feature = "gif",
        feature = "bmp",
        feature = "ico"
    ))]
    #[test]
    fn corrupted_inputs_do_not_panic() {
        for (_, encoded) in test_support::raster_inputs() {
            for i in (0..encoded.len()).step_by(7) {
                let mut bad = encoded.clone();
                bad[i] ^= 0xff;
                let _ = decode(&bad);
            }
        }
        let mut state = 1234567u32;
        for len in 0..512 {
            let mut bytes = vec![0; len];
            for b in &mut bytes {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                *b = state as u8;
            }
            for signature in [
                &b"\xff\xd8"[..],
                b"GIF89a",
                b"BM",
                b"\0\0\x01\0",
                b"qoif",
                b"\x89PNG\r\n\x1a\n",
            ] {
                let mut input = signature.to_vec();
                input.extend_from_slice(&bytes);
                let _ = decode(&input);
            }
        }
    }

    #[cfg(any(feature = "png", feature = "gif"))]
    #[test]
    fn animation_snapshot_only_stores_the_changed_area() {
        let area = Area {
            x: 1,
            y: 1,
            width: 1,
            height: 1,
        };
        let mut canvas = (0..24).collect::<Vec<_>>();
        let original = canvas.clone();
        let mut budget = Budget::default();
        let snapshot = area
            .snapshot(&canvas, 3, &mut budget)
            .expect("area snapshot");
        assert_eq!(snapshot.len(), 4);
        canvas[16..20].fill(255);
        area.restore(&mut canvas, 3, &snapshot);
        assert_eq!(canvas, original);
    }
}
