/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use alloc::vec;
use alloc::vec::Vec;

use super::color::{ColorModel, Transform};
use super::metadata::{ExifData, ORIENTATION, orient};
use super::{Budget, DecodeError, Format, Image, Result, jpeg, pixel_len};

const NEW_SUBFILE_TYPE: u16 = 254;
const IMAGE_WIDTH: u16 = 256;
const IMAGE_LENGTH: u16 = 257;
const BITS_PER_SAMPLE: u16 = 258;
const COMPRESSION: u16 = 259;
const PHOTOMETRIC: u16 = 262;
const FILL_ORDER: u16 = 266;
const STRIP_OFFSETS: u16 = 273;
const SAMPLES_PER_PIXEL: u16 = 277;
const ROWS_PER_STRIP: u16 = 278;
const STRIP_BYTE_COUNTS: u16 = 279;
const PLANAR_CONFIGURATION: u16 = 284;
const T4_OPTIONS: u16 = 292;
const PREDICTOR: u16 = 317;
const COLOR_MAP: u16 = 320;
const TILE_WIDTH: u16 = 322;
const TILE_LENGTH: u16 = 323;
const TILE_OFFSETS: u16 = 324;
const TILE_BYTE_COUNTS: u16 = 325;
const EXTRA_SAMPLES: u16 = 338;
const SAMPLE_FORMAT: u16 = 339;
const JPEG_TABLES: u16 = 347;
const ICC_PROFILE: u16 = 34675;

const NONE: u32 = 1;
const HUFFMAN: u32 = 2;
const GROUP3: u32 = 3;
const GROUP4: u32 = 4;
const LZW: u32 = 5;
const JPEG: u32 = 7;
const DEFLATE: u32 = 8;
const PACKBITS: u32 = 32773;
const ADOBE_DEFLATE: u32 = 32946;

const WHITE_IS_ZERO: u32 = 0;
const BLACK_IS_ZERO: u32 = 1;
const RGB: u32 = 2;
const PALETTE: u32 = 3;
const SEPARATED: u32 = 5;
const YCBCR: u32 = 6;

const HORIZONTAL: u32 = 2;
const FLOATING_POINT: u32 = 3;

const FLOAT: u32 = 3;

// Directories followed before giving up on a chain that loops.
const MAX_DIRECTORIES: usize = 256;

// MARK: Directory
struct Directory<'a> {
    tiff: &'a ExifData<'a>,
    data: &'a [u8],
    at: usize,
}

impl<'a> Directory<'a> {
    // The value position, type and count of a tag's entry.
    fn entry(&self, tag: u16) -> Result<Option<(usize, u32, usize)>> {
        let count = self
            .tiff
            .read(self.at, 2)
            .ok_or(DecodeError::InvalidHeader)? as usize;
        for index in 0..count {
            let entry = self.at + 2 + index * 12;
            let read = |offset, length| {
                self.tiff
                    .read(entry + offset, length)
                    .ok_or(DecodeError::InvalidHeader)
            };
            if read(0, 2)? != u32::from(tag) {
                continue;
            }
            let kind = read(2, 2)?;
            let count = read(4, 4)? as usize;
            let size = match kind {
                3 => 2,
                4 => 4,
                _ => 1,
            };
            let length = count.checked_mul(size).ok_or(DecodeError::InvalidHeader)?;
            let at = if length <= 4 {
                entry + 8
            } else {
                read(8, 4)? as usize
            };
            if at
                .checked_add(length)
                .is_none_or(|end| end > self.data.len())
            {
                return Err(DecodeError::InvalidHeader);
            }
            return Ok(Some((at, kind, count)));
        }
        Ok(None)
    }

    // The numbers of a byte, short or long entry.
    fn values(&self, tag: u16) -> Result<Option<Vec<u32>>> {
        let Some((at, kind, count)) = self.entry(tag)? else {
            return Ok(None);
        };
        let size = match kind {
            1 => 1,
            3 => 2,
            4 => 4,
            _ => return Err(DecodeError::InvalidHeader),
        };
        let mut values = Vec::new();
        values
            .try_reserve_exact(count)
            .map_err(|_| DecodeError::ImageTooLarge)?;
        for index in 0..count {
            values.push(
                self.tiff
                    .read(at + index * size, size)
                    .ok_or(DecodeError::InvalidHeader)?,
            );
        }
        Ok(Some(values))
    }

    // The value of an entry that has to be the same for every sample.
    fn uniform(&self, tag: u16, default: u32) -> Result<u32> {
        let Some(values) = self.values(tag)? else {
            return Ok(default);
        };
        let first = *values.first().ok_or(DecodeError::InvalidHeader)?;
        if values.iter().any(|value| *value != first) {
            return Err(DecodeError::UnsupportedFeature);
        }
        Ok(first)
    }

    fn value(&self, tag: u16, default: u32) -> Result<u32> {
        Ok(match self.values(tag)? {
            Some(values) => *values.first().ok_or(DecodeError::InvalidHeader)?,
            None => default,
        })
    }

    fn bytes(&self, tag: u16) -> Result<Option<&'a [u8]>> {
        Ok(self
            .entry(tag)?
            .map(|(at, _, count)| &self.data[at..at + count]))
    }

    fn next(&self) -> Option<usize> {
        let count = self.tiff.read(self.at, 2)? as usize;
        let next = self.tiff.read(self.at + 2 + count * 12, 4)? as usize;
        (next != 0).then_some(next)
    }
}

// MARK: Decoder
// How the samples of a pixel turn into RGBA.
struct Color<'a> {
    photometric: u32,
    // The 8-bit value of every sample value.
    scale: Vec<u8>,
    // The colors of palette indices.
    palette: Vec<[u8; 3]>,
    // The sample index of the alpha, and whether colors are premultiplied by it.
    alpha: Option<(usize, bool)>,
    cmyk: Option<&'a Transform>,
}

impl Color<'_> {
    fn rgba(&self, samples: &[u16]) -> [u8; 4] {
        let scale = |index: usize| self.scale[usize::from(samples[index])];
        let [r, g, b] = match self.photometric {
            WHITE_IS_ZERO => [255 - scale(0); 3],
            BLACK_IS_ZERO => [scale(0); 3],
            PALETTE => self.palette[usize::from(samples[0])],
            SEPARATED => {
                let ink = [scale(0), scale(1), scale(2), scale(3)];
                match self.cmyk {
                    Some(transform) => transform.cmyk(ink),
                    None => core::array::from_fn(|k| {
                        ((u16::from(255 - ink[k]) * u16::from(255 - ink[3]) + 127) / 255) as u8
                    }),
                }
            }
            _ => [scale(0), scale(1), scale(2)],
        };
        let Some((index, premultiplied)) = self.alpha else {
            return [r, g, b, 255];
        };
        let a = scale(index);
        if !premultiplied {
            return [r, g, b, a];
        }
        let straight = |value: u8| match a {
            0 => 0,
            _ => ((u32::from(value) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8,
        };
        [straight(r), straight(g), straight(b), a]
    }
}

// Where a strip or tile lies in the image.
#[derive(Clone, Copy)]
struct Block {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}

pub(super) fn decode(data: &[u8]) -> Result<Image> {
    let tiff = ExifData::new(data).ok_or(DecodeError::InvalidMagic)?;
    let directory = select(&tiff, data)?;
    let width = directory.value(IMAGE_WIDTH, 0)?;
    let height = directory.value(IMAGE_LENGTH, 0)?;
    let mut budget = Budget::default();
    let mut pixels = budget.zeroed::<u8>(pixel_len(width, height)?)?;
    let (width, height) = (width as usize, height as usize);

    let samples = directory.value(SAMPLES_PER_PIXEL, 1)? as usize;
    let bits = directory.uniform(BITS_PER_SAMPLE, 1)?;
    // Unsigned integers, or IEEE floats; void samples are read as unsigned.
    let float = match directory.uniform(SAMPLE_FORMAT, 1)? {
        1 | 4 => false,
        FLOAT => true,
        _ => return Err(DecodeError::UnsupportedFeature),
    };
    let compression = directory.value(COMPRESSION, NONE)?;
    let photometric =
        directory.value(PHOTOMETRIC, if samples >= 3 { RGB } else { BLACK_IS_ZERO })?;
    let channels = match photometric {
        WHITE_IS_ZERO | BLACK_IS_ZERO | PALETTE => 1,
        RGB => 3,
        YCBCR if compression == JPEG => 3,
        SEPARATED => 4,
        _ => return Err(DecodeError::UnsupportedFeature),
    };
    let supported_bits = if float {
        matches!(bits, 16 | 32 | 64)
    } else {
        (1..=32).contains(&bits)
    };
    if samples < channels || samples > 16 || !supported_bits {
        return Err(DecodeError::UnsupportedFeature);
    }
    let separate = samples > 1 && directory.value(PLANAR_CONFIGURATION, 1)? == 2;

    let tiled = directory.entry(TILE_OFFSETS)?.is_some();
    let (block_width, block_height, offsets, counts) = if tiled {
        (
            directory.value(TILE_WIDTH, 0)? as usize,
            directory.value(TILE_LENGTH, 0)? as usize,
            TILE_OFFSETS,
            TILE_BYTE_COUNTS,
        )
    } else {
        (
            width,
            (directory.value(ROWS_PER_STRIP, u32::MAX)? as usize).min(height),
            STRIP_OFFSETS,
            STRIP_BYTE_COUNTS,
        )
    };
    if block_width == 0 || block_height == 0 {
        return Err(DecodeError::InvalidHeader);
    }
    let offsets = directory
        .values(offsets)?
        .ok_or(DecodeError::InvalidHeader)?;
    let counts = directory.values(counts)?;
    let (across, down) = (width.div_ceil(block_width), height.div_ceil(block_height));
    let planes = if separate { samples } else { 1 };
    let blocks = across * down;
    let needed = blocks
        .checked_mul(planes)
        .ok_or(DecodeError::ImageTooLarge)?;
    if offsets.len() < needed || counts.as_ref().is_some_and(|counts| counts.len() < needed) {
        return Err(DecodeError::InvalidData);
    }
    // Uncompressed strips without byte counts fill their rows.
    let row_bytes = (block_width * (samples / planes) * bits as usize).div_ceil(8);
    let block_bytes = row_bytes
        .checked_mul(block_height)
        .ok_or(DecodeError::ImageTooLarge)?;
    let block_data = |index: usize| {
        let start = offsets[index] as usize;
        let length = match &counts {
            Some(counts) => counts[index] as usize,
            None if compression == NONE => block_bytes,
            None => return Err(DecodeError::InvalidHeader),
        };
        data.get(start..start.saturating_add(length))
            .ok_or(DecodeError::InvalidData)
    };
    let icc = directory.bytes(ICC_PROFILE)?;
    let model = match channels {
        1 => ColorModel::Gray,
        3 => ColorModel::Rgb,
        _ => ColorModel::Cmyk,
    };
    let transform = icc.and_then(|icc| Transform::icc(icc, model));

    if compression == JPEG {
        if separate || bits != 8 || float || samples != channels || photometric == WHITE_IS_ZERO {
            return Err(DecodeError::UnsupportedFeature);
        }
        let tables = directory.bytes(JPEG_TABLES)?.filter(|t| t.len() > 4);
        for index in 0..blocks {
            let block = block_at(index, across, block_width, block_height, width, height);
            let tile = jpeg::decode_tiff(tables, block_data(index)?, photometric == RGB)?;
            let (tile_width, tile_height) = (tile.width() as usize, tile.height() as usize);
            for (y, row) in tile
                .pixels()
                .chunks_exact(tile_width * 4)
                .take(block.height.min(tile_height))
                .enumerate()
            {
                let length = block.width.min(tile_width) * 4;
                let start = ((block.y + y) * width + block.x) * 4;
                pixels[start..start + length].copy_from_slice(&row[..length]);
            }
        }
    } else {
        let codec = match compression {
            NONE => Codec::None,
            LZW => Codec::Lzw,
            DEFLATE | ADOBE_DEFLATE => Codec::Deflate,
            PACKBITS => Codec::PackBits,
            HUFFMAN | GROUP3 | GROUP4 if bits == 1 && samples == 1 => {
                Codec::Fax(Fax::new(match compression {
                    HUFFMAN => Coding::Huffman,
                    GROUP3 => Coding::Group3 {
                        two_d: directory.value(T4_OPTIONS, 0)? & 1 != 0,
                    },
                    _ => Coding::Group4,
                }))
            }
            _ => return Err(DecodeError::UnsupportedFeature),
        };
        // Samples are kept with at most 16 bits, and floats are scaled from 0 to 1.
        let value_bits = if float { 16 } else { bits.min(16) };
        let max = (1u32 << value_bits) - 1;
        let scale = (0..=max)
            .map(|value| ((value * 255 + max / 2) / max) as u8)
            .collect();
        let palette = if photometric == PALETTE {
            if float || bits > 16 {
                return Err(DecodeError::UnsupportedFeature);
            }
            let map = directory
                .values(COLOR_MAP)?
                .filter(|map| map.len() >= 3 << bits)
                .ok_or(DecodeError::InvalidHeader)?;
            let count = 1 << bits;
            // Some writers store 8-bit palette values instead of 16-bit ones.
            let eight = map[..3 * count].iter().all(|value| *value < 256);
            let channel = |value: u32| {
                if eight {
                    value as u8
                } else {
                    ((value * 255 + 32767) / 65535) as u8
                }
            };
            (0..count)
                .map(|i| [map[i], map[count + i], map[2 * count + i]].map(channel))
                .collect()
        } else {
            Vec::new()
        };
        let extra = directory.values(EXTRA_SAMPLES)?.unwrap_or_default();
        // Unspecified extra samples of images with more than three samples are alpha, like
        // libtiff reads them.
        let alpha = match extra.first() {
            Some(1) => Some(true),
            Some(2) => Some(false),
            Some(0) if samples > 3 => Some(true),
            _ => None,
        };
        let color = Color {
            photometric,
            scale,
            palette,
            alpha: alpha
                .filter(|_| samples > channels)
                .map(|premultiplied| (channels, premultiplied)),
            cmyk: transform.as_ref().filter(|_| model == ColorModel::Cmyk),
        };
        let little = data[0] == b'I';
        let reverse = directory.value(FILL_ORDER, 1)? == 2;
        let predictor = directory.value(PREDICTOR, 1)?;
        if !matches!(
            (predictor, float),
            (1, _) | (HORIZONTAL, false) | (FLOATING_POINT, true)
        ) {
            return Err(DecodeError::UnsupportedFeature);
        }
        let mut values = budget.zeroed::<u16>(
            block_width
                .checked_mul(block_height)
                .and_then(|n| n.checked_mul(samples))
                .ok_or(DecodeError::ImageTooLarge)?,
        )?;
        let channels = samples / planes;
        let mut raw = Vec::new();
        let mut row_samples = budget.zeroed::<u64>(block_width * channels)?;
        let (mut accumulated, mut ordered) = (Vec::new(), Vec::new());
        for index in 0..blocks {
            let block = block_at(index, across, block_width, block_height, width, height);
            // Strips stop at the last row, while tiles are always full.
            let rows = if tiled { block_height } else { block.height };
            for plane in 0..planes {
                let mut input = block_data(plane * blocks + index)?;
                let reversed;
                if reverse {
                    reversed = input.iter().map(|b| b.reverse_bits()).collect::<Vec<_>>();
                    input = &reversed;
                }
                let expected = row_bytes * rows;
                budget.claim(expected)?;
                raw.clear();
                raw.try_reserve(expected)
                    .map_err(|_| DecodeError::ImageTooLarge)?;
                codec.decompress(input, &mut raw, expected, block_width)?;
                // Short strips are read as if padded with zeros.
                raw.resize(expected, 0);
                for (y, row) in raw.chunks_exact(row_bytes).enumerate() {
                    // The floating-point predictor stores the bytes of each sample in planes
                    // from the most significant one, differenced across the row.
                    let (row, little) = if predictor == FLOATING_POINT {
                        accumulated.clear();
                        accumulated.extend_from_slice(row);
                        for i in channels..accumulated.len() {
                            accumulated[i] = accumulated[i].wrapping_add(accumulated[i - channels]);
                        }
                        let (bytes, count) = (bits as usize / 8, row_samples.len());
                        ordered.resize(accumulated.len(), 0);
                        for (i, byte) in ordered.iter_mut().enumerate() {
                            *byte = accumulated[i % bytes * count + i / bytes];
                        }
                        (&ordered[..], false)
                    } else {
                        (row, little)
                    };
                    for (i, value) in row_samples.iter_mut().enumerate() {
                        *value = sample(row, i, bits, little);
                    }
                    if predictor == HORIZONTAL {
                        let mask = u64::MAX >> (64 - bits);
                        for i in channels..row_samples.len() {
                            row_samples[i] =
                                row_samples[i].wrapping_add(row_samples[i - channels]) & mask;
                        }
                    }
                    let row_values = &mut values[y * block_width * samples..];
                    for (i, value) in row_samples.iter().enumerate() {
                        row_values[i / channels * samples + plane + i % channels] =
                            reduce(*value, bits, float);
                    }
                }
            }
            for y in 0..block.height {
                let start = ((block.y + y) * width + block.x) * 4;
                let row = pixels[start..start + block.width * 4]
                    .as_chunks_mut::<4>()
                    .0;
                let source = values[y * block_width * samples..].chunks_exact(samples);
                for (pixel, samples) in row.iter_mut().zip(source) {
                    *pixel = color.rgba(samples);
                }
            }
        }
    }
    if let Some(transform) = transform
        && model != ColorModel::Cmyk
    {
        transform.apply(&mut pixels);
    }
    let orientation = directory.value(ORIENTATION, 1)?;
    let (width, height, pixels) = orient(
        width as u32,
        height as u32,
        pixels,
        if (1..=8).contains(&orientation) {
            orientation as u16
        } else {
            1
        },
        &mut budget,
    )?;
    Ok(Image::still(Format::Tiff, width, height, pixels))
}

// The first full-resolution image, skipping reduced-resolution previews before it.
fn select<'a>(tiff: &'a ExifData<'a>, data: &'a [u8]) -> Result<Directory<'a>> {
    let mut at = tiff.first_directory().ok_or(DecodeError::InvalidHeader)?;
    let mut first = None;
    for _ in 0..MAX_DIRECTORIES {
        let directory = Directory { tiff, data, at };
        tiff.read(at, 2).ok_or(DecodeError::InvalidHeader)?;
        if directory.value(NEW_SUBFILE_TYPE, 0)? & 1 == 0 {
            return Ok(directory);
        }
        let next = directory.next();
        first.get_or_insert(directory);
        match next {
            Some(next) => at = next,
            None => break,
        }
    }
    first.ok_or(DecodeError::InvalidHeader)
}

fn block_at(
    index: usize,
    across: usize,
    block_width: usize,
    block_height: usize,
    width: usize,
    height: usize,
) -> Block {
    let (x, y) = (index % across * block_width, index / across * block_height);
    Block {
        x,
        y,
        width: block_width.min(width - x),
        height: block_height.min(height - y),
    }
}

// Reads the sample at an index of a row, packed from the most significant bit.
fn sample(row: &[u8], index: usize, bits: u32, little: bool) -> u64 {
    match bits {
        8 => u64::from(row[index]),
        16 | 32 | 64 => {
            let size = bits as usize / 8;
            let bytes = &row[index * size..][..size];
            let byte = |i: usize| u64::from(bytes[if little { size - 1 - i } else { i }]);
            (0..size).fold(0, |value, i| value << 8 | byte(i))
        }
        _ => {
            let bit = index * bits as usize;
            let window = (0..5).fold(0u64, |window, offset| {
                window << 8 | u64::from(row.get(bit / 8 + offset).copied().unwrap_or(0))
            });
            window >> (40 - bit % 8 - bits as usize) & ((1 << bits) - 1)
        }
    }
}

// Reduces a sample to at most 16 bits, scaling floats from 0 to 1 to the full range.
fn reduce(value: u64, bits: u32, float: bool) -> u16 {
    let unit = |value: f32| (value.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16;
    match (float, bits) {
        (true, 16) => unit(half(value as u16)),
        (true, 32) => unit(f32::from_bits(value as u32)),
        (true, _) => unit(f64::from_bits(value) as f32),
        (false, 17..) => (value >> (bits - 16)) as u16,
        (false, _) => value as u16,
    }
}

// Converts an IEEE half-precision float.
fn half(value: u16) -> f32 {
    let sign = u32::from(value >> 15) << 31;
    let exponent = u32::from(value >> 10 & 0x1f);
    let fraction = u32::from(value & 0x3ff);
    match exponent {
        0 => {
            let magnitude = fraction as f32 / (1 << 24) as f32;
            if sign == 0 { magnitude } else { -magnitude }
        }
        0x1f => f32::from_bits(sign | 0xff << 23 | fraction << 13),
        _ => f32::from_bits(sign | (exponent + 112) << 23 | fraction << 13),
    }
}

// MARK: Compression
enum Codec {
    None,
    Lzw,
    Deflate,
    PackBits,
    Fax(Fax),
}

impl Codec {
    // Decompresses a strip or tile of rows of a width into at most a limit of bytes.
    fn decompress(
        &self,
        input: &[u8],
        out: &mut Vec<u8>,
        limit: usize,
        width: usize,
    ) -> Result<()> {
        match self {
            Self::None => out.extend_from_slice(&input[..input.len().min(limit)]),
            Self::Lzw => lzw(input, out, limit)?,
            Self::Deflate => {
                *out = deflate::zlib_decompress(input, limit).map_err(|error| match error {
                    deflate::Error::OutOfMemory => DecodeError::ImageTooLarge,
                    _ => DecodeError::InvalidData,
                })?;
            }
            Self::PackBits => packbits(input, out, limit),
            Self::Fax(fax) => fax.decode(input, width, limit / width.div_ceil(8), out)?,
        }
        Ok(())
    }
}

// Decodes LZW codes of 9 to 12 bits, packed from the most significant bit, which widen one
// code early.
fn lzw(input: &[u8], out: &mut Vec<u8>, limit: usize) -> Result<()> {
    const CLEAR: usize = 256;
    const END: usize = 257;
    // Old-style LZW codes are packed from the least significant bit.
    if input.first() == Some(&0) && input.get(1).is_some_and(|b| b & 1 != 0) {
        return Err(DecodeError::UnsupportedFeature);
    }
    let mut prefix = [0u16; 4096];
    let mut suffix: [u8; 4096] = core::array::from_fn(|code| code as u8);
    let mut length = [1u16; 4096];
    let (mut next, mut width, mut previous) = (END + 1, 9, None);
    let (mut bits, mut count) = (0u32, 0);
    let mut bytes = input.iter();
    while out.len() < limit {
        while count < width {
            let Some(&byte) = bytes.next() else {
                return Ok(());
            };
            bits = bits << 8 | u32::from(byte);
            count += 8;
        }
        count -= width;
        let code = (bits >> count) as usize & ((1 << width) - 1);
        bits &= (1 << count) - 1;
        if code == CLEAR {
            (next, width, previous) = (END + 1, 9, None);
            continue;
        }
        if code == END {
            break;
        }
        let Some(last) = previous else {
            if code > 255 {
                return Err(DecodeError::InvalidData);
            }
            out.push(code as u8);
            previous = Some(code);
            continue;
        };
        let string = if code < next {
            code
        } else if code == next {
            last
        } else {
            return Err(DecodeError::InvalidData);
        };
        let start = out.len();
        out.resize(start + usize::from(length[string]), 0);
        let mut link = string;
        for byte in out[start..].iter_mut().rev() {
            *byte = suffix[link];
            link = usize::from(prefix[link]);
        }
        let first = out[start];
        if code == next {
            out.push(first);
        }
        if next < 4096 {
            prefix[next] = last as u16;
            suffix[next] = first;
            length[next] = length[last] + 1;
            next += 1;
        }
        if next + 1 >= 1 << width && width < 12 {
            width += 1;
        }
        previous = Some(code);
    }
    out.truncate(limit);
    Ok(())
}

fn packbits(input: &[u8], out: &mut Vec<u8>, limit: usize) {
    let mut bytes = input;
    while let [header, rest @ ..] = bytes
        && out.len() < limit
    {
        let header = *header as i8;
        if header >= 0 {
            let (literal, rest) = rest.split_at(rest.len().min(header as usize + 1));
            out.extend_from_slice(literal);
            bytes = rest;
        } else if header == -128 {
            bytes = rest;
        } else if let [value, rest @ ..] = rest {
            out.resize(out.len() + usize::from(header.unsigned_abs()) + 1, *value);
            bytes = rest;
        } else {
            break;
        }
    }
    out.truncate(limit);
}

// MARK: Fax
// CCITT T.4 and T.6 bilevel coding, as compressions 2, 3, and 4. Rows are written as packed
// bits with white as 0 and black as 1.

/// How the rows of a strip are coded.
#[derive(Clone, Copy)]
enum Coding {
    /// Modified Huffman rows that each start on a byte boundary.
    Huffman,
    /// T.4 rows after optional end-of-line codes, 2D coded after a tag bit when enabled.
    Group3 { two_d: bool },
    /// T.6 rows, all 2D coded.
    Group4,
}

// The (code, length, run) of the terminating and makeup codes of each color.
#[rustfmt::skip]
const WHITE: [(u16, u8, u16); 91] = [
    (0b00110101, 8, 0), (0b000111, 6, 1), (0b0111, 4, 2), (0b1000, 4, 3), (0b1011, 4, 4),
    (0b1100, 4, 5), (0b1110, 4, 6), (0b1111, 4, 7), (0b10011, 5, 8), (0b10100, 5, 9),
    (0b00111, 5, 10), (0b01000, 5, 11), (0b001000, 6, 12), (0b000011, 6, 13),
    (0b110100, 6, 14), (0b110101, 6, 15), (0b101010, 6, 16), (0b101011, 6, 17),
    (0b0100111, 7, 18), (0b0001100, 7, 19), (0b0001000, 7, 20), (0b0010111, 7, 21),
    (0b0000011, 7, 22), (0b0000100, 7, 23), (0b0101000, 7, 24), (0b0101011, 7, 25),
    (0b0010011, 7, 26), (0b0100100, 7, 27), (0b0011000, 7, 28), (0b00000010, 8, 29),
    (0b00000011, 8, 30), (0b00011010, 8, 31), (0b00011011, 8, 32), (0b00010010, 8, 33),
    (0b00010011, 8, 34), (0b00010100, 8, 35), (0b00010101, 8, 36), (0b00010110, 8, 37),
    (0b00010111, 8, 38), (0b00101000, 8, 39), (0b00101001, 8, 40), (0b00101010, 8, 41),
    (0b00101011, 8, 42), (0b00101100, 8, 43), (0b00101101, 8, 44), (0b00000100, 8, 45),
    (0b00000101, 8, 46), (0b00001010, 8, 47), (0b00001011, 8, 48), (0b01010010, 8, 49),
    (0b01010011, 8, 50), (0b01010100, 8, 51), (0b01010101, 8, 52), (0b00100100, 8, 53),
    (0b00100101, 8, 54), (0b01011000, 8, 55), (0b01011001, 8, 56), (0b01011010, 8, 57),
    (0b01011011, 8, 58), (0b01001010, 8, 59), (0b01001011, 8, 60), (0b00110010, 8, 61),
    (0b00110011, 8, 62), (0b00110100, 8, 63),
    (0b11011, 5, 64), (0b10010, 5, 128), (0b010111, 6, 192), (0b0110111, 7, 256),
    (0b00110110, 8, 320), (0b00110111, 8, 384), (0b01100100, 8, 448), (0b01100101, 8, 512),
    (0b01101000, 8, 576), (0b01100111, 8, 640), (0b011001100, 9, 704),
    (0b011001101, 9, 768), (0b011010010, 9, 832), (0b011010011, 9, 896),
    (0b011010100, 9, 960), (0b011010101, 9, 1024), (0b011010110, 9, 1088),
    (0b011010111, 9, 1152), (0b011011000, 9, 1216), (0b011011001, 9, 1280),
    (0b011011010, 9, 1344), (0b011011011, 9, 1408), (0b010011000, 9, 1472),
    (0b010011001, 9, 1536), (0b010011010, 9, 1600), (0b011000, 6, 1664),
    (0b010011011, 9, 1728),
];

#[rustfmt::skip]
const BLACK: [(u16, u8, u16); 91] = [
    (0b0000110111, 10, 0), (0b010, 3, 1), (0b11, 2, 2), (0b10, 2, 3), (0b011, 3, 4),
    (0b0011, 4, 5), (0b0010, 4, 6), (0b00011, 5, 7), (0b000101, 6, 8), (0b000100, 6, 9),
    (0b0000100, 7, 10), (0b0000101, 7, 11), (0b0000111, 7, 12), (0b00000100, 8, 13),
    (0b00000111, 8, 14), (0b000011000, 9, 15), (0b0000010111, 10, 16),
    (0b0000011000, 10, 17), (0b0000001000, 10, 18), (0b00001100111, 11, 19),
    (0b00001101000, 11, 20), (0b00001101100, 11, 21), (0b00000110111, 11, 22),
    (0b00000101000, 11, 23), (0b00000010111, 11, 24), (0b00000011000, 11, 25),
    (0b000011001010, 12, 26), (0b000011001011, 12, 27), (0b000011001100, 12, 28),
    (0b000011001101, 12, 29), (0b000001101000, 12, 30), (0b000001101001, 12, 31),
    (0b000001101010, 12, 32), (0b000001101011, 12, 33), (0b000011010010, 12, 34),
    (0b000011010011, 12, 35), (0b000011010100, 12, 36), (0b000011010101, 12, 37),
    (0b000011010110, 12, 38), (0b000011010111, 12, 39), (0b000001101100, 12, 40),
    (0b000001101101, 12, 41), (0b000011011010, 12, 42), (0b000011011011, 12, 43),
    (0b000001010100, 12, 44), (0b000001010101, 12, 45), (0b000001010110, 12, 46),
    (0b000001010111, 12, 47), (0b000001100100, 12, 48), (0b000001100101, 12, 49),
    (0b000001010010, 12, 50), (0b000001010011, 12, 51), (0b000000100100, 12, 52),
    (0b000000110111, 12, 53), (0b000000111000, 12, 54), (0b000000100111, 12, 55),
    (0b000000101000, 12, 56), (0b000001011000, 12, 57), (0b000001011001, 12, 58),
    (0b000000101011, 12, 59), (0b000000101100, 12, 60), (0b000001011010, 12, 61),
    (0b000001100110, 12, 62), (0b000001100111, 12, 63),
    (0b0000001111, 10, 64), (0b000011001000, 12, 128), (0b000011001001, 12, 192),
    (0b000001011011, 12, 256), (0b000000110011, 12, 320), (0b000000110100, 12, 384),
    (0b000000110101, 12, 448), (0b0000001101100, 13, 512), (0b0000001101101, 13, 576),
    (0b0000001001010, 13, 640), (0b0000001001011, 13, 704), (0b0000001001100, 13, 768),
    (0b0000001001101, 13, 832), (0b0000001110010, 13, 896), (0b0000001110011, 13, 960),
    (0b0000001110100, 13, 1024), (0b0000001110101, 13, 1088),
    (0b0000001110110, 13, 1152), (0b0000001110111, 13, 1216),
    (0b0000001010010, 13, 1280), (0b0000001010011, 13, 1344),
    (0b0000001010100, 13, 1408), (0b0000001010101, 13, 1472),
    (0b0000001011010, 13, 1536), (0b0000001011011, 13, 1600),
    (0b0000001100100, 13, 1664), (0b0000001100101, 13, 1728),
];

// Makeup codes both colors share, for runs from 1792 pixels.
#[rustfmt::skip]
const EXTENDED: [(u16, u8, u16); 13] = [
    (0b00000001000, 11, 1792), (0b00000001100, 11, 1856), (0b00000001101, 11, 1920),
    (0b000000010010, 12, 1984), (0b000000010011, 12, 2048), (0b000000010100, 12, 2112),
    (0b000000010101, 12, 2176), (0b000000010110, 12, 2240), (0b000000010111, 12, 2304),
    (0b000000011100, 12, 2368), (0b000000011101, 12, 2432), (0b000000011110, 12, 2496),
    (0b000000011111, 12, 2560),
];

// The longest code, which indexes the run lookup tables.
const LOOKUP_BITS: u32 = 13;
const END_OF_LINE: u32 = 0b0000_0000_0001;

enum Mode {
    Pass,
    Horizontal,
    Vertical(isize),
}

struct Bits<'a> {
    data: &'a [u8],
    at: usize,
}

impl Bits<'_> {
    // The next bits, as zeros past the end.
    fn peek(&self, count: u32) -> u32 {
        let byte = self.at / 8;
        let window = (0..3).fold(0, |window, offset| {
            window << 8 | u32::from(self.data.get(byte + offset).copied().unwrap_or(0))
        });
        window >> (24 - self.at % 8 - count as usize) & ((1 << count) - 1)
    }

    const fn skip(&mut self, count: u32) {
        self.at += count as usize;
    }

    const fn done(&self) -> bool {
        self.at >= self.data.len() * 8
    }

    // Skips an end-of-line code after any zero fill bits.
    fn end_of_line(&mut self) {
        let start = self.at;
        while !self.done() && self.peek(1) == 0 {
            self.skip(1);
        }
        if self.at - start >= 11 && self.peek(1) == 1 {
            self.skip(1);
        } else {
            self.at = start;
        }
    }

    fn mode(&mut self) -> Result<Mode> {
        let code = self.peek(7);
        let (length, mode) = match code {
            0b1000000.. => (1, Mode::Vertical(0)),
            0b0110000.. => (3, Mode::Vertical(1)),
            0b0100000.. => (3, Mode::Vertical(-1)),
            0b0010000.. => (3, Mode::Horizontal),
            0b0001000.. => (4, Mode::Pass),
            0b0000110.. => (6, Mode::Vertical(2)),
            0b0000100.. => (6, Mode::Vertical(-2)),
            0b0000011 => (7, Mode::Vertical(3)),
            0b0000010 => (7, Mode::Vertical(-3)),
            // Uncompressed extensions and misplaced end-of-line codes.
            _ => return Err(DecodeError::InvalidData),
        };
        self.skip(length);
        Ok(mode)
    }
}

struct Fax {
    coding: Coding,
    // The code length and run of every 13-bit prefix, or zero for no code.
    white: Vec<u32>,
    black: Vec<u32>,
}

impl Fax {
    fn new(coding: Coding) -> Self {
        let table = |codes: &[(u16, u8, u16)]| {
            let mut table = vec![0; 1 << LOOKUP_BITS];
            for &(code, length, run) in codes.iter().chain(&EXTENDED) {
                let shift = LOOKUP_BITS - u32::from(length);
                let start = usize::from(code) << shift;
                table[start..start + (1 << shift)].fill(u32::from(length) << 16 | u32::from(run));
            }
            table
        };
        Self {
            coding,
            white: table(&WHITE),
            black: table(&BLACK),
        }
    }

    /// Decodes rows of a width into packed bits.
    fn decode(&self, input: &[u8], width: usize, rows: usize, out: &mut Vec<u8>) -> Result<()> {
        let mut bits = Bits { data: input, at: 0 };
        // The changing elements of the previous row, starting from an all-white row, and of
        // the current row. A row is white until its first change.
        let mut reference = vec![width; 3];
        let mut line = Vec::new();
        for _ in 0..rows {
            let two_d = match self.coding {
                Coding::Huffman => false,
                Coding::Group4 => true,
                Coding::Group3 { two_d } => {
                    bits.end_of_line();
                    let tag = two_d && bits.peek(1) == 0;
                    if two_d {
                        bits.skip(1);
                    }
                    tag
                }
            };
            // Truncated strips and end-of-block codes leave the remaining rows white.
            if bits.done() || bits.peek(12) == END_OF_LINE {
                break;
            }
            line.clear();
            if two_d {
                self.row_2d(&mut bits, width, &reference, &mut line)?;
            } else {
                self.row_1d(&mut bits, width, &mut line)?;
            }
            let start = out.len();
            out.resize(start + width.div_ceil(8), 0);
            let row = &mut out[start..];
            for span in line.chunks(2) {
                for x in span[0]..span.get(1).copied().unwrap_or(width) {
                    row[x / 8] |= 0x80 >> (x % 8);
                }
            }
            if matches!(self.coding, Coding::Huffman) {
                bits.at = bits.at.next_multiple_of(8);
            }
            core::mem::swap(&mut reference, &mut line);
            reference.extend([width; 3]);
        }
        Ok(())
    }

    // Reads a run of a color: makeup codes and a terminating code.
    fn run(&self, bits: &mut Bits<'_>, black: bool) -> Result<usize> {
        let table = if black { &self.black } else { &self.white };
        let mut total = 0;
        loop {
            let entry = table[bits.peek(LOOKUP_BITS) as usize];
            if entry == 0 || bits.done() {
                return Err(DecodeError::InvalidData);
            }
            bits.skip(entry >> 16);
            let run = (entry & 0xffff) as usize;
            total += run;
            if run < 64 {
                return Ok(total);
            }
        }
    }

    fn row_1d(&self, bits: &mut Bits<'_>, width: usize, line: &mut Vec<usize>) -> Result<()> {
        let (mut position, mut black) = (0, false);
        while position < width {
            position += self.run(bits, black)?;
            if position > width {
                return Err(DecodeError::InvalidData);
            }
            if position < width {
                line.push(position);
            }
            black = !black;
        }
        Ok(())
    }

    fn row_2d(
        &self,
        bits: &mut Bits<'_>,
        width: usize,
        reference: &[usize],
        line: &mut Vec<usize>,
    ) -> Result<()> {
        // Changing elements at even indices start black runs.
        let (mut a0, mut black, mut index) = (-1isize, false, 0);
        while a0 < width as isize {
            // The first change on the reference row right of a0 to the other color.
            while index > 0 && reference[index - 1] as isize > a0 {
                index -= 1;
            }
            while reference[index] as isize <= a0 {
                index += 1;
            }
            if (index % 2 == 1) != black {
                index += 1;
            }
            let (b1, b2) = (reference[index], reference[index + 1]);
            let start = a0.max(0) as usize;
            match bits.mode()? {
                Mode::Pass => a0 = b2 as isize,
                Mode::Horizontal => {
                    let a1 = start + self.run(bits, black)?;
                    let a2 = a1 + self.run(bits, !black)?;
                    if a2 > width {
                        return Err(DecodeError::InvalidData);
                    }
                    line.extend([a1, a2].into_iter().filter(|a| *a < width));
                    a0 = a2 as isize;
                }
                Mode::Vertical(offset) => {
                    let a1 = b1 as isize + offset;
                    if a1 < start as isize || a1 > width as isize {
                        return Err(DecodeError::InvalidData);
                    }
                    if (a1 as usize) < width {
                        line.push(a1 as usize);
                    }
                    a0 = a1;
                    black = !black;
                }
            }
            if bits.done() && a0 < width as isize {
                return Err(DecodeError::InvalidData);
            }
        }
        Ok(())
    }
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_follow_extensions_and_signatures() {
        for extension in ["tif", "TIFF"] {
            assert_eq!(Format::from_extension(extension), Some(Format::Tiff));
        }
        assert_eq!(Format::from_signature(b"II*\0"), Some(Format::Tiff));
        assert_eq!(Format::from_signature(b"MM\0*"), Some(Format::Tiff));
        // BigTIFF is not supported.
        assert_eq!(Format::from_signature(b"II+\0"), None);
        assert!(!Format::Tiff.can_encode());
    }

    #[test]
    fn reduced_resolution_directories_are_skipped() {
        // A 1x1 preview of a black pixel, then the full 2x1 white image.
        let mut data = b"II*\0\x08\0\0\0".to_vec();
        let directory = |data: &mut Vec<u8>, subfile: u16, width: u16, value: u8, last: bool| {
            let next = if last {
                0
            } else {
                data.len() as u32 + 2 + 7 * 12 + 4 + u32::from(width)
            };
            let pixels = data.len() as u16 + 2 + 7 * 12 + 4;
            data.extend_from_slice(&7u16.to_le_bytes());
            for (tag, value) in [
                (NEW_SUBFILE_TYPE, subfile),
                (IMAGE_WIDTH, width),
                (IMAGE_LENGTH, 1),
                (BITS_PER_SAMPLE, 8),
                (PHOTOMETRIC, BLACK_IS_ZERO as u16),
                (STRIP_OFFSETS, pixels),
                (STRIP_BYTE_COUNTS, width),
            ] {
                data.extend_from_slice(&tag.to_le_bytes());
                data.extend_from_slice(&[3, 0, 1, 0, 0, 0]);
                data.extend_from_slice(&u32::from(value).to_le_bytes());
            }
            data.extend_from_slice(&next.to_le_bytes());
            data.resize(data.len() + usize::from(width), value);
        };
        directory(&mut data, 1, 1, 0, false);
        directory(&mut data, 0, 2, 255, true);
        let image = decode(&data).expect("TIFF");
        assert_eq!(image.pixels(), [255; 8]);
    }

    #[test]
    fn packbits_and_lzw_streams() {
        let mut out = Vec::new();
        packbits(b"\x02abc\x80\xfdz\x00!", &mut out, 100);
        assert_eq!(out, b"abczzzz!");
        // Clear, 'a', 'b', the new code for "ab", end, as 9-bit codes.
        let mut out = Vec::new();
        lzw(&[0x80, 0x18, 0x4c, 0x50, 0x28, 0x08], &mut out, 100).expect("LZW");
        assert_eq!(out, b"abab");
        assert_eq!(
            lzw(&[0x00, 0x01], &mut Vec::new(), 100),
            Err(DecodeError::UnsupportedFeature)
        );
    }

    #[test]
    fn codes_are_prefix_free() {
        for codes in [&WHITE[..], &BLACK[..]] {
            let all = codes.iter().chain(&EXTENDED).collect::<Vec<_>>();
            for (i, a) in all.iter().enumerate() {
                for b in &all[i + 1..] {
                    let length = a.1.min(b.1);
                    assert_ne!(
                        a.0 >> (a.1 - length),
                        b.0 >> (b.1 - length),
                        "{a:?} and {b:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn huffman_rows_are_byte_aligned() {
        // White 2, black 3, white 3 in an 8 pixel row, then black 8 after alignment.
        // 0111 10 1000 | 000101 0 (black 8 has to follow white 0: 00110101).
        let fax = Fax::new(Coding::Huffman);
        let mut out = Vec::new();
        fax.decode(
            &[0b0111_1010, 0b0000_0000, 0b0011_0101, 0b0001_0100],
            8,
            2,
            &mut out,
        )
        .expect("Modified Huffman rows");
        assert_eq!(out, [0b0011_1000, 0b1111_1111]);
    }
}
