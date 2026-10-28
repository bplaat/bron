/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use alloc::vec;
use alloc::vec::Vec;
use core::mem;
use core::time::Duration;

use super::{
    Area, Bitmap, Budget, ColorSpace, DecodeError, EncodeError, EncodingStyle, Format, Frame,
    Image, LoopCount, Reader, Result, pixel_len, round,
};

// MARK: Decoder
#[derive(Clone, Copy, Default, Eq, PartialEq)]
enum DisposalMethod {
    #[default]
    None,
    Keep,
    Background,
    Previous,
}

impl TryFrom<u8> for DisposalMethod {
    type Error = DecodeError;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::Keep),
            2 => Ok(Self::Background),
            3 => Ok(Self::Previous),
            _ => Err(DecodeError::InvalidData),
        }
    }
}

#[derive(Default)]
struct GraphicControl {
    delay: Duration,
    transparent_index: Option<u8>,
    disposal: DisposalMethod,
}

struct Decoder<'a> {
    reader: Reader<'a>,
    width: u32,
    height: u32,
    pixel_len: usize,
    global_palette: [[u8; 4]; 256],
    global_palette_len: usize,
    budget: Budget,
    compressed: Vec<u8>,
    canvas: Vec<u8>,
    frames: Vec<Frame>,
    has_loop_extension: bool,
    loop_count: LoopCount,
    control: GraphicControl,
}

pub(super) fn decode(data: &[u8]) -> Result<Image> {
    Decoder::new(data)?.decode()
}

impl<'a> Decoder<'a> {
    fn new(data: &'a [u8]) -> Result<Self> {
        // Extension blocks configure the next frame. Image descriptors decompress palette indexes,
        // composite a displayed frame, and then apply its disposal operation.
        let mut reader = Reader::new(&data[6..]);
        let width = u32::from(reader.le16()?);
        let height = u32::from(reader.le16()?);
        let pixel_len = pixel_len(width, height)?;
        let packed = reader.byte()?;
        reader.byte()?; // Background index is not representable on a transparent RGBA canvas.
        reader.byte()?; // Pixel aspect ratio does not change the pixel canvas.
        let mut global_palette = [[0; 4]; 256];
        let global_palette_len = Self::read_table(&mut reader, packed, &mut global_palette)?;
        Ok(Self {
            reader,
            width,
            height,
            pixel_len,
            global_palette,
            global_palette_len,
            budget: Budget::default(),
            compressed: Vec::new(),
            canvas: Vec::new(),
            frames: Vec::new(),
            has_loop_extension: false,
            loop_count: LoopCount::Finite(1),
            control: GraphicControl::default(),
        })
    }

    fn decode(mut self) -> Result<Image> {
        loop {
            match self.reader.byte()? {
                0x21 => self.read_extension()?,
                0x2c => self.read_frame()?,
                0x3b => {
                    if self.frames.is_empty() {
                        return Err(DecodeError::InvalidData);
                    }
                    return Ok(Image {
                        format: Format::Gif,
                        color_space: ColorSpace::Srgb,
                        is_animated: self.has_loop_extension || self.frames.len() > 1,
                        frames: self.frames,
                        text: Vec::new(),
                        loop_count: self.loop_count,
                    });
                }
                _ => return Err(DecodeError::InvalidData),
            }
        }
    }

    fn read_extension(&mut self) -> Result<()> {
        match self.reader.byte()? {
            0xf9 => {
                if self.reader.byte()? != 4 {
                    return Err(DecodeError::InvalidData);
                }
                let flags = self.reader.byte()?;
                if flags & 0xe0 != 0 {
                    return Err(DecodeError::InvalidData);
                }
                self.control.disposal = DisposalMethod::try_from((flags >> 2) & 7)?;
                self.control.delay = Duration::from_millis(u64::from(self.reader.le16()?) * 10);
                let index = self.reader.byte()?;
                self.control.transparent_index = if flags & 1 != 0 { Some(index) } else { None };
                if self.reader.byte()? != 0 {
                    return Err(DecodeError::InvalidData);
                }
            }
            0xff => {
                let size = self.reader.byte()? as usize;
                if size != 11 {
                    return Err(DecodeError::InvalidData);
                }
                let name = self.reader.take(size)?;
                if matches!(name, b"NETSCAPE2.0" | b"ANIMEXTS1.0") {
                    self.compressed.clear();
                    Self::read_blocks(&mut self.reader, &mut self.budget, &mut self.compressed)?;
                    let bytes = &self.compressed;
                    if bytes.len() != 3 || bytes[0] != 1 {
                        return Err(DecodeError::InvalidData);
                    }
                    let repeats = u16::from_le_bytes([bytes[1], bytes[2]]);
                    self.has_loop_extension = true;
                    self.loop_count = if repeats == 0 {
                        LoopCount::Infinite
                    } else {
                        LoopCount::Finite(u32::from(repeats) + 1)
                    };
                } else {
                    Self::skip_blocks(&mut self.reader)?;
                }
            }
            0x01 => return Err(DecodeError::UnsupportedFeature), // Plain-text rendering.
            _ => Self::skip_blocks(&mut self.reader)?,
        }
        Ok(())
    }

    fn read_frame(&mut self) -> Result<()> {
        let control = mem::take(&mut self.control);
        let x = self.reader.le16()? as usize;
        let y = self.reader.le16()? as usize;
        let w = self.reader.le16()? as usize;
        let h = self.reader.le16()? as usize;
        let area = Area {
            x,
            y,
            width: w,
            height: h,
        }
        .validate(self.width, self.height)?;
        let flags = self.reader.byte()?;
        if flags & 0x18 != 0 {
            return Err(DecodeError::InvalidData);
        }
        let mut local = [[0; 4]; 256];
        let local_len = Self::read_table(&mut self.reader, flags, &mut local)?;
        // Keep the full 256-entry table so validated indexes need no per-pixel bounds checks.
        let (palette, palette_len) = if local_len != 0 {
            (&local, local_len)
        } else {
            (&self.global_palette, self.global_palette_len)
        };
        if palette_len == 0
            || control
                .transparent_index
                .is_some_and(|t| t as usize >= palette_len)
        {
            return Err(DecodeError::InvalidData);
        }
        let bg = [0; 4];
        let min_code = self.reader.byte()?;
        self.compressed.clear();
        Self::read_blocks(&mut self.reader, &mut self.budget, &mut self.compressed)?;
        let indices = lzw(&self.compressed, min_code, w * h, &mut self.budget)?;
        if indices.iter().copied().max().unwrap_or(0) as usize >= palette_len {
            return Err(DecodeError::InvalidData);
        }
        if self.canvas.is_empty() {
            self.canvas = self.budget.zeroed(self.pixel_len)?;
        }
        // Once the trailer follows, the canvas can become the output frame without a copy.
        let last_frame = self.reader.data.get(self.reader.pos) == Some(&0x3b);
        let previous = if !last_frame && control.disposal == DisposalMethod::Previous {
            Some(area.snapshot(&self.canvas, self.width as usize, &mut self.budget)?)
        } else {
            None
        };
        let passes = if flags & 0x40 != 0 {
            &[(0, 8), (4, 8), (2, 4), (1, 2)][..]
        } else {
            &[(0, 1)][..]
        };
        let mut source_row = 0;
        for &(start, step) in passes {
            for row in (start..h).step_by(step) {
                let dest = ((y + row) * self.width as usize + x) * 4;
                let dest = self.canvas[dest..dest + w * 4].as_chunks_mut::<4>().0;
                let indices = &indices[source_row * w..(source_row + 1) * w];
                if let Some(transparent) = control.transparent_index {
                    for (p, &index) in dest.iter_mut().zip(indices) {
                        if index != transparent {
                            *p = palette[index as usize];
                        }
                    }
                } else {
                    for (p, &index) in dest.iter_mut().zip(indices) {
                        *p = palette[index as usize];
                    }
                }
                source_row += 1;
            }
        }
        let pixels = if last_frame {
            mem::take(&mut self.canvas)
        } else {
            self.budget.copy(&self.canvas)?
        };
        self.budget.frame(
            &mut self.frames,
            self.width,
            self.height,
            pixels,
            control.delay,
        )?;
        if last_frame {
            return Ok(());
        }
        match control.disposal {
            DisposalMethod::Background => {
                area.clear(&mut self.canvas, self.width as usize, bg);
            }
            DisposalMethod::Previous => {
                area.restore(
                    &mut self.canvas,
                    self.width as usize,
                    &previous.expect("previous canvas was saved"),
                );
            }
            _ => {}
        }
        Ok(())
    }

    fn read_table(
        reader: &mut Reader<'_>,
        flags: u8,
        palette: &mut [[u8; 4]; 256],
    ) -> Result<usize> {
        if flags & 0x80 == 0 {
            return Ok(0);
        }
        let count = 2usize << (flags & 7);
        for color in &mut palette[..count] {
            color[..3].copy_from_slice(reader.take(3)?);
            color[3] = 255;
        }
        Ok(count)
    }

    fn read_blocks(reader: &mut Reader<'_>, budget: &mut Budget, out: &mut Vec<u8>) -> Result<()> {
        loop {
            let len = reader.byte()? as usize;
            if len == 0 {
                return Ok(());
            }
            budget.append(out, reader.take(len)?)?;
        }
    }

    fn skip_blocks(reader: &mut Reader<'_>) -> Result<()> {
        loop {
            let len = reader.byte()? as usize;
            if len == 0 {
                return Ok(());
            }
            reader.take(len)?;
        }
    }
}

fn lzw(data: &[u8], min: u8, len: usize, budget: &mut Budget) -> Result<Vec<u8>> {
    // Codes are packed least-significant bit first and grow in width with the dictionary.
    if !(2..=8).contains(&min) {
        return Err(DecodeError::InvalidData);
    }
    let clear = 1usize << min;
    let end = clear + 1;
    let mut next = clear + 2;
    let mut size = min + 1;
    // Every dictionary entry is the previous string plus the next string's first byte, which
    // always sits contiguously in the output, so entries are stored as output ranges.
    let mut offsets = [0usize; 4096];
    let mut lengths = [0usize; 4096];
    let mut previous: Option<(usize, usize)> = None;
    let bit_len = data
        .len()
        .checked_mul(8)
        .ok_or(DecodeError::ImageTooLarge)?;
    // A code can expand to at most one byte per preceding code, capped by the
    // dictionary size. Reject impossible dimensions before allocating their output.
    let code_count = bit_len / usize::from(min + 1);
    let max_output = code_count.saturating_mul(code_count.min(offsets.len()));
    if len > max_output {
        return Err(DecodeError::InvalidData);
    }
    let mut output = budget.zeroed(len)?;
    let mut written = 0;
    let mut acc = 0u64;
    let mut acc_bits = 0u32;
    let mut pos = 0;
    loop {
        while acc_bits <= 56 && pos < data.len() {
            acc |= u64::from(data[pos]) << acc_bits;
            acc_bits += 8;
            pos += 1;
        }
        if acc_bits < u32::from(size) {
            return Err(DecodeError::InvalidData);
        }
        let code = (acc & ((1 << size) - 1)) as usize;
        acc >>= size;
        acc_bits -= u32::from(size);
        if code == clear {
            next = clear + 2;
            size = min + 1;
            previous = None;
            continue;
        }
        if code == end {
            return if written == len {
                Ok(output)
            } else {
                Err(DecodeError::InvalidData)
            };
        }
        let start = written;
        if code < clear {
            if written == len {
                return Err(DecodeError::InvalidData);
            }
            output[written] = code as u8;
            written += 1;
        } else if code < next {
            let (offset, count) = (offsets[code], lengths[code]);
            if count > len - written {
                return Err(DecodeError::InvalidData);
            }
            if count <= 16 && written + 16 <= len {
                // Copy short strings as one block; bytes past `count` are overwritten later.
                let block: [u8; 16] = output[offset..offset + 16].try_into().expect("16 bytes");
                output[written..written + 16].copy_from_slice(&block);
            } else {
                output.copy_within(offset..offset + count, written);
            }
            written += count;
        } else if code == next
            && let Some((offset, count)) = previous
        {
            // The code being defined: the previous string followed by its own first byte.
            if count >= len - written {
                return Err(DecodeError::InvalidData);
            }
            output.copy_within(offset..offset + count, written);
            output[written + count] = output[offset];
            written += count + 1;
        } else {
            return Err(DecodeError::InvalidData);
        }
        if let Some((offset, count)) = previous
            && next < 4096
        {
            offsets[next] = offset;
            lengths[next] = count + 1;
            next += 1;
            if next == 1 << size && size < 12 {
                size += 1;
            }
        }
        previous = Some((start, written - start));
    }
}

// MARK: Encoder
#[derive(Clone, Copy)]
struct GifFrame<'a> {
    bitmap: &'a Bitmap,
    delay: Duration,
}

#[derive(Clone, Copy)]
enum GifFrames<'a> {
    Still(&'a Bitmap),
    Animated(&'a [Frame]),
}

impl<'a> GifFrames<'a> {
    const fn len(self) -> usize {
        match self {
            Self::Still(_) => 1,
            Self::Animated(frames) => frames.len(),
        }
    }

    const fn get(self, index: usize) -> GifFrame<'a> {
        match self {
            Self::Still(bitmap) => GifFrame {
                bitmap,
                delay: Duration::ZERO,
            },
            Self::Animated(frames) => GifFrame {
                bitmap: &frames[index].bitmap,
                delay: frames[index].delay,
            },
        }
    }
}

pub(super) fn encode_bitmap(
    bitmap: &Bitmap,
    style: EncodingStyle,
) -> core::result::Result<Vec<u8>, EncodeError> {
    encode_frames(GifFrames::Still(bitmap), LoopCount::Finite(1), style)
}

pub(super) fn encode(
    frames: &[Frame],
    loop_count: LoopCount,
    style: EncodingStyle,
) -> core::result::Result<Vec<u8>, EncodeError> {
    encode_frames(GifFrames::Animated(frames), loop_count, style)
}

fn encode_frames(
    frames: GifFrames<'_>,
    loop_count: LoopCount,
    style: EncodingStyle,
) -> core::result::Result<Vec<u8>, EncodeError> {
    let first = frames.get(0).bitmap;
    let width = u16::try_from(first.width).map_err(|_| EncodeError::InvalidDimensions)?;
    let height = u16::try_from(first.height).map_err(|_| EncodeError::InvalidDimensions)?;
    let mut out = Vec::new();
    out.extend_from_slice(b"GIF89a");
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&[0x70, 0, 0]);
    let repeats = match loop_count {
        LoopCount::Infinite => Some(0),
        LoopCount::Finite(1) => None,
        LoopCount::Finite(plays) if (2..=65536).contains(&plays) => Some((plays - 1) as u16),
        _ => return Err(EncodeError::InvalidTiming),
    };
    if let Some(repeats) = repeats {
        out.extend_from_slice(b"\x21\xff\x0bNETSCAPE2.0\x03\x01");
        out.extend_from_slice(&repeats.to_le_bytes());
        out.push(0);
    }
    // An opaque frame only stores the area that changed, drawn over the kept previous frame.
    // Disposal only clears a frame's own area, so a frame before a transparent one stays full.
    let opaque: Vec<_> = (0..frames.len())
        .map(|index| {
            let pixels = frames.get(index).bitmap.data.as_chunks::<4>().0;
            index == 0 || pixels.iter().all(|pixel| pixel[3] >= 128)
        })
        .collect();
    let cropped: Vec<_> = (0..frames.len())
        .map(|index| index != 0 && opaque[index] && opaque.get(index + 1) != Some(&false))
        .collect();
    for index in 0..frames.len() {
        let frame = frames.get(index);
        let delay = u16::try_from(frame.delay.as_nanos().div_ceil(10_000_000))
            .map_err(|_| EncodeError::InvalidTiming)?;
        let region = if cropped[index] {
            Some(super::encoder::crop_changed(
                frames.get(index - 1).bitmap,
                frame.bitmap,
            )?)
        } else {
            None
        };
        let (x, y, bitmap) = region
            .as_ref()
            .map_or((0, 0, frame.bitmap), |(x, y, bitmap)| (*x, *y, bitmap));
        let (palette, candidates, transparency) = gif_palette(bitmap, style);
        let bits = (palette.len().max(2).next_power_of_two().trailing_zeros() as u8).max(1);
        let min_code_size = bits.max(2);
        let disposal = if cropped.get(index + 1) == Some(&true) {
            1
        } else {
            2
        };
        out.extend_from_slice(&[
            0x21,
            0xf9,
            4,
            disposal << 2 | u8::from(transparency.is_some()),
        ]);
        out.extend_from_slice(&delay.to_le_bytes());
        out.push(transparency.unwrap_or(0));
        out.push(0);
        out.push(0x2c);
        for value in [x, y, bitmap.width, bitmap.height] {
            out.extend_from_slice(&(value as u16).to_le_bytes());
        }
        out.push(0x80 | (bits - 1));
        for color in &palette {
            out.extend_from_slice(color);
        }
        for _ in palette.len()..(1usize << bits) {
            out.extend_from_slice(&[0, 0, 0]);
        }
        out.push(min_code_size);
        let data = candidates
            .iter()
            .map(|indices| gif_lzw(indices, min_code_size))
            .min_by_key(Vec::len)
            .expect("GIF has indices");
        for block in data.chunks(255) {
            out.push(block.len() as u8);
            out.extend_from_slice(block);
        }
        out.push(0);
    }
    out.push(0x3b);
    Ok(out)
}

fn gif_palette(bitmap: &Bitmap, style: EncodingStyle) -> (Vec<[u8; 3]>, Vec<Vec<u8>>, Option<u8>) {
    let pixels = &bitmap.data;
    let transparent = pixels.as_chunks::<4>().0.iter().any(|pixel| pixel[3] < 128);
    let mut colors = Vec::<[u8; 3]>::with_capacity(256);
    let mut lookup = [0u64; 512];
    let mut indices = Vec::with_capacity(pixels.len() / 4);
    if transparent {
        colors.push([0, 0, 0]);
    }
    'pixels: for pixel in pixels.as_chunks::<4>().0 {
        let index = if pixel[3] < 128 {
            0
        } else {
            let key = u32::from_be_bytes([pixel[0], pixel[1], pixel[2], 255]);
            let mut slot = (key.wrapping_mul(0x9e37_79b1) as usize) & (lookup.len() - 1);
            loop {
                let entry = lookup[slot];
                if entry == 0 {
                    if colors.len() == 256 {
                        colors.clear();
                        break 'pixels;
                    }
                    let index = colors.len() as u8;
                    colors.push([pixel[0], pixel[1], pixel[2]]);
                    lookup[slot] = (u64::from(key) << 9) | (u64::from(index) + 1);
                    break index;
                }
                if entry >> 9 == u64::from(key) {
                    break (entry - 1) as u8;
                }
                slot = (slot + 1) & (lookup.len() - 1);
            }
        };
        if colors.is_empty() {
            break;
        }
        indices.push(index);
    }
    if colors.is_empty() {
        let (colors, candidates) = gif_quantize(bitmap, transparent, style);
        return (colors, candidates, transparent.then_some(0));
    }
    (colors, vec![indices], transparent.then_some(0))
}

// One histogram cell: the pixel count and mean color of a 5-bit-per-channel cube.
#[derive(Clone, Copy)]
struct GifCell {
    count: f64,
    color: [f64; 3],
}

fn gif_distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3)
        .map(|channel| {
            let delta = a[channel] - b[channel];
            delta * delta
        })
        .sum()
}

fn gif_nearest(palette: &[[f64; 3]], color: [f64; 3]) -> usize {
    (0..palette.len())
        .min_by(|&a, &b| {
            gif_distance(palette[a], color).total_cmp(&gif_distance(palette[b], color))
        })
        .expect("palette has colors")
}

// Sum of squared distances from the cells to their weighted mean.
fn gif_error(cells: &[GifCell]) -> f64 {
    let (mut count, mut sum, mut squares) = (0.0, [0.0; 3], 0.0);
    for cell in cells {
        count += cell.count;
        for (sum, value) in sum.iter_mut().zip(cell.color) {
            *sum += cell.count * value;
            squares += cell.count * value * value;
        }
    }
    squares - sum.iter().map(|sum| sum * sum).sum::<f64>() / count
}

fn gif_mean(cells: &[GifCell]) -> [f64; 3] {
    let count: f64 = cells.iter().map(|cell| cell.count).sum();
    core::array::from_fn(|channel| {
        cells
            .iter()
            .map(|cell| cell.count * cell.color[channel])
            .sum::<f64>()
            / count
    })
}

// Reduces the colors with variance-based median cut over a histogram, refines the palette
// with k-means, and maps pixels with serpentine Floyd-Steinberg error diffusion. Max
// compression also tries exact nearest colors and lighter diffusion.
fn gif_quantize(
    bitmap: &Bitmap,
    transparent: bool,
    style: EncodingStyle,
) -> (Vec<[u8; 3]>, Vec<Vec<u8>>) {
    let pixels = bitmap.data.as_chunks::<4>().0;
    let mut histogram = vec![(0u32, [0u64; 3]); 1 << 15];
    for pixel in pixels.iter().filter(|pixel| pixel[3] >= 128) {
        let cell = &mut histogram[usize::from(pixel[0] >> 3) << 10
            | usize::from(pixel[1] >> 3) << 5
            | usize::from(pixel[2] >> 3)];
        cell.0 += 1;
        for (sum, &value) in cell.1.iter_mut().zip(pixel) {
            *sum += u64::from(value);
        }
    }
    let mut cells: Vec<_> = histogram
        .iter()
        .filter(|cell| cell.0 != 0)
        .map(|&(count, sum)| GifCell {
            count: f64::from(count),
            color: sum.map(|sum| sum as f64 / f64::from(count)),
        })
        .collect();
    let reserved = usize::from(transparent);
    let target = 256 - reserved;

    // Split the box with the largest error at the point that minimizes the two halves' error.
    let mut boxes = vec![(0, cells.len(), gif_error(&cells))];
    while boxes.len() < target {
        let Some((index, _)) = boxes
            .iter()
            .enumerate()
            .filter(|(_, (start, end, _))| end - start > 1)
            .max_by(|a, b| a.1.2.total_cmp(&b.1.2))
        else {
            break;
        };
        let (start, end, _) = boxes.swap_remove(index);
        let slice = &mut cells[start..end];
        let mean = gif_mean(slice);
        let channel = (0..3)
            .max_by(|&a, &b| {
                let spread = |channel: usize| {
                    slice
                        .iter()
                        .map(|cell| {
                            let delta = cell.color[channel] - mean[channel];
                            cell.count * delta * delta
                        })
                        .sum::<f64>()
                };
                spread(a).total_cmp(&spread(b))
            })
            .expect("colors have channels");
        slice.sort_by(|a, b| a.color[channel].total_cmp(&b.color[channel]));
        let (mut count, mut sum, mut squares) = (0.0, 0.0, 0.0);
        let total: (f64, f64, f64) = slice.iter().fold((0.0, 0.0, 0.0), |acc, cell| {
            let value = cell.color[channel];
            (
                acc.0 + cell.count,
                acc.1 + cell.count * value,
                acc.2 + cell.count * value * value,
            )
        });
        let mut split = (f64::MAX, 1);
        for (offset, cell) in slice[..slice.len() - 1].iter().enumerate() {
            let value = cell.color[channel];
            count += cell.count;
            sum += cell.count * value;
            squares += cell.count * value * value;
            let rest = (total.0 - count, total.1 - sum, total.2 - squares);
            let error = squares - sum * sum / count + rest.2 - rest.1 * rest.1 / rest.0;
            if error < split.0 {
                split = (error, offset + 1);
            }
        }
        let middle = start + split.1;
        boxes.push((start, middle, gif_error(&cells[start..middle])));
        boxes.push((middle, end, gif_error(&cells[middle..end])));
    }
    let mut palette: Vec<_> = boxes
        .iter()
        .map(|&(start, end, _)| gif_mean(&cells[start..end]))
        .collect();

    // Move each palette color to the mean of the cells nearest to it.
    for _ in 0..4 {
        let mut sums = vec![(0.0, [0.0; 3]); palette.len()];
        for cell in &cells {
            let sum = &mut sums[gif_nearest(&palette, cell.color)];
            sum.0 += cell.count;
            for (sum, value) in sum.1.iter_mut().zip(cell.color) {
                *sum += cell.count * value;
            }
        }
        for (color, (count, sum)) in palette.iter_mut().zip(sums) {
            if count > 0.0 {
                *color = sum.map(|sum| sum / count);
            }
        }
    }
    let mut colors: Vec<[u8; 3]> = vec![[0; 3]; reserved];
    colors.extend(
        palette
            .iter()
            .map(|color| color.map(|value| round(value) as u8)),
    );
    let palette: Vec<_> = colors.iter().map(|color| color.map(f64::from)).collect();
    let opaque = &palette[reserved..];
    let width = bitmap.width as usize;

    // Nearest colors are cached per 6-bit cell center; diffusion corrects the difference.
    let mut cache = vec![0u16; 1 << 18];
    let mut candidates = vec![gif_diffuse(pixels, width, &palette, 0.25, |color| {
        let cell = color.map(|value| value as usize >> 2);
        let key = cell[0] << 12 | cell[1] << 6 | cell[2];
        if cache[key] == 0 {
            let center = cell.map(|value| (value << 2 | 2) as f64);
            cache[key] = (reserved + gif_nearest(opaque, center)) as u16 + 1;
        }
        usize::from(cache[key] - 1)
    })];
    if style == EncodingStyle::MaxCompression {
        candidates.push(gif_diffuse(pixels, width, &palette, 0.125, |color| {
            reserved + gif_nearest(opaque, color.map(f64::from))
        }));
    }
    (colors, candidates)
}

fn gif_diffuse(
    pixels: &[[u8; 4]],
    width: usize,
    palette: &[[f64; 3]],
    strength: f32,
    mut lookup: impl FnMut([f32; 3]) -> usize,
) -> Vec<u8> {
    let mut indices = vec![0; pixels.len()];
    let mut current = vec![[0f32; 3]; width + 2];
    let mut next = vec![[0f32; 3]; width + 2];
    for (y, row) in pixels.chunks_exact(width).enumerate() {
        let reverse = y % 2 == 1;
        for step in 0..width {
            let x = if reverse { width - 1 - step } else { step };
            let pixel = row[x];
            if pixel[3] < 128 {
                continue;
            }
            let wanted: [f32; 3] = core::array::from_fn(|channel| {
                (f32::from(pixel[channel]) + current[x + 1][channel]).clamp(0.0, 255.0)
            });
            let index = lookup(wanted);
            indices[y * width + x] = index as u8;
            let error: [f32; 3] = core::array::from_fn(|channel| {
                (wanted[channel] - palette[index][channel] as f32).clamp(-64.0, 64.0) * strength
            });
            let (ahead, behind) = if reverse { (x, x + 2) } else { (x + 2, x) };
            for channel in 0..3 {
                current[ahead][channel] += error[channel] * 7.0 / 16.0;
                next[behind][channel] += error[channel] * 3.0 / 16.0;
                next[x + 1][channel] += error[channel] * 5.0 / 16.0;
                next[ahead][channel] += error[channel] / 16.0;
            }
        }
        mem::swap(&mut current, &mut next);
        next.fill([0.0; 3]);
    }
    indices
}

fn gif_lzw(indices: &[u8], min_code_size: u8) -> Vec<u8> {
    let clear = 1u16 << min_code_size;
    let end = clear + 1;
    let mut next = end + 1;
    let mut width = min_code_size + 1;
    let mut dictionary = [0u32; 8192];
    let mut out = Vec::new();
    let mut bits = 0u32;
    let mut bit_count = 0u8;
    let mut write = |code: u16, width: u8, out: &mut Vec<u8>| {
        bits |= u32::from(code) << bit_count;
        bit_count += width;
        while bit_count >= 8 {
            out.push(bits as u8);
            bits >>= 8;
            bit_count -= 8;
        }
    };
    write(clear, width, &mut out);
    if let Some((&first, rest)) = indices.split_first() {
        let mut prefix = u16::from(first);
        for &index in rest {
            let key = (u32::from(prefix) << 8) | u32::from(index);
            let mut slot = (key.wrapping_mul(0x9e37_79b1) as usize) & (dictionary.len() - 1);
            while dictionary[slot] != 0 && dictionary[slot] >> 12 != key {
                slot = (slot + 1) & (dictionary.len() - 1);
            }
            if dictionary[slot] != 0 {
                prefix = (dictionary[slot] & 0xfff) as u16;
            } else {
                write(prefix, width, &mut out);
                if next < 4096 {
                    dictionary[slot] = (key << 12) | u32::from(next);
                    next += 1;
                    if next > (1 << width) && width < 12 {
                        width += 1;
                    }
                } else {
                    write(clear, width, &mut out);
                    dictionary.fill(0);
                    next = end + 1;
                    width = min_code_size + 1;
                }
                prefix = u16::from(index);
            }
        }
        write(prefix, width, &mut out);
        if next == 1 << width && width < 12 {
            width += 1;
        }
    }
    write(end, width, &mut out);
    if bit_count != 0 {
        out.push(bits as u8);
    }
    out
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Bitmap, EncodeOptions, EncodingStyle, Format, encode, encode_animation_with_options,
    };

    #[test]
    fn disposal_method_values_are_validated() {
        assert!(matches!(
            DisposalMethod::try_from(3),
            Ok(DisposalMethod::Previous)
        ));
        assert!(DisposalMethod::try_from(4).is_err());
    }

    // A reference GIF LZW encoder that resets the dictionary once it is full.
    fn lzw_encode(data: &[u8], min: u8) -> Vec<u8> {
        let clear = 1u32 << min;
        let (mut out, mut acc, mut acc_bits) = (Vec::new(), 0u64, 0u32);
        let mut emit = |code: u32, size: u32| {
            acc |= u64::from(code) << acc_bits;
            acc_bits += size;
            while acc_bits >= 8 {
                out.push(acc as u8);
                acc >>= 8;
                acc_bits -= 8;
            }
        };
        let mut dictionary = std::collections::HashMap::new();
        let mut next = clear + 2;
        let mut size = u32::from(min) + 1;
        emit(clear, size);
        let mut prefix = u32::from(data[0]);
        for &byte in &data[1..] {
            if let Some(&code) = dictionary.get(&(prefix, byte)) {
                prefix = code;
                continue;
            }
            emit(prefix, size);
            if next < 4096 {
                dictionary.insert((prefix, byte), next);
                next += 1;
                if next > 1 << size && size < 12 {
                    size += 1;
                }
            } else {
                emit(clear, size);
                dictionary.clear();
                next = clear + 2;
                size = u32::from(min) + 1;
            }
            prefix = u32::from(byte);
        }
        emit(prefix, size);
        emit(clear + 1, size);
        if acc_bits > 0 {
            out.push(acc as u8);
        }
        out
    }

    #[test]
    fn lzw_round_trips_growth_resets_and_long_strings() {
        let mut state = 7u32;
        let mut data = Vec::new();
        for _ in 0..4000 {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            // Short noisy runs grow the code width; long runs build long dictionary strings.
            let run = if state.is_multiple_of(16) {
                300
            } else {
                state as usize % 8 + 1
            };
            data.extend(core::iter::repeat_n((state >> 8) as u8 & 3, run));
        }
        for min in [2, 8] {
            let encoded = lzw_encode(&data, min);
            assert_eq!(
                lzw(&encoded, min, data.len(), &mut Budget::default()).as_deref(),
                Ok(&data[..])
            );
            assert!(lzw(&encoded, min, data.len() - 1, &mut Budget::default()).is_err());
        }
    }

    #[test]
    fn image_rs_interlaced_fixture_decodes() {
        let image = decode(include_bytes!("../tests/images/gif/anim/interlaced.gif"))
            .expect("interlaced GIF");
        assert_eq!((image.width(), image.height()), (32, 32));
        assert_eq!(image.frames().len(), 1);
    }
    fn gif_frame(out: &mut Vec<u8>, x: u16, index: u8, disposal: u8, transparent: bool) {
        out.extend_from_slice(&[
            0x21,
            0xf9,
            4,
            (disposal << 2) | u8::from(transparent),
            1,
            0,
            0,
            0,
            0x2c,
        ]);
        out.extend_from_slice(&x.to_le_bytes());
        out.extend_from_slice(&[0, 0, 1, 0, 1, 0, 0, 2]);
        // One-pixel stream: clear (4), palette index, end (5), all 3-bit codes.
        let codes = 4u16 | (u16::from(index) << 3) | (5 << 6);
        out.extend_from_slice(&[2, codes as u8, (codes >> 8) as u8, 0]);
    }

    #[test]
    fn gif_animation_metadata_distinguishes_single_frame_playback() {
        let mut single_frame = b"GIF89a\x01\0\x01\0\x80\0\0".to_vec();
        single_frame.extend_from_slice(&[255, 0, 0, 0, 0, 0]);
        gif_frame(&mut single_frame, 0, 0, 0, false);
        single_frame.push(0x3b);

        let image = decode(&single_frame).expect("single-frame GIF");
        assert!(!image.is_animated());
        assert_eq!(image.loop_count(), LoopCount::Finite(1));

        let mut final_previous = single_frame[..19].to_vec();
        gif_frame(&mut final_previous, 0, 0, 3, false);
        final_previous.push(0x3b);
        let image = decode(&final_previous).expect("final frame with previous disposal");
        assert_eq!(image.frames()[0].pixels(), &[255, 0, 0, 255]);

        for (repeats, expected) in [(0u16, LoopCount::Infinite), (2, LoopCount::Finite(3))] {
            let mut animated = single_frame[..19].to_vec();
            animated.extend_from_slice(b"\x21\xff\x0bNETSCAPE2.0\x03\x01");
            animated.extend_from_slice(&repeats.to_le_bytes());
            animated.push(0);
            animated.extend_from_slice(&single_frame[19..]);
            let image = decode(&animated).expect("looping single-frame GIF");
            assert!(image.is_animated());
            assert_eq!(image.loop_count(), expected);
        }
    }

    #[test]
    fn gif_transparency_restore_background_and_previous() {
        for disposal in [2, 3] {
            let mut data = b"GIF89a\x02\0\x01\0\x81\0\0".to_vec();
            data.extend_from_slice(&[0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255]);
            gif_frame(&mut data, 0, 1, 0, true);
            gif_frame(&mut data, 0, 2, disposal, true);
            gif_frame(&mut data, 1, 3, 0, true);
            data.push(0x3b);
            let image = decode(&data).expect("GIF disposal");
            assert!(image.is_animated());
            assert_eq!(image.loop_count(), LoopCount::Finite(1));
            assert_eq!(image.frames()[0].pixels(), &[255, 0, 0, 255, 0, 0, 0, 0]);
            assert_eq!(image.frames()[1].pixels(), &[0, 255, 0, 255, 0, 0, 0, 0]);
            let left = if disposal == 2 {
                [0, 0, 0, 0]
            } else {
                [255, 0, 0, 255]
            };
            assert_eq!(
                image.frames()[2].pixels(),
                [left, [0, 0, 255, 255]].concat()
            );
        }
    }

    #[test]
    fn partial_first_frame_leaves_uncovered_canvas_transparent() {
        let mut data = b"GIF89a\x02\0\x01\0\x80\x01\0".to_vec();
        data.extend_from_slice(&[255, 0, 0, 0, 0, 255]);
        gif_frame(&mut data, 0, 0, 2, false);
        gif_frame(&mut data, 1, 1, 0, false);
        data.push(0x3b);
        let image = decode(&data).expect("partial GIF frame");
        assert_eq!(image.frames()[0].pixels(), &[255, 0, 0, 255, 0, 0, 0, 0]);
        assert_eq!(image.frames()[1].pixels(), &[0, 0, 0, 0, 0, 0, 255, 255]);
    }

    #[cfg(feature = "gif")]
    #[test]
    fn gif_single_frame_transparency() {
        let bitmap = Bitmap::new(2, 1, vec![255, 0, 0, 255, 0, 0, 0, 0]).unwrap();
        let decoded = crate::decode(&encode(&bitmap, Format::Gif).unwrap()).unwrap();
        assert_eq!(decoded.pixels()[..4], [255, 0, 0, 255]);
        assert_eq!(decoded.pixels()[7], 0);
    }

    #[test]
    fn gif_quantized_gradient_preserves_transparency_and_color() {
        let mut pixels = Vec::new();
        for y in 0..64u8 {
            for x in 0..64u8 {
                pixels.extend_from_slice(&[
                    x * 4,
                    y * 4,
                    x.wrapping_mul(3).wrapping_add(y.wrapping_mul(5)),
                    if x < 8 { 0 } else { 255 },
                ]);
            }
        }
        let bitmap = Bitmap::new(64, 64, pixels).unwrap();
        for style in [
            EncodingStyle::NormalCompression,
            EncodingStyle::MaxCompression,
        ] {
            let encoded = crate::encode_with_options(
                &bitmap,
                Format::Gif,
                EncodeOptions {
                    style,
                    ..EncodeOptions::default()
                },
            )
            .unwrap();
            let decoded = crate::decode(&encoded).unwrap();
            let mut squared_error = 0u64;
            let mut count = 0u64;
            for (source, result) in bitmap
                .data()
                .as_chunks::<4>()
                .0
                .iter()
                .zip(decoded.pixels().as_chunks::<4>().0)
            {
                assert_eq!(result[3], source[3]);
                if source[3] == 255 {
                    for channel in 0..3 {
                        let error = i32::from(source[channel]) - i32::from(result[channel]);
                        squared_error += error.unsigned_abs().pow(2) as u64;
                        count += 1;
                    }
                }
            }
            assert!(squared_error / count < 150, "{style:?}");
        }
    }

    #[cfg(feature = "gif")]
    #[test]
    fn gif_styles_encode_the_same_animation() {
        let first = [255, 0, 0, 255].repeat(64);
        let mut second = first.clone();
        second[(3 * 8 + 5) * 4..(3 * 8 + 5) * 4 + 4].copy_from_slice(&[0, 255, 0, 255]);
        let mut changed = second.clone();
        changed[4..8].copy_from_slice(&[0, 0, 255, 255]);
        let mut transparent = changed.clone();
        transparent[..4].fill(0);
        let frames = [
            first,
            second,
            changed,
            transparent.clone(),
            transparent,
            [0, 0, 255, 255].repeat(64),
        ]
        .map(|pixels| {
            Frame::new(
                Bitmap::new(8, 8, pixels).unwrap(),
                Duration::from_millis(50),
            )
        });
        let normal = crate::encode_animation(Format::Gif, &frames, LoopCount::Infinite).unwrap();
        let max = encode_animation_with_options(
            Format::Gif,
            &frames,
            LoopCount::Infinite,
            EncodeOptions {
                style: EncodingStyle::MaxCompression,
                ..EncodeOptions::default()
            },
        )
        .unwrap();
        assert_eq!(normal, max);
        // The second frame only stores its changed pixel at (5, 3).
        assert!(
            normal
                .windows(9)
                .any(|bytes| bytes == [0x2c, 5, 0, 3, 0, 1, 0, 1, 0])
        );
        let decoded = crate::decode(&max).unwrap();
        assert_eq!(decoded.frames().len(), frames.len());
        for (actual, expected) in decoded.frames().iter().zip(&frames) {
            assert_eq!(actual.pixels(), expected.pixels());
        }
    }

    #[test]
    fn gif_delay_rounds_up_from_nanoseconds() {
        let bitmap = Bitmap::new(1, 1, vec![255, 0, 0, 255]).unwrap();
        for (delay, expected) in [
            (Duration::from_nanos(1), Duration::from_millis(10)),
            (Duration::from_micros(10_500), Duration::from_millis(20)),
        ] {
            let frame = Frame::new(Bitmap::new(1, 1, bitmap.data().to_vec()).unwrap(), delay);
            let encoded =
                crate::encode_animation(Format::Gif, &[frame], LoopCount::Infinite).unwrap();
            assert_eq!(
                crate::decode(&encoded).unwrap().frames()[0].delay(),
                expected
            );
        }
    }

    #[test]
    fn gif_lzw_round_trips_across_code_widths() {
        for (width, height) in [(1u32, 1u32), (8, 8), (128, 128)] {
            let mut pixels = Vec::new();
            for i in 0..width * height {
                let color = (i.wrapping_mul(73) ^ (i / 11)) as u8;
                pixels.extend_from_slice(&[
                    color,
                    color.wrapping_mul(3),
                    color.wrapping_mul(5),
                    255,
                ]);
            }
            let bitmap = Bitmap::new(width, height, pixels).unwrap();
            let encoded = encode(&bitmap, Format::Gif).unwrap();
            assert_eq!(crate::decode(&encoded).unwrap().pixels(), bitmap.data());
        }
    }

    #[test]
    fn gif_quantized_palette_decodes_with_transparency() {
        let mut pixels = Vec::new();
        for y in 0..32u8 {
            for x in 0..32u8 {
                pixels.extend_from_slice(&[x * 7, y * 7, x ^ y, 255]);
            }
        }
        pixels[3] = 0;
        let bitmap = Bitmap::new(32, 32, pixels).unwrap();
        let encoded = encode(&bitmap, Format::Gif).unwrap();
        let decoded = crate::decode(&encoded).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (32, 32));
        assert_eq!(decoded.pixels()[3], 0);
    }
}
