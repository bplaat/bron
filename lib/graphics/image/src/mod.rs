/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]
#![cfg_attr(not(any(test, feature = "std")), no_std)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};
use core::time::Duration;

mod math;

#[cfg(any(feature = "jpeg", feature = "webp"))]
mod metadata;

#[cfg(feature = "gif")]
use math::round;
#[cfg(feature = "jpeg")]
use math::{roundf, truncf};

mod encoder;
pub use color::{linear_to_srgb, srgb_to_linear};
pub use encoder::{
    Compression, EncodeError, EncodeOptions, encode, encode_animation,
    encode_animation_with_options, encode_with_options,
};
#[cfg(any(feature = "jpeg", feature = "webp"))]
pub use metadata::Orientation;
#[cfg(feature = "jpeg")]
pub use metadata::{copy_jpeg_exif, reorient_jpeg};

#[cfg(feature = "bmp")]
mod bmp;
mod color;
#[cfg(feature = "gif")]
mod gif;
#[cfg(feature = "icns")]
mod icns;
#[cfg(feature = "ico")]
mod ico;
#[cfg(feature = "jpeg")]
mod jpeg;
#[cfg(feature = "jpeg2000")]
mod jpeg2000;
#[cfg(feature = "png")]
mod png;
#[cfg(feature = "pnm")]
mod pnm;
#[cfg(feature = "qoi")]
mod qoi;
#[cfg(all(
    test,
    any(
        feature = "bmp",
        feature = "gif",
        feature = "ico",
        feature = "jpeg",
        feature = "png",
        feature = "qoi",
        feature = "tga"
    )
))]
mod test_support;
#[cfg(feature = "tga")]
mod tga;
#[cfg(feature = "webp")]
mod webp;

/// The encoded format detected from an image's file signature.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    /// Windows or OS/2 bitmap.
    #[cfg(feature = "bmp")]
    Bmp,
    /// Graphics Interchange Format.
    #[cfg(feature = "gif")]
    Gif,
    /// Apple icon.
    #[cfg(feature = "icns")]
    Icns,
    /// Windows icon.
    #[cfg(feature = "ico")]
    Ico,
    /// JPEG.
    #[cfg(feature = "jpeg")]
    Jpeg,
    /// JPEG 2000 in a JP2 container or a raw codestream.
    #[cfg(feature = "jpeg2000")]
    Jpeg2000,
    /// PNG, including APNG animations.
    #[cfg(feature = "png")]
    Png,
    /// Netpbm PBM, PGM, PPM, or PAM.
    #[cfg(feature = "pnm")]
    Pnm,
    /// Quite OK Image.
    #[cfg(feature = "qoi")]
    Qoi,
    /// Truevision TGA.
    #[cfg(feature = "tga")]
    Tga,
    /// WebP, including lossy, lossless and animated images.
    #[cfg(feature = "webp")]
    WebP,
}

impl Format {
    /// The format of a file name extension, ignoring case.
    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            #[cfg(feature = "bmp")]
            "bmp" | "dib" => Some(Self::Bmp),
            #[cfg(feature = "gif")]
            "gif" => Some(Self::Gif),
            #[cfg(feature = "ico")]
            "ico" => Some(Self::Ico),
            #[cfg(feature = "icns")]
            "icns" => Some(Self::Icns),
            #[cfg(feature = "jpeg")]
            "jpg" | "jpeg" | "jpe" | "jfif" => Some(Self::Jpeg),
            #[cfg(feature = "jpeg2000")]
            "j2c" | "j2k" | "jp2" | "jpc" => Some(Self::Jpeg2000),
            #[cfg(feature = "png")]
            "png" | "apng" => Some(Self::Png),
            #[cfg(feature = "pnm")]
            "pbm" | "pgm" | "ppm" | "pnm" | "pam" => Some(Self::Pnm),
            #[cfg(feature = "qoi")]
            "qoi" => Some(Self::Qoi),
            #[cfg(feature = "tga")]
            "tga" => Some(Self::Tga),
            #[cfg(feature = "webp")]
            "webp" => Some(Self::WebP),
            _ => None,
        }
    }

    /// The format of encoded bytes from their signature. TGA files and headerless DIB bitmaps
    /// have none.
    pub const fn from_signature(data: &[u8]) -> Option<Self> {
        match data {
            #[cfg(feature = "bmp")]
            [b'B', b'M', ..] => Some(Self::Bmp),
            #[cfg(feature = "gif")]
            [b'G', b'I', b'F', b'8', b'7' | b'9', b'a', ..] => Some(Self::Gif),
            #[cfg(feature = "ico")]
            [0, 0, 1, 0, ..] => Some(Self::Ico),
            #[cfg(feature = "icns")]
            [b'i', b'c', b'n', b's', ..] => Some(Self::Icns),
            #[cfg(feature = "webp")]
            [
                b'R',
                b'I',
                b'F',
                b'F',
                _,
                _,
                _,
                _,
                b'W',
                b'E',
                b'B',
                b'P',
                ..,
            ] => Some(Self::WebP),
            #[cfg(feature = "jpeg")]
            [0xff, 0xd8, ..] => Some(Self::Jpeg),
            #[cfg(feature = "jpeg2000")]
            [0, 0, 0, 12, b'j', b'P', b' ', b' ', 13, 10, 0x87, 10, ..]
            | [0xff, 0x4f, 0xff, 0x51, ..] => Some(Self::Jpeg2000),
            #[cfg(feature = "png")]
            [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n', ..] => Some(Self::Png),
            #[cfg(feature = "pnm")]
            [
                b'P',
                b'1'..=b'7',
                b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c,
                ..,
            ] => Some(Self::Pnm),
            #[cfg(feature = "qoi")]
            [b'q', b'o', b'i', b'f', ..] => Some(Self::Qoi),
            _ => None,
        }
    }

    /// Whether images can be encoded in the format.
    pub const fn can_encode(self) -> bool {
        #[cfg(feature = "ico")]
        if matches!(self, Self::Ico) {
            return false;
        }
        #[cfg(feature = "icns")]
        if matches!(self, Self::Icns) {
            return false;
        }
        #[cfg(feature = "jpeg2000")]
        if matches!(self, Self::Jpeg2000) {
            return false;
        }
        #[cfg(feature = "pnm")]
        if matches!(self, Self::Pnm) {
            return false;
        }
        #[cfg(feature = "webp")]
        if matches!(self, Self::WebP) {
            return false;
        }
        true
    }
}

/// The number of times an animation is played.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoopCount {
    /// The animation repeats indefinitely.
    Infinite,
    /// The animation plays this many times in total.
    Finite(u32),
}

/// A row-major, straight-alpha sRGB RGBA8 bitmap.
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

    /// Returns the row-major, straight-alpha sRGB RGBA8 pixels.
    pub fn data(&self) -> &[u8] {
        &self.data
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

    /// Returns the encoded display duration; static images have zero duration.
    pub const fn delay(&self) -> Duration {
        self.delay
    }
}

/// A decoded image with sRGB pixels. Embedded color profiles and EXIF orientation are already
/// applied to the pixels.
#[derive(Debug, Eq, PartialEq)]
pub struct Image {
    format: Format,
    frames: Vec<Frame>,
    text: Vec<(String, String)>,
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
        options: EncodeOptions<'_>,
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

    /// Returns one or more fully composited frames.
    pub fn frames(&self) -> &[Frame] {
        &self.frames
    }

    /// Returns the first displayed frame's RGBA8 pixels.
    pub fn pixels(&self) -> &[u8] {
        self.frames[0].pixels()
    }

    /// Returns whether the image has animation frames or playback metadata.
    pub const fn is_animated(&self) -> bool {
        self.is_animated
    }

    /// Returns the keyword and text pairs of the PNG/APNG text chunks.
    pub fn text(&self) -> &[(String, String)] {
        &self.text
    }

    /// Returns the total animation plays.
    pub const fn loop_count(&self) -> LoopCount {
        self.loop_count
    }

    #[cfg(any(
        feature = "bmp",
        feature = "jpeg",
        feature = "jpeg2000",
        feature = "png",
        feature = "qoi",
        feature = "pnm",
        feature = "tga",
        feature = "webp"
    ))]
    fn still(format: Format, width: u32, height: u32, pixels: Vec<u8>) -> Self {
        Self {
            format,
            frames: alloc::vec![Frame {
                bitmap: Bitmap {
                    width,
                    height,
                    data: pixels,
                },
                delay: Duration::ZERO,
            }],
            text: Vec::new(),
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

/// Decodes a complete image within 1 GiB, excluding uncommon BMP/JPEG variants.
#[cfg_attr(
    not(any(
        feature = "bmp",
        feature = "gif",
        feature = "jpeg",
        feature = "jpeg2000",
        feature = "png",
        feature = "qoi",
        feature = "pnm",
        feature = "tga",
        feature = "webp"
    )),
    allow(clippy::missing_const_for_fn)
)]
pub fn decode(_data: &[u8]) -> Result<Image> {
    match Format::from_signature(_data) {
        #[cfg(feature = "bmp")]
        Some(Format::Bmp) => return bmp::decode(_data),
        #[cfg(feature = "gif")]
        Some(Format::Gif) => return gif::decode(_data),
        #[cfg(feature = "ico")]
        Some(Format::Ico) => return ico::decode(_data),
        #[cfg(feature = "icns")]
        Some(Format::Icns) => return icns::decode(_data),
        #[cfg(feature = "webp")]
        Some(Format::WebP) => return webp::decode(_data),
        #[cfg(feature = "jpeg")]
        Some(Format::Jpeg) => return jpeg::decode(_data),
        #[cfg(feature = "jpeg2000")]
        Some(Format::Jpeg2000) => return jpeg2000::decode(_data),
        #[cfg(feature = "png")]
        Some(Format::Png) => return png::decode(_data),
        #[cfg(feature = "pnm")]
        Some(Format::Pnm) => return pnm::decode(_data),
        #[cfg(feature = "qoi")]
        Some(Format::Qoi) => return qoi::decode(_data),
        _ => {}
    }
    // A headerless DIB starts with its header size, so it is tried after the signatures.
    #[cfg(feature = "bmp")]
    if let Some(length) = _data.first_chunk::<4>()
        && matches!(
            u32::from_le_bytes(*length),
            12 | 40 | 52 | 56 | 64 | 108 | 124
        )
    {
        return bmp::decode_dib(_data);
    }
    // TGA has no signature, so it is only tried when its header is plausible.
    #[cfg(feature = "tga")]
    if let Some(header) = tga::Header::parse(_data) {
        return tga::decode(_data, &header);
    }
    Err(DecodeError::InvalidMagic)
}

#[cfg(any(
    feature = "bmp",
    feature = "gif",
    feature = "ico",
    feature = "jpeg",
    feature = "jpeg2000",
    feature = "png",
    feature = "qoi",
    feature = "pnm",
    feature = "tga",
    feature = "webp"
))]
const MAX_BYTES: usize = 1024 * 1024 * 1024;

// Count cumulative allocations conservatively, including buffers later discarded.
#[derive(Default)]
#[cfg(any(
    feature = "bmp",
    feature = "gif",
    feature = "ico",
    feature = "jpeg",
    feature = "jpeg2000",
    feature = "png",
    feature = "qoi",
    feature = "pnm",
    feature = "tga",
    feature = "webp"
))]
struct Budget {
    used: usize,
}

#[cfg(any(
    feature = "bmp",
    feature = "gif",
    feature = "jpeg",
    feature = "jpeg2000",
    feature = "png",
    feature = "qoi",
    feature = "pnm",
    feature = "tga",
    feature = "webp"
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
        feature = "bmp",
        feature = "gif",
        feature = "jpeg",
        feature = "jpeg2000",
        feature = "png",
        feature = "qoi",
        feature = "pnm",
        feature = "tga",
        feature = "webp"
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

    #[cfg(feature = "jpeg2000")]
    fn release<T>(&mut self, values: Vec<T>) {
        self.used -= values.len() * size_of::<T>();
        drop(values);
    }

    #[cfg(any(feature = "gif", feature = "png"))]
    fn copy(&mut self, bytes: &[u8]) -> Result<Vec<u8>> {
        self.claim(bytes.len())?;
        let mut out = Vec::new();
        out.try_reserve_exact(bytes.len())
            .map_err(|_| DecodeError::ImageTooLarge)?;
        out.extend_from_slice(bytes);
        Ok(out)
    }

    #[cfg(any(feature = "gif", feature = "png"))]
    fn append<T: Copy>(&mut self, out: &mut Vec<T>, bytes: &[T]) -> Result<()> {
        self.claim(size_of_val(bytes))?;
        out.try_reserve(bytes.len())
            .map_err(|_| DecodeError::ImageTooLarge)?;
        out.extend_from_slice(bytes);
        Ok(())
    }

    #[cfg(any(feature = "gif", feature = "png", feature = "webp"))]
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

#[cfg(any(
    feature = "bmp",
    feature = "gif",
    feature = "jpeg",
    feature = "jpeg2000",
    feature = "png",
    feature = "tga",
    feature = "webp"
))]
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

#[cfg(any(
    feature = "bmp",
    feature = "gif",
    feature = "jpeg",
    feature = "png",
    feature = "tga"
))]
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

#[cfg(any(
    feature = "bmp",
    feature = "gif",
    feature = "jpeg",
    feature = "png",
    feature = "tga"
))]
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

    #[cfg(any(feature = "bmp", feature = "gif"))]
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

    #[cfg(any(feature = "bmp", feature = "jpeg", feature = "png"))]
    const fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }
}

#[derive(Clone, Copy)]
#[cfg(any(feature = "gif", feature = "png"))]
struct Area {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}

#[cfg(any(feature = "gif", feature = "png"))]
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

// MARK: Tests
#[cfg(all(
    test,
    any(
        feature = "bmp",
        feature = "gif",
        feature = "ico",
        feature = "jpeg",
        feature = "png",
        feature = "qoi",
        feature = "tga"
    )
))]
mod tests {
    use super::*;

    #[cfg(all(feature = "jpeg", feature = "png", feature = "ico", feature = "tga"))]
    #[test]
    fn formats_follow_extensions_and_signatures() {
        assert_eq!(Format::from_extension("JPEG"), Some(Format::Jpeg));
        assert_eq!(Format::from_extension("apng"), Some(Format::Png));
        assert_eq!(Format::from_extension("tga"), Some(Format::Tga));
        assert_eq!(Format::from_extension("svg"), None);
        assert_eq!(
            Format::from_signature(b"\xff\xd8\xff\xe0"),
            Some(Format::Jpeg)
        );
        assert_eq!(
            Format::from_signature(b"\x89PNG\r\n\x1a\n"),
            Some(Format::Png)
        );
        assert_eq!(Format::from_signature(b"\x89PNG"), None);
        assert_eq!(Format::from_signature(b"\0\0\x02\0"), None);
        assert!(Format::Png.can_encode() && !Format::Ico.can_encode());
    }

    #[cfg(any(feature = "gif", feature = "jpeg"))]
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
    #[cfg(feature = "tga")]
    #[test]
    fn tga_reference_corpus() {
        test_support::run_raster_format("tga");
    }

    #[cfg(feature = "qoi")]
    #[test]
    fn qoi_color_reference_corpus() {
        test_support::run_raster_format("color/qoi");
    }

    #[cfg(feature = "jpeg")]
    #[test]
    fn jpeg_color_reference_corpus() {
        test_support::run_raster_format("color/jpeg");
    }

    #[cfg(feature = "png")]
    #[test]
    fn png_color_reference_corpus() {
        test_support::run_raster_format("color/png");
    }

    #[cfg(feature = "bmp")]
    #[test]
    fn bmp_color_reference_corpus() {
        test_support::run_raster_format("color/bmp");
    }

    #[cfg(any(
        feature = "bmp",
        feature = "gif",
        feature = "ico",
        feature = "jpeg",
        feature = "png",
        feature = "qoi",
        feature = "tga"
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
        feature = "bmp",
        feature = "gif",
        feature = "ico",
        feature = "jpeg",
        feature = "png",
        feature = "qoi",
        feature = "tga"
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
                // A 16x16 RLE true color TGA header.
                b"\0\0\x0a\0\0\0\0\0\0\0\0\0\x10\0\x10\0\x20\x08",
            ] {
                let mut input = signature.to_vec();
                input.extend_from_slice(&bytes);
                let _ = decode(&input);
            }
        }
    }

    #[cfg(any(feature = "gif", feature = "png"))]
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
