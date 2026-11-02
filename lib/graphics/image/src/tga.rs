/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// Truevision TGA has no signature, so the 18-byte header is validated before decoding.
// Explicit extension alpha semantics take precedence over legacy header conventions.

use alloc::vec::Vec;

use super::{
    Bitmap, Budget, DecodeError, EncodeError, EncodingStyle, Format, Image, Reader, Result,
    pixel_len,
};

const HEADER_LEN: usize = 18;
const FOOTER: [u8; 26] = *b"\0\0\0\0\0\0\0\0TRUEVISION-XFILE.\0";
const COLOR_MAPPED: u8 = 1;
const TRUE_COLOR: u8 = 2;
const GRAYSCALE: u8 = 3;
const RLE: u8 = 8;
const TOP_DOWN: u8 = 0x20;
const RIGHT_TO_LEFT: u8 = 0x10;

// MARK: Decoder
pub(super) struct Header {
    id_len: usize,
    color_map: Option<ColorMap>,
    image_type: u8,
    width: u32,
    height: u32,
    depth: u8,
    descriptor: u8,
}

#[derive(Clone, Copy)]
struct ColorMap {
    first: usize,
    len: usize,
    depth: u8,
}

impl Header {
    /// Returns the header when the data plausibly starts with one.
    pub(super) fn parse(data: &[u8]) -> Option<Self> {
        let h = data.first_chunk::<HEADER_LEN>()?;
        let le16 = |i: usize| u16::from_le_bytes([h[i], h[i + 1]]);
        let image_type = h[2];
        let color_map = ColorMap {
            first: usize::from(le16(3)),
            len: usize::from(le16(5)),
            depth: h[7],
        };
        let depth = h[16];
        let descriptor = h[17];
        let color_map = match (h[1], image_type & !RLE) {
            (1, _) if color_map.len != 0 && matches!(color_map.depth, 15 | 16 | 24 | 32) => {
                Some(color_map)
            }
            (0, TRUE_COLOR | GRAYSCALE) if color_map.len == 0 => None,
            _ => return None,
        };
        let supported_depth = match image_type & !RLE {
            COLOR_MAPPED => color_map.is_some() && matches!(depth, 8 | 16),
            TRUE_COLOR => matches!(depth, 15 | 16 | 24 | 32),
            GRAYSCALE => matches!(depth, 8 | 16),
            _ => false,
        };
        let (width, height) = (u32::from(le16(12)), u32::from(le16(14)));
        // Descriptor bits 6 and 7 select obsolete interleaving and must be zero.
        (supported_depth && width != 0 && height != 0 && descriptor & 0xc0 == 0).then_some(Self {
            id_len: usize::from(h[0]),
            color_map,
            image_type,
            width,
            height,
            depth,
            descriptor,
        })
    }
}

// A 16-bit color uses its top bit as alpha when the descriptor declares alpha bits.
fn color(bytes: &[u8], depth: u8, alpha: bool) -> [u8; 4] {
    match *bytes {
        [low, high] => {
            let value = u16::from_le_bytes([low, high]);
            let channel = |shift: u16| {
                let c = (value >> shift & 31) as u8;
                c << 3 | c >> 2
            };
            let alpha = if depth == 16 && alpha && value & 0x8000 == 0 {
                0
            } else {
                255
            };
            [channel(10), channel(5), channel(0), alpha]
        }
        _ => [
            bytes[2],
            bytes[1],
            bytes[0],
            bytes.get(3).copied().unwrap_or(255),
        ],
    }
}

/// Decodes a TGA file whose header `Header::parse` accepted.
pub(super) fn decode(data: &[u8], header: &Header) -> Result<Image> {
    let attributes = attributes_type(data)?;
    let declared_alpha = header.descriptor & 0x0f != 0 || matches!(attributes, Some(2..=4));
    let mut r = Reader::new(data);
    r.take(HEADER_LEN + header.id_len)?;
    let mut budget = Budget::default();
    let palette = match header.color_map {
        Some(map) => {
            let entry_len = usize::from(map.depth.div_ceil(8));
            let mut palette = budget.zeroed::<[u8; 4]>(map.len)?;
            for (entry, bytes) in palette
                .iter_mut()
                .zip(r.take(map.len * entry_len)?.chunks_exact(entry_len))
            {
                *entry = color(bytes, map.depth, declared_alpha);
            }
            palette
        }
        None => Vec::new(),
    };
    let bytes_per_pixel = usize::from(header.depth.div_ceil(8));
    let first = header.color_map.map_or(0, |map| map.first);
    let convert = |bytes: &[u8]| -> Result<[u8; 4]> {
        Ok(match header.image_type & !RLE {
            COLOR_MAPPED => {
                let index = match *bytes {
                    [low, high] => usize::from(u16::from_le_bytes([low, high])),
                    _ => usize::from(bytes[0]),
                };
                *index
                    .checked_sub(first)
                    .and_then(|index| palette.get(index))
                    .ok_or(DecodeError::InvalidData)?
            }
            GRAYSCALE => [
                bytes[0],
                bytes[0],
                bytes[0],
                bytes.get(1).copied().unwrap_or(255),
            ],
            _ => color(bytes, header.depth, declared_alpha),
        })
    };

    let mut pixels = budget.zeroed::<u8>(pixel_len(header.width, header.height)?)?;
    let output = pixels.as_chunks_mut::<4>().0;
    if header.image_type & RLE != 0 {
        let mut written = 0;
        while written < output.len() {
            let packet = r.byte()?;
            let count = usize::from(packet & 0x7f) + 1;
            let run = output
                .get_mut(written..written + count)
                .ok_or(DecodeError::InvalidData)?;
            if packet & 0x80 != 0 {
                run.fill(convert(r.take(bytes_per_pixel)?)?);
            } else {
                let source = r.take(count * bytes_per_pixel)?;
                for (pixel, bytes) in run.iter_mut().zip(source.chunks_exact(bytes_per_pixel)) {
                    *pixel = convert(bytes)?;
                }
            }
            written += count;
        }
    } else {
        let source = r.take(output.len() * bytes_per_pixel)?;
        for (pixel, bytes) in output.iter_mut().zip(source.chunks_exact(bytes_per_pixel)) {
            *pixel = convert(bytes)?;
        }
    }

    let row_len = header.width as usize;
    if header.descriptor & RIGHT_TO_LEFT != 0 {
        for row in output.chunks_exact_mut(row_len) {
            row.reverse();
        }
    }
    if header.descriptor & TOP_DOWN == 0 {
        let rows = output.len() / row_len;
        for y in 0..rows / 2 {
            let (top, bottom) = output.split_at_mut((rows - 1 - y) * row_len);
            top[y * row_len..(y + 1) * row_len].swap_with_slice(&mut bottom[..row_len]);
        }
    }
    let opaque = matches!(attributes, Some(0 | 1))
        || (attributes.is_none() && !declared_alpha && output.iter().all(|p| p[3] == 0));
    for pixel in output {
        if opaque {
            pixel[3] = 255;
        } else if attributes == Some(4) && pixel[3] != 0 {
            let alpha = u32::from(pixel[3]);
            for channel in &mut pixel[..3] {
                *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
    Ok(Image::still(
        Format::Tga,
        header.width,
        header.height,
        pixels,
    ))
}

// A signed footer with an extension offset must reference a complete extension area.
fn attributes_type(data: &[u8]) -> Result<Option<u8>> {
    let Some(footer) = data.get(data.len().saturating_sub(FOOTER.len())..) else {
        return Ok(None);
    };
    if footer.len() != FOOTER.len() || footer[8..] != FOOTER[8..] {
        return Ok(None);
    }
    let offset = u32::from_le_bytes([footer[0], footer[1], footer[2], footer[3]]) as usize;
    if offset == 0 {
        return Ok(None);
    }
    let extension = data
        .get(..data.len() - FOOTER.len())
        .and_then(|data| data.get(offset..))
        .and_then(|data| data.get(..495))
        .ok_or(DecodeError::InvalidData)?;
    if extension[..2] != 495u16.to_le_bytes() {
        return Err(DecodeError::InvalidData);
    }
    Ok(Some(extension[494]))
}

// MARK: Encoder
// Compression tries grayscale, palettes and RLE, which are all cheap, so normal and max
// compression are the same.
pub(super) fn encode(
    bitmap: &Bitmap,
    style: EncodingStyle,
    alpha: bool,
) -> core::result::Result<Vec<u8>, EncodeError> {
    let compress = style != EncodingStyle::NoCompression;
    if bitmap.width > u32::from(u16::MAX) || bitmap.height > u32::from(u16::MAX) {
        return Err(EncodeError::InvalidDimensions);
    }
    let pixels = bitmap.data.as_chunks::<4>().0;
    let force_alpha = alpha;
    let alpha = alpha || pixels.iter().any(|pixel| pixel[3] != 255);
    let grayscale = compress && !force_alpha && pixels.iter().all(|p| p[0] == p[1] && p[1] == p[2]);
    // Grayscale always beats or ties true color for the same packet boundaries.
    let mut out = if grayscale {
        candidate(
            bitmap,
            GRAYSCALE,
            1 + usize::from(alpha),
            alpha,
            &[],
            compress,
            |[r, _, _, a]| [r, a, 0, 0],
        )?
    } else {
        candidate(
            bitmap,
            TRUE_COLOR,
            if alpha { 4 } else { 3 },
            alpha,
            &[],
            compress,
            |[r, g, b, a]| [b, g, r, a],
        )?
    };
    // Forced alpha keeps the documented 32-bit true-color representation, and an opaque grayscale
    // palette has the same pixel width plus palette overhead.
    if !compress || force_alpha || (grayscale && !alpha) {
        return Ok(out);
    }
    let mut lookup = [0u64; 512];
    let mut palette = [[0u8; 4]; 256];
    let mut colors = 0;
    for &pixel in pixels {
        let key = u32::from_be_bytes(pixel);
        let slot = super::encoder::palette_slot(&lookup, key);
        if lookup[slot] == 0 {
            if colors == 256 {
                return Ok(out);
            }
            let [r, g, b, a] = pixel;
            palette[colors] = [b, g, r, a];
            lookup[slot] = (u64::from(key) << 9) | (colors as u64 + 1);
            colors += 1;
        }
    }
    let mapped = candidate(
        bitmap,
        COLOR_MAPPED,
        1,
        alpha,
        &palette[..colors],
        compress,
        |pixel| {
            let slot = super::encoder::palette_slot(&lookup, u32::from_be_bytes(pixel));
            [(lookup[slot] - 1) as u8, 0, 0, 0]
        },
    )?;
    if mapped.len() < out.len() {
        out = mapped;
    }
    Ok(out)
}

fn candidate(
    bitmap: &Bitmap,
    image_type: u8,
    bytes_per_pixel: usize,
    alpha: bool,
    palette: &[[u8; 4]],
    compress: bool,
    convert: impl Fn([u8; 4]) -> [u8; 4],
) -> core::result::Result<Vec<u8>, EncodeError> {
    let width = bitmap.width as u16;
    let height = bitmap.height as u16;
    let palette_depth = if alpha { 4 } else { 3 };
    let prefix_len = HEADER_LEN + palette.len() * palette_depth;
    let mut out = Vec::new();
    out.try_reserve_exact(prefix_len + bitmap.data.len() / 4 * bytes_per_pixel + FOOTER.len())
        .map_err(|_| EncodeError::ImageTooLarge)?;
    out.extend_from_slice(&[0, u8::from(!palette.is_empty()), image_type, 0, 0]);
    out.extend_from_slice(&(palette.len() as u16).to_le_bytes());
    out.push(if palette.is_empty() {
        0
    } else {
        palette_depth as u8 * 8
    });
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&[bytes_per_pixel as u8 * 8, if alpha { 8 } else { 0 }]);
    for color in palette {
        out.extend_from_slice(&color[..palette_depth]);
    }
    for row in bitmap
        .data
        .as_chunks::<4>()
        .0
        .chunks_exact(usize::from(width))
        .rev()
    {
        for &pixel in row {
            out.extend_from_slice(&convert(pixel)[..bytes_per_pixel]);
        }
    }
    if compress {
        let mut compressed = Vec::new();
        // Stop candidates that cannot beat raw output before growing the buffer.
        compressed
            .try_reserve_exact(out.len() + FOOTER.len())
            .map_err(|_| EncodeError::ImageTooLarge)?;
        compressed.extend_from_slice(&out[..prefix_len]);
        compressed[2] |= RLE;
        if rle_encode(
            &out[prefix_len..],
            width,
            bytes_per_pixel,
            &mut compressed,
            out.len(),
        ) {
            out = compressed;
        }
    }
    out.extend_from_slice(&FOOTER);
    Ok(out)
}

// Packets stay within one row, as the TGA 2.0 specification recommends.
fn rle_encode(
    body: &[u8],
    width: u16,
    bytes_per_pixel: usize,
    out: &mut Vec<u8>,
    limit: usize,
) -> bool {
    for row in body.chunks_exact(usize::from(width) * bytes_per_pixel) {
        let pixel = |i: usize| &row[i * bytes_per_pixel..(i + 1) * bytes_per_pixel];
        let len = usize::from(width);
        let mut i = 0;
        while i < len {
            let mut run = 1;
            while run < 128 && i + run < len && pixel(i + run) == pixel(i) {
                run += 1;
            }
            if run > 1 {
                if out.len() + 1 + bytes_per_pixel >= limit {
                    return false;
                }
                out.push(0x80 | (run - 1) as u8);
                out.extend_from_slice(pixel(i));
                i += run;
            } else {
                // Raw pixels continue until two equal pixels can start a run.
                let start = i;
                i += 1;
                while i < len && i - start < 128 && (i + 1 == len || pixel(i) != pixel(i + 1)) {
                    i += 1;
                }
                if out.len() + 1 + (i - start) * bytes_per_pixel >= limit {
                    return false;
                }
                out.push((i - start - 1) as u8);
                out.extend_from_slice(&row[start * bytes_per_pixel..i * bytes_per_pixel]);
            }
        }
    }
    true
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EncodeOptions, encode_with_options};

    fn options(style: EncodingStyle, tga_alpha: bool) -> EncodeOptions<'static> {
        EncodeOptions {
            style,
            tga_alpha,
            ..EncodeOptions::default()
        }
    }

    fn round_trip(pixels: &[u8], options: EncodeOptions<'_>) -> Vec<u8> {
        let bitmap = Bitmap::new(2, 2, pixels.to_vec()).unwrap();
        let encoded = encode_with_options(&bitmap, Format::Tga, options).unwrap();
        let decoded = crate::decode(&encoded).unwrap();
        assert_eq!(decoded.format(), Format::Tga);
        assert_eq!(decoded.pixels(), pixels);
        encoded
    }

    #[test]
    fn opaque_images_use_24_bit_pixels() {
        let pixels = [
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 12, 34, 56, 255,
        ];
        let encoded = round_trip(&pixels, options(EncodingStyle::NoCompression, false));
        assert_eq!((encoded[2], encoded[16], encoded[17]), (2, 24, 0));
        assert_eq!(encoded.len(), HEADER_LEN + 2 * 2 * 3 + FOOTER.len());
        assert!(encoded.ends_with(&FOOTER));
        // The bottom-left pixel comes first, in BGR order.
        assert_eq!(encoded[HEADER_LEN..HEADER_LEN + 3], [255, 0, 0]);

        let encoded = round_trip(&pixels, options(EncodingStyle::NoCompression, true));
        assert_eq!((encoded[16], encoded[17]), (32, 8));
    }

    #[test]
    fn transparent_images_use_32_bit_pixels() {
        let pixels = [
            255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 12, 34, 56, 255,
        ];
        let encoded = round_trip(&pixels, options(EncodingStyle::NoCompression, false));
        assert_eq!((encoded[16], encoded[17]), (32, 8));
    }

    #[test]
    fn compression_uses_rle_for_flat_images() {
        let bitmap = Bitmap::new(64, 64, [10, 20, 30, 255].repeat(64 * 64)).unwrap();
        let encode = |style| encode_with_options(&bitmap, Format::Tga, options(style, false));
        let none = encode(EncodingStyle::NoCompression).unwrap();
        let normal = encode(EncodingStyle::NormalCompression).unwrap();
        assert_eq!((none[2], normal[2]), (2, 9));
        assert!(normal.len() < none.len());
        assert_eq!(encode(EncodingStyle::MaxCompression).unwrap(), normal);
        assert_eq!(crate::decode(&normal).unwrap().pixels(), bitmap.data());

        // Raw and run packets of up to 128 pixels.
        let mut pixels = Vec::new();
        for i in 0..1200u32 {
            pixels.extend_from_slice(&[i.min(200) as u8, 1, 2, 255]);
        }
        let bitmap = Bitmap::new(1200, 1, pixels).unwrap();
        let encoded = encode_with_options(&bitmap, Format::Tga, EncodeOptions::default()).unwrap();
        assert_eq!(encoded[2], 10);
        assert_eq!(crate::decode(&encoded).unwrap().pixels(), bitmap.data());
    }

    #[test]
    fn oversized_images_are_rejected() {
        let bitmap = Bitmap::new(65536, 1, vec![0; 65536 * 4]).unwrap();
        assert_eq!(
            encode_with_options(&bitmap, Format::Tga, EncodeOptions::default()),
            Err(EncodeError::InvalidDimensions)
        );
    }

    fn header(image_type: u8, color_map: (u8, u16, u8), depth: u8, descriptor: u8) -> Vec<u8> {
        let mut out = vec![0, color_map.0, image_type, 0, 0];
        out.extend_from_slice(&color_map.1.to_le_bytes());
        out.extend_from_slice(&[color_map.2, 0, 0, 0, 0, 2, 0, 1, 0, depth, descriptor]);
        out
    }

    #[test]
    fn implausible_headers_are_not_tga() {
        for data in [
            header(0, (0, 0, 0), 24, 0),
            header(4, (0, 0, 0), 24, 0),
            header(2, (0, 0, 0), 12, 0),
            header(2, (0, 4, 24), 24, 0),
            header(2, (2, 0, 0), 24, 0),
            header(9, (0, 0, 0), 8, 0),
            header(1, (1, 2, 8), 8, 0),
            header(3, (0, 0, 0), 24, 0),
            header(2, (0, 0, 0), 24, 0x40),
            header(2, (0, 0, 0), 24, 0)[..17].to_vec(),
        ] {
            assert_eq!(crate::decode(&data), Err(DecodeError::InvalidMagic));
        }
        let mut empty = header(2, (0, 0, 0), 24, 0);
        empty[12] = 0;
        assert_eq!(crate::decode(&empty), Err(DecodeError::InvalidMagic));
    }

    #[test]
    fn corrupt_data_is_rejected() {
        // Truncated pixels.
        let mut data = header(3, (0, 0, 0), 8, 0);
        data.push(1);
        assert_eq!(crate::decode(&data), Err(DecodeError::InvalidData));
        // A run packet crossing the image end.
        let mut data = header(11, (0, 0, 0), 8, 0);
        data.extend_from_slice(&[0x82, 7]);
        assert_eq!(crate::decode(&data), Err(DecodeError::InvalidData));
        // Color map indices outside the color map.
        let mut data = header(1, (1, 1, 24), 8, 0);
        data.extend_from_slice(&[1, 2, 3, 0, 1]);
        assert_eq!(crate::decode(&data), Err(DecodeError::InvalidData));
        // A truncated color map.
        let mut data = header(1, (1, 4, 24), 8, 0);
        data.extend_from_slice(&[1, 2, 3, 0, 0]);
        assert_eq!(crate::decode(&data), Err(DecodeError::InvalidData));
        // A skipped image ID that runs past the end.
        let mut data = header(3, (0, 0, 0), 8, 0);
        data[0] = 4;
        data.extend_from_slice(&[1, 2]);
        assert_eq!(crate::decode(&data), Err(DecodeError::InvalidData));
    }

    #[test]
    fn color_map_origin_offsets_indices() {
        let mut data = header(1, (1, 2, 24), 8, TOP_DOWN);
        data[3] = 10;
        data.extend_from_slice(&[1, 2, 3, 4, 5, 6, 11, 10]);
        assert_eq!(
            crate::decode(&data).unwrap().pixels(),
            [6, 5, 4, 255, 3, 2, 1, 255]
        );
    }

    fn extension(data: &mut Vec<u8>, attributes: u8) {
        let offset = data.len() as u32;
        let mut area = [0; 495];
        area[..2].copy_from_slice(&495u16.to_le_bytes());
        area[494] = attributes;
        data.extend_from_slice(&area);
        let mut footer = FOOTER;
        footer[..4].copy_from_slice(&offset.to_le_bytes());
        data.extend_from_slice(&footer);
    }

    #[test]
    fn fully_transparent_pixels_round_trip() {
        for style in [
            EncodingStyle::NoCompression,
            EncodingStyle::NormalCompression,
        ] {
            for force in [false, true] {
                for color in [[30, 60, 90, 0], [40, 40, 40, 0]] {
                    round_trip(&color.repeat(4), options(style, force));
                }
            }
        }
    }

    #[test]
    fn extension_controls_alpha() {
        for attributes in 0..=4 {
            let mut data = header(2, (0, 0, 0), 32, TOP_DOWN | 8);
            data.extend_from_slice(&[16, 32, 64, 128, 0, 0, 0, 0]);
            extension(&mut data, attributes);
            let expected = match attributes {
                0 | 1 => [64, 32, 16, 255, 0, 0, 0, 255],
                4 => [128, 64, 32, 128, 0, 0, 0, 0],
                _ => [64, 32, 16, 128, 0, 0, 0, 0],
            };
            assert_eq!(crate::decode(&data).unwrap().pixels(), expected);
        }
        let mut data = header(2, (0, 0, 0), 16, TOP_DOWN);
        data.extend_from_slice(&[0, 124, 0, 252]);
        extension(&mut data, 3);
        assert_eq!(
            crate::decode(&data).unwrap().pixels(),
            [255, 0, 0, 0, 255, 0, 0, 255]
        );
    }

    #[test]
    fn invalid_extension_is_rejected() {
        for offset in [1u32, 18, u32::MAX] {
            let mut data = header(2, (0, 0, 0), 24, TOP_DOWN);
            data.extend_from_slice(&[0; 6]);
            let mut footer = FOOTER;
            footer[..4].copy_from_slice(&offset.to_le_bytes());
            data.extend_from_slice(&footer);
            assert_eq!(crate::decode(&data), Err(DecodeError::InvalidData));
        }
    }

    #[test]
    fn gray_alpha_and_wide_palette_indices() {
        for rle in [0, RLE] {
            let mut gray = header(GRAYSCALE | rle, (0, 0, 0), 16, TOP_DOWN | 8);
            if rle != 0 {
                gray.push(1);
            }
            gray.extend_from_slice(&[20, 0, 70, 128]);
            assert_eq!(
                crate::decode(&gray).unwrap().pixels(),
                [20, 20, 20, 0, 70, 70, 70, 128]
            );
            let mut mapped = header(COLOR_MAPPED | rle, (1, 2, 32), 16, TOP_DOWN | 8);
            mapped[3..5].copy_from_slice(&300u16.to_le_bytes());
            mapped.extend_from_slice(&[1, 2, 3, 0, 4, 5, 6, 128]);
            if rle != 0 {
                mapped.push(1);
            }
            mapped.extend_from_slice(&[45, 1, 44, 1]);
            assert_eq!(
                crate::decode(&mapped).unwrap().pixels(),
                [6, 5, 4, 128, 3, 2, 1, 0]
            );
        }
    }

    #[test]
    fn compression_selects_smaller_layouts() {
        for (gray, alpha) in [(true, false), (true, true), (false, false), (false, true)] {
            let pixels: Vec<u8> = (0..1024)
                .flat_map(|i| {
                    let v = if gray {
                        (i % 256) as u8
                    } else {
                        (i % 4 * 50) as u8
                    };
                    [
                        v,
                        if gray { v } else { 100 },
                        v,
                        if alpha { v } else { 255 },
                    ]
                })
                .collect();
            let bitmap = Bitmap::new(256, 4, pixels).unwrap();
            let encoded =
                encode_with_options(&bitmap, Format::Tga, EncodeOptions::default()).unwrap();
            assert_eq!(encoded[2], if gray { GRAYSCALE } else { COLOR_MAPPED });
            assert_eq!(crate::decode(&encoded).unwrap().pixels(), bitmap.data());
            let forced = encode_with_options(
                &bitmap,
                Format::Tga,
                options(EncodingStyle::NormalCompression, true),
            )
            .unwrap();
            assert_eq!(forced[16], 32);
            assert_eq!(crate::decode(&forced).unwrap().pixels(), bitmap.data());
        }
    }

    #[test]
    fn palette_limits_and_packet_boundaries_round_trip() {
        for colors in [1, 2, 256, 257] {
            for width in [1, 2, 127, 128, 129, 300] {
                let pixels = (0..width * 3)
                    .flat_map(|i| {
                        let v = (i % colors) as u16;
                        [v as u8, (v >> 8) as u8, 100, 255]
                    })
                    .collect();
                let bitmap = Bitmap::new(width, 3, pixels).unwrap();
                let none = encode_with_options(
                    &bitmap,
                    Format::Tga,
                    options(EncodingStyle::NoCompression, false),
                )
                .unwrap();
                let normal =
                    encode_with_options(&bitmap, Format::Tga, EncodeOptions::default()).unwrap();
                assert!(normal.len() <= none.len());
                assert_eq!(crate::decode(&normal).unwrap().pixels(), bitmap.data());
            }
        }
    }

    #[test]
    fn extension_alpha_applies_to_gray_and_palette() {
        let mut gray = header(GRAYSCALE, (0, 0, 0), 16, TOP_DOWN);
        gray.extend_from_slice(&[32, 128, 0, 0]);
        extension(&mut gray, 4);
        assert_eq!(
            crate::decode(&gray).unwrap().pixels(),
            [64, 64, 64, 128, 0, 0, 0, 0]
        );
        let mut mapped = header(COLOR_MAPPED, (1, 1, 32), 8, TOP_DOWN);
        mapped.extend_from_slice(&[10, 20, 30, 0, 0, 0]);
        extension(&mut mapped, 3);
        assert_eq!(
            crate::decode(&mapped).unwrap().pixels(),
            [30, 20, 10, 0, 30, 20, 10, 0]
        );
    }

    #[test]
    fn unset_alpha_is_opaque() {
        let mut data = header(2, (0, 0, 0), 32, TOP_DOWN);
        data.extend_from_slice(&[1, 2, 3, 0, 4, 5, 6, 0]);
        assert_eq!(
            crate::decode(&data).unwrap().pixels(),
            [3, 2, 1, 255, 6, 5, 4, 255]
        );
    }
}
