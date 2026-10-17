/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use alloc::vec::Vec;

use super::{Bitmap, Budget, ColorSpace, DecodeError, EncodeError, Format, Image};

const HEADER_LEN: usize = 14;
const END_MARKER: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 1];
const MAX_PIXELS: u64 = 400_000_000;

// MARK: Decoder
struct Decoder<'a> {
    data: &'a [u8],
    cursor: usize,
    stream_end: usize,
    index: [[u8; 4]; 64],
    pixel: [u8; 4],
}

/// Decodes a complete QOI file to RGBA pixels.
pub(super) fn decode(data: &[u8]) -> Result<Image, DecodeError> {
    Decoder::new(data)?.decode()
}

impl<'a> Decoder<'a> {
    fn new(data: &'a [u8]) -> Result<Self, DecodeError> {
        if data.len() < HEADER_LEN + END_MARKER.len() {
            return Err(DecodeError::InvalidData);
        }
        if &data[..4] != b"qoif" {
            return Err(DecodeError::InvalidMagic);
        }
        let stream_end = data.len() - END_MARKER.len();
        if data[stream_end..] != END_MARKER {
            return Err(DecodeError::InvalidData);
        }
        Ok(Self {
            data,
            cursor: HEADER_LEN,
            stream_end,
            index: [[0; 4]; 64],
            pixel: [0, 0, 0, 255],
        })
    }

    fn decode(mut self) -> Result<Image, DecodeError> {
        let width = u32::from_be_bytes(self.data[4..8].try_into().expect("four bytes"));
        let height = u32::from_be_bytes(self.data[8..12].try_into().expect("four bytes"));
        if width == 0 || height == 0 || !matches!(self.data[12], 3 | 4) || self.data[13] > 1 {
            return Err(DecodeError::InvalidHeader);
        }
        let pixel_count = u64::from(width) * u64::from(height);
        if pixel_count >= MAX_PIXELS {
            return Err(DecodeError::ImageTooLarge);
        }
        let pixel_count = usize::try_from(pixel_count).map_err(|_| DecodeError::ImageTooLarge)?;
        let output_len = pixel_count
            .checked_mul(4)
            .ok_or(DecodeError::ImageTooLarge)?;
        if self.stream_end - HEADER_LEN < pixel_count.div_ceil(62) {
            return Err(DecodeError::InvalidData);
        }
        let mut budget = Budget::default();
        let mut pixels = budget.zeroed::<u8>(output_len)?;
        let output = pixels.as_chunks_mut::<4>().0;
        let mut written = 0;
        while written < output.len() {
            if self.cursor >= self.stream_end {
                return Err(DecodeError::InvalidData);
            }
            // The end marker follows the stream, so operand bytes can be read before the
            // cursor is validated against the stream end.
            let bytes = &self.data[self.cursor..self.cursor + 5];
            let byte = bytes[0];
            let (len, run) = match byte {
                0xfe => {
                    self.pixel[..3].copy_from_slice(&bytes[1..4]);
                    (4, 1)
                }
                0xff => {
                    self.pixel.copy_from_slice(&bytes[1..5]);
                    (5, 1)
                }
                _ if byte & 0xc0 == 0x00 => {
                    self.pixel = self.index[usize::from(byte & 0x3f)];
                    (1, 1)
                }
                _ if byte & 0xc0 == 0x40 => {
                    self.pixel[0] = self.pixel[0].wrapping_add((byte >> 4 & 3).wrapping_sub(2));
                    self.pixel[1] = self.pixel[1].wrapping_add((byte >> 2 & 3).wrapping_sub(2));
                    self.pixel[2] = self.pixel[2].wrapping_add((byte & 3).wrapping_sub(2));
                    (1, 1)
                }
                _ if byte & 0xc0 == 0x80 => {
                    let second = bytes[1];
                    let green = (byte & 0x3f).wrapping_sub(32);
                    self.pixel[0] = self.pixel[0]
                        .wrapping_add(green.wrapping_add((second >> 4).wrapping_sub(8)));
                    self.pixel[1] = self.pixel[1].wrapping_add(green);
                    self.pixel[2] = self.pixel[2]
                        .wrapping_add(green.wrapping_add((second & 15).wrapping_sub(8)));
                    (2, 1)
                }
                _ => (1, usize::from(byte & 0x3f) + 1),
            };
            self.cursor += len;
            if self.cursor > self.stream_end || run > output.len() - written {
                return Err(DecodeError::InvalidData);
            }
            self.index[Self::pixel_hash(self.pixel)] = self.pixel;
            if run == 1 {
                output[written] = self.pixel;
            } else {
                output[written..written + run].fill(self.pixel);
            }
            written += run;
        }
        if self.cursor != self.stream_end {
            return Err(DecodeError::InvalidData);
        }
        let mut image = Image::still(Format::Qoi, width, height, pixels);
        image.color_space = if self.data[13] == 0 {
            ColorSpace::Srgb
        } else {
            ColorSpace::LinearSrgb
        };
        Ok(image)
    }

    const fn pixel_hash(pixel: [u8; 4]) -> usize {
        (pixel[0] as usize * 3
            + pixel[1] as usize * 5
            + pixel[2] as usize * 7
            + pixel[3] as usize * 11)
            % 64
    }
}

// MARK: Encoder
pub(super) fn encode(bitmap: &Bitmap) -> Result<Vec<u8>, EncodeError> {
    let mut out = Vec::new();
    out.try_reserve(
        bitmap
            .data
            .len()
            .checked_add(22)
            .ok_or(EncodeError::ImageTooLarge)?,
    )
    .map_err(|_| EncodeError::ImageTooLarge)?;
    out.extend_from_slice(b"qoif");
    out.extend_from_slice(&bitmap.width.to_be_bytes());
    out.extend_from_slice(&bitmap.height.to_be_bytes());
    out.extend_from_slice(&[4, 0]);
    let mut previous = [0, 0, 0, 255];
    let mut index = [[0; 4]; 64];
    let mut run = 0u8;
    for pixel in bitmap.data.as_chunks::<4>().0 {
        if *pixel == previous {
            run += 1;
            if run == 62 {
                out.push(0xc0 | (run - 1));
                run = 0;
            }
            continue;
        }
        if run != 0 {
            out.push(0xc0 | (run - 1));
            run = 0;
        }
        let hash = (usize::from(pixel[0]) * 3
            + usize::from(pixel[1]) * 5
            + usize::from(pixel[2]) * 7
            + usize::from(pixel[3]) * 11)
            % 64;
        if index[hash] == *pixel {
            out.push(hash as u8);
        } else {
            index[hash] = *pixel;
            if pixel[3] == previous[3] {
                let dr = i16::from(pixel[0].wrapping_sub(previous[0]) as i8);
                let dg = i16::from(pixel[1].wrapping_sub(previous[1]) as i8);
                let db = i16::from(pixel[2].wrapping_sub(previous[2]) as i8);
                if (-2..=1).contains(&dr) && (-2..=1).contains(&dg) && (-2..=1).contains(&db) {
                    out.push(0x40 | ((dr + 2) as u8) << 4 | ((dg + 2) as u8) << 2 | (db + 2) as u8);
                } else if (-32..=31).contains(&dg)
                    && (-8..=7).contains(&(dr - dg))
                    && (-8..=7).contains(&(db - dg))
                {
                    out.extend_from_slice(&[
                        0x80 | (dg + 32) as u8,
                        ((dr - dg + 8) as u8) << 4 | (db - dg + 8) as u8,
                    ]);
                } else {
                    out.extend_from_slice(&[0xfe, pixel[0], pixel[1], pixel[2]]);
                }
            } else {
                out.extend_from_slice(&[0xff, pixel[0], pixel[1], pixel[2], pixel[3]]);
            }
        }
        previous = *pixel;
    }
    if run != 0 {
        out.push(0xc0 | (run - 1));
    }
    out.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 1]);
    Ok(out)
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Format, encode};

    #[test]
    fn image_rs_basic_fixture_decodes() {
        let image = decode(include_bytes!("../tests/images/qoi/basic-test.qoi"))
            .expect("image-rs QOI fixture");
        assert_eq!((image.width(), image.height()), (5, 5));
        assert_eq!(image.pixels().len(), 5 * 5 * 4);
        assert!(!image.is_animated());
        assert_eq!(image.loop_count(), crate::LoopCount::Finite(1));
    }

    #[cfg(feature = "qoi")]
    #[test]
    fn qoi_uses_difference_opcodes() {
        let bitmap = Bitmap::new(
            3,
            1,
            vec![10, 20, 30, 255, 11, 21, 30, 255, 20, 28, 40, 255],
        )
        .unwrap();
        let encoded = encode(&bitmap, Format::Qoi).unwrap();
        assert!(
            encoded[14..encoded.len() - 8]
                .iter()
                .any(|b| b & 0xc0 == 0x40)
        );
        assert!(
            encoded[14..encoded.len() - 8]
                .iter()
                .any(|b| b & 0xc0 == 0x80)
        );
        assert_eq!(crate::decode(&encoded).unwrap().pixels(), bitmap.data());
    }
}
