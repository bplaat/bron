/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

use super::{Bitmap, Format, Frame, LoopCount};

/// The encoding time and file size tradeoff.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum EncodingStyle {
    /// Apply useful size optimizations without searching many alternatives.
    #[default]
    NormalCompression,
    /// Try additional representations and return the smallest candidate.
    MaxCompression,
}

/// Options for raster encoding.
#[derive(Clone, Copy, Debug)]
pub struct EncodeOptions {
    /// Encoding time and file size tradeoff.
    pub style: EncodingStyle,
    /// JPEG quality from 1 to 100. Ignored by other formats.
    pub jpeg_quality: u8,
}

impl Default for EncodeOptions {
    fn default() -> Self {
        Self {
            style: EncodingStyle::NormalCompression,
            jpeg_quality: 85,
        }
    }
}

/// A failure to encode an image.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EncodeError {
    /// Dimensions are zero or outside the chosen format's limits.
    InvalidDimensions,
    /// Pixel data does not match the dimensions.
    InvalidPixels,
    /// The format does not support animation.
    UnsupportedAnimation,
    /// The format does not have an encoder.
    UnsupportedFormat,
    /// JPEG requires fully opaque pixels.
    AlphaUnsupported,
    /// A delay or loop count cannot be represented by the format.
    InvalidTiming,
    /// JPEG quality must be between 1 and 100.
    InvalidQuality,
    /// Output exceeds the supported size or an allocation failed.
    ImageTooLarge,
}

impl Display for EncodeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidDimensions => "invalid image dimensions",
            Self::InvalidPixels => "pixel data does not match image dimensions",
            Self::UnsupportedAnimation => "format does not support animation",
            Self::UnsupportedFormat => "format does not have an encoder",
            Self::AlphaUnsupported => "JPEG requires opaque pixels",
            Self::InvalidTiming => "animation timing is outside format limits",
            Self::InvalidQuality => "JPEG quality must be between 1 and 100",
            Self::ImageTooLarge => "image exceeds encoding resource limits",
        })
    }
}

impl core::error::Error for EncodeError {}

pub(super) fn validate_pixels(width: u32, height: u32, data: &[u8]) -> Result<(), EncodeError> {
    if width == 0 || height == 0 {
        return Err(EncodeError::InvalidDimensions);
    }
    let len = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|n| n.checked_mul(4))
        .ok_or(EncodeError::ImageTooLarge)?;
    if len > 1024 * 1024 * 1024 {
        return Err(EncodeError::ImageTooLarge);
    }
    if usize::try_from(len).ok() != Some(data.len()) {
        return Err(EncodeError::InvalidPixels);
    }
    Ok(())
}

// MARK: Encoder

/// Encodes a single RGBA8 bitmap with default options.
pub fn encode(bitmap: &Bitmap, format: Format) -> Result<Vec<u8>, EncodeError> {
    encode_with_options(bitmap, format, EncodeOptions::default())
}

/// Encodes a single RGBA8 bitmap.
pub fn encode_with_options(
    bitmap: &Bitmap,
    format: Format,
    options: EncodeOptions,
) -> Result<Vec<u8>, EncodeError> {
    validate_pixels(bitmap.width, bitmap.height, &bitmap.data)?;
    match format {
        #[cfg(feature = "bmp")]
        Format::Bmp => super::bmp::encode(bitmap),
        #[cfg(feature = "gif")]
        Format::Gif => super::gif::encode_bitmap(bitmap, options.style),
        #[cfg(feature = "png")]
        Format::Png => super::png::encode(bitmap, None, options.style),
        #[cfg(feature = "jpeg")]
        Format::Jpeg => super::jpeg::encode(bitmap, options.jpeg_quality, options.style),
        #[cfg(feature = "qoi")]
        Format::Qoi => super::qoi::encode(bitmap),
        #[cfg(feature = "ico")]
        Format::Ico => Err(EncodeError::UnsupportedFormat),
        #[allow(unreachable_patterns)]
        _ => {
            let _ = options;
            Err(EncodeError::UnsupportedFormat)
        }
    }
}

/// Encodes complete canvas frames as GIF or APNG.
pub fn encode_animation(
    format: Format,
    frames: &[Frame],
    loop_count: LoopCount,
) -> Result<Vec<u8>, EncodeError> {
    encode_animation_with_options(format, frames, loop_count, EncodeOptions::default())
}

/// Encodes complete canvas frames as GIF or APNG with the requested options.
pub fn encode_animation_with_options(
    format: Format,
    frames: &[Frame],
    _loop_count: LoopCount,
    options: EncodeOptions,
) -> Result<Vec<u8>, EncodeError> {
    let first = frames.first().ok_or(EncodeError::InvalidDimensions)?;
    let width = first.bitmap.width;
    let height = first.bitmap.height;
    for frame in frames {
        if frame.bitmap.width != width || frame.bitmap.height != height {
            return Err(EncodeError::InvalidDimensions);
        }
        validate_pixels(width, height, &frame.bitmap.data)?;
    }
    match format {
        #[cfg(feature = "gif")]
        Format::Gif => super::gif::encode(frames, _loop_count, options.style),
        #[cfg(feature = "png")]
        Format::Png => super::png::encode_animation(frames, _loop_count, options.style),
        #[allow(unreachable_patterns)]
        _ => {
            let _ = options;
            Err(EncodeError::UnsupportedAnimation)
        }
    }
}

// Crops `current` to the rectangle that differs from `previous`, returning its offset.
#[cfg(any(feature = "gif", feature = "png"))]
pub(super) fn crop_changed(
    previous: &Bitmap,
    current: &Bitmap,
) -> Result<(u32, u32, Bitmap), EncodeError> {
    let width = current.width as usize;
    let mut left = width;
    let mut top = current.height as usize;
    let mut right = 0;
    let mut bottom = 0;
    for (index, (a, b)) in previous
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .zip(current.data.as_chunks::<4>().0)
        .enumerate()
    {
        if a != b {
            let x = index % width;
            let y = index / width;
            left = left.min(x);
            top = top.min(y);
            right = right.max(x + 1);
            bottom = bottom.max(y + 1);
        }
    }
    if left == width {
        (left, top, right, bottom) = (0, 0, 1, 1);
    }
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact((right - left) * (bottom - top) * 4)
        .map_err(|_| EncodeError::ImageTooLarge)?;
    for y in top..bottom {
        let start = (y * width + left) * 4;
        pixels.extend_from_slice(&current.data[start..start + (right - left) * 4]);
    }
    Ok((
        left as u32,
        top as u32,
        Bitmap::new((right - left) as u32, (bottom - top) as u32, pixels)?,
    ))
}

// MARK: Tests
#[cfg(all(
    test,
    any(
        feature = "bmp",
        feature = "gif",
        feature = "png",
        feature = "jpeg",
        feature = "qoi"
    )
))]
mod tests {
    use super::*;

    #[cfg(any(
        feature = "bmp",
        feature = "gif",
        feature = "png",
        feature = "jpeg",
        feature = "qoi"
    ))]
    #[test]
    #[allow(clippy::single_element_loop)] // A single codec feature leaves one format.
    fn normal_style_encodes_supported_formats() {
        let bitmap = Bitmap::new(4, 4, [64, 128, 192, 255].repeat(16)).unwrap();
        for format in [
            #[cfg(feature = "bmp")]
            Format::Bmp,
            #[cfg(feature = "gif")]
            Format::Gif,
            #[cfg(feature = "png")]
            Format::Png,
            #[cfg(feature = "jpeg")]
            Format::Jpeg,
            #[cfg(feature = "qoi")]
            Format::Qoi,
        ] {
            let encoded = encode(&bitmap, format).unwrap();
            let decoded = crate::decode(&encoded).unwrap();
            assert_eq!((decoded.width(), decoded.height()), (4, 4));
        }
    }

    #[cfg(any(
        feature = "bmp",
        feature = "gif",
        feature = "png",
        feature = "jpeg",
        feature = "qoi"
    ))]
    #[test]
    #[allow(clippy::single_element_loop)] // A single codec feature leaves one format.
    fn max_style_does_not_exceed_normal_size() {
        let mut pixels = Vec::new();
        for y in 0..32u8 {
            for x in 0..32u8 {
                pixels.extend_from_slice(&[x, y, x ^ y, 255]);
            }
        }
        for bitmap in [
            Bitmap::new(32, 32, pixels).unwrap(),
            Bitmap::new(32, 32, [255, 0, 0, 255].repeat(1024)).unwrap(),
        ] {
            for format in [
                #[cfg(feature = "bmp")]
                Format::Bmp,
                #[cfg(feature = "gif")]
                Format::Gif,
                #[cfg(feature = "png")]
                Format::Png,
                #[cfg(feature = "jpeg")]
                Format::Jpeg,
                #[cfg(feature = "qoi")]
                Format::Qoi,
            ] {
                let normal = encode(&bitmap, format).unwrap();
                let max = encode_with_options(
                    &bitmap,
                    format,
                    EncodeOptions {
                        style: EncodingStyle::MaxCompression,
                        ..EncodeOptions::default()
                    },
                )
                .unwrap();
                assert!(max.len() <= normal.len(), "{format:?}");
            }
        }
    }

    #[cfg(any(feature = "bmp", feature = "png", feature = "qoi"))]
    #[test]
    fn lossless_round_trip() {
        let pixels = vec![
            255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 12, 34, 56, 255,
        ];
        let bitmap = Bitmap::new(2, 2, pixels.clone()).unwrap();
        for format in [
            #[cfg(feature = "bmp")]
            Format::Bmp,
            #[cfg(feature = "png")]
            Format::Png,
            #[cfg(feature = "qoi")]
            Format::Qoi,
        ] {
            let decoded = crate::decode(&encode(&bitmap, format).unwrap()).unwrap();
            assert_eq!(decoded.pixels(), pixels);
        }
    }

    #[cfg(any(feature = "gif", feature = "png"))]
    #[test]
    #[allow(clippy::single_element_loop)] // A single codec feature leaves one format.
    fn animation_round_trip() {
        let frames = [
            Frame::new(
                Bitmap::new(1, 1, vec![255, 0, 0, 255]).unwrap(),
                core::time::Duration::from_millis(100),
            ),
            Frame::new(
                Bitmap::new(1, 1, vec![0, 255, 0, 255]).unwrap(),
                core::time::Duration::from_millis(200),
            ),
        ];
        for format in [
            #[cfg(feature = "gif")]
            Format::Gif,
            #[cfg(feature = "png")]
            Format::Png,
        ] {
            let decoded =
                crate::decode(&encode_animation(format, &frames, LoopCount::Finite(3)).unwrap())
                    .unwrap();
            assert_eq!(decoded.frames().len(), 2);
            assert_eq!(decoded.loop_count(), LoopCount::Finite(3));
            assert_eq!(decoded.frames()[0].delay(), frames[0].delay());
            assert_eq!(decoded.frames()[1].delay(), frames[1].delay());
            assert_eq!(decoded.frames()[0].pixels(), frames[0].pixels());
            assert_eq!(decoded.frames()[1].pixels(), frames[1].pixels());
        }
    }
}
