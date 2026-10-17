/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use alloc::vec;
use alloc::vec::Vec;
use core::time::Duration;

use crc::{CRC_32_ISO_HDLC, Crc};

use super::{
    Area, Bitmap, Budget, ColorSpace, DecodeError, EncodeError, EncodingStyle, Format, Frame,
    Image, LoopCount, Reader, Result, pixel_len,
};

// MARK: Decoder
struct Header {
    width: u32,
    height: u32,
    depth: u8,
    color: ColorType,
    channels: usize,
    interlace: InterlaceMethod,
}

#[derive(Clone, Copy)]
struct Control {
    area: Area,
    delay: Duration,
    dispose: DisposeOp,
    blend: BlendOp,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ColorType {
    Grayscale,
    Truecolor,
    Indexed,
    GrayscaleAlpha,
    TruecolorAlpha,
}

impl ColorType {
    const fn channels(self) -> usize {
        match self {
            Self::Grayscale | Self::Indexed => 1,
            Self::GrayscaleAlpha => 2,
            Self::Truecolor => 3,
            Self::TruecolorAlpha => 4,
        }
    }

    const fn accepts_depth(self, depth: u8) -> bool {
        match self {
            Self::Grayscale => matches!(depth, 1 | 2 | 4 | 8 | 16),
            Self::Indexed => matches!(depth, 1 | 2 | 4 | 8),
            _ => matches!(depth, 8 | 16),
        }
    }
}

impl TryFrom<u8> for ColorType {
    type Error = DecodeError;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::Grayscale),
            2 => Ok(Self::Truecolor),
            3 => Ok(Self::Indexed),
            4 => Ok(Self::GrayscaleAlpha),
            6 => Ok(Self::TruecolorAlpha),
            _ => Err(DecodeError::InvalidHeader),
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum InterlaceMethod {
    None,
    Adam7,
}

impl TryFrom<u8> for InterlaceMethod {
    type Error = DecodeError;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::Adam7),
            _ => Err(DecodeError::InvalidHeader),
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum DisposeOp {
    None,
    Background,
    Previous,
}

impl TryFrom<u8> for DisposeOp {
    type Error = DecodeError;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::Background),
            2 => Ok(Self::Previous),
            _ => Err(DecodeError::InvalidData),
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum BlendOp {
    Source,
    Over,
}

impl TryFrom<u8> for BlendOp {
    type Error = DecodeError;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::Source),
            1 => Ok(Self::Over),
            _ => Err(DecodeError::InvalidData),
        }
    }
}

#[derive(Clone, Copy, Default, Eq, PartialEq)]
enum DataPhase {
    #[default]
    Before,
    In,
    After,
}

impl DataPhase {
    const fn has_idat(self) -> bool {
        !matches!(self, Self::Before)
    }

    fn observe(&mut self, kind: &[u8]) {
        if *self == Self::In && kind != b"IDAT" {
            *self = Self::After;
        }
    }
}

#[derive(Clone, Copy, Default, Eq, PartialEq)]
enum FrameData {
    #[default]
    None,
    Idat,
    Fdat {
        seen: bool,
    },
}

struct PngState {
    palette: [[u8; 4]; 256],
    palette_len: usize,
    transparency: Option<[u16; 3]>,
    seen_transparency: bool,
    animation: Option<(u32, u32)>,
    sequence: u32,
    control: Option<Control>,
    compressed: Vec<u8>,
    canvas: Vec<u8>,
    frames: Vec<Frame>,
    data_phase: DataPhase,
    frame_data: FrameData,
}

impl PngState {
    const fn new() -> Self {
        Self {
            palette: [[0, 0, 0, 255]; 256],
            palette_len: 0,
            transparency: None,
            seen_transparency: false,
            animation: None,
            sequence: 0,
            control: None,
            compressed: Vec::new(),
            canvas: Vec::new(),
            frames: Vec::new(),
            data_phase: DataPhase::Before,
            frame_data: FrameData::None,
        }
    }

    fn check_sequence(&mut self, value: u32) -> Result<()> {
        if value != self.sequence {
            return Err(DecodeError::InvalidData);
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or(DecodeError::InvalidData)?;
        Ok(())
    }

    fn finish_frame(&mut self, header: &Header, budget: &mut Budget, last: bool) -> Result<()> {
        // Validate an excluded default image even though it is not emitted as a frame.
        let Some(control) = self.control else {
            header.raster(
                header.width,
                header.height,
                &self.compressed,
                &self.palette[..self.palette_len],
                self.transparency,
                budget,
            )?;
            return Ok(());
        };
        let pixels = header.raster(
            control.area.width as u32,
            control.area.height as u32,
            &self.compressed,
            &self.palette[..self.palette_len],
            self.transparency,
            budget,
        )?;
        let previous = if !last && control.dispose == DisposeOp::Previous {
            Some(
                control
                    .area
                    .snapshot(&self.canvas, header.width as usize, budget)?,
            )
        } else {
            None
        };
        for (row, source) in pixels.chunks_exact(control.area.width * 4).enumerate() {
            let start = ((control.area.y + row) * header.width as usize + control.area.x) * 4;
            let dest = &mut self.canvas[start..start + source.len()];
            if control.blend == BlendOp::Source {
                dest.copy_from_slice(source);
            } else {
                for (dest, source) in dest
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(source.as_chunks::<4>().0.iter())
                {
                    over(dest, source);
                }
            }
        }
        let pixels = if last {
            core::mem::take(&mut self.canvas)
        } else {
            budget.copy(&self.canvas)?
        };
        budget.frame(
            &mut self.frames,
            header.width,
            header.height,
            pixels,
            control.delay,
        )?;
        if last {
            return Ok(());
        }
        match control.dispose {
            DisposeOp::Background => {
                control
                    .area
                    .clear(&mut self.canvas, header.width as usize, [0; 4]);
            }
            DisposeOp::Previous => {
                control.area.restore(
                    &mut self.canvas,
                    header.width as usize,
                    &previous.expect("previous canvas was saved"),
                );
            }
            DisposeOp::None => {}
        }
        Ok(())
    }
}

pub(super) fn decode(data: &[u8]) -> Result<Image> {
    // Parse the chunk stream while retaining the state needed to turn each APNG frame data
    // stream into a fully composited canvas frame.
    let mut r = Reader::new(&data[8..]);
    let (kind, bytes) = chunk(&mut r)?;
    if kind != b"IHDR" || bytes.len() != 13 {
        return Err(DecodeError::InvalidHeader);
    }
    let mut h = Reader::new(bytes);
    let width = h.be32()?;
    let height = h.be32()?;
    let depth = h.byte()?;
    let color = ColorType::try_from(h.byte()?)?;
    if !color.accepts_depth(depth) || h.byte()? != 0 || h.byte()? != 0 {
        return Err(DecodeError::InvalidHeader);
    }
    let interlace = InterlaceMethod::try_from(h.byte()?)?;
    let len = pixel_len(width, height)?;
    let header = Header {
        width,
        height,
        depth,
        color,
        channels: color.channels(),
        interlace,
    };
    let mut budget = Budget::default();
    let mut state = PngState::new();
    loop {
        let (kind, bytes) = chunk(&mut r)?;
        state.data_phase.observe(kind);
        match kind {
            b"PLTE" => {
                if state.data_phase.has_idat()
                    || state.palette_len != 0
                    || state.seen_transparency
                    || matches!(color, ColorType::Grayscale | ColorType::GrayscaleAlpha)
                    || bytes.is_empty()
                    || bytes.len() % 3 != 0
                    || bytes.len() > 768
                {
                    return Err(DecodeError::InvalidData);
                }
                state.palette_len = bytes.len() / 3;
                if color == ColorType::Indexed && state.palette_len > 1 << depth {
                    return Err(DecodeError::InvalidData);
                }
                for (entry, rgb) in state
                    .palette
                    .iter_mut()
                    .zip(bytes.as_chunks::<3>().0.iter())
                {
                    entry[..3].copy_from_slice(rgb);
                }
            }
            b"tRNS" => {
                if state.data_phase.has_idat() || state.seen_transparency {
                    return Err(DecodeError::InvalidData);
                }
                state.seen_transparency = true;
                let mut t = Reader::new(bytes);
                match color {
                    ColorType::Grayscale if bytes.len() == 2 => {
                        let value = t.be16()?;
                        if depth < 16 && value >= 1 << depth {
                            return Err(DecodeError::InvalidData);
                        }
                        state.transparency = Some([value, value, value]);
                    }
                    ColorType::Truecolor if bytes.len() == 6 => {
                        let values = [t.be16()?, t.be16()?, t.be16()?];
                        if depth < 16 && values.iter().any(|v| *v > 255) {
                            return Err(DecodeError::InvalidData);
                        }
                        state.transparency = Some(values);
                    }
                    ColorType::Indexed
                        if state.palette_len != 0
                            && !bytes.is_empty()
                            && bytes.len() <= state.palette_len =>
                    {
                        for (entry, alpha) in state.palette.iter_mut().zip(bytes) {
                            entry[3] = *alpha;
                        }
                    }
                    _ => return Err(DecodeError::InvalidData),
                }
            }
            b"acTL" => {
                if state.data_phase.has_idat() || state.animation.is_some() || bytes.len() != 8 {
                    return Err(DecodeError::InvalidData);
                }
                let mut a = Reader::new(bytes);
                let count = a.be32()?;
                let plays = a.be32()?;
                if count == 0 {
                    return Err(DecodeError::InvalidData);
                }
                state.animation = Some((count, plays));
                state.canvas = budget.zeroed(len)?;
            }
            b"fcTL" => {
                if state.animation.is_none() || bytes.len() != 26 {
                    return Err(DecodeError::InvalidData);
                }
                if state.data_phase.has_idat() {
                    state.finish_frame(&header, &mut budget, false)?;
                    state.compressed.clear();
                } else if state.control.is_some() {
                    return Err(DecodeError::InvalidData);
                }
                let mut c = Reader::new(bytes);
                state.check_sequence(c.be32()?)?;
                let w = c.be32()? as usize;
                let h = c.be32()? as usize;
                let x = c.be32()? as usize;
                let y = c.be32()? as usize;
                let area = Area {
                    x,
                    y,
                    width: w,
                    height: h,
                }
                .validate(width, height)?;
                if !state.data_phase.has_idat()
                    && (x != 0 || y != 0 || w != width as usize || h != height as usize)
                {
                    return Err(DecodeError::InvalidData);
                }
                let num = c.be16()?;
                let den = c.be16()?;
                let delay = Duration::from_nanos(
                    u64::from(num) * 1_000_000_000 / u64::from(if den == 0 { 100 } else { den }),
                );
                let dispose = DisposeOp::try_from(c.byte()?)?;
                let blend = BlendOp::try_from(c.byte()?)?;
                state.frame_data = if state.data_phase.has_idat() {
                    FrameData::Fdat { seen: false }
                } else {
                    FrameData::Idat
                };
                state.control = Some(Control {
                    area,
                    delay,
                    dispose,
                    blend,
                });
            }
            b"IDAT" => {
                if state.data_phase == DataPhase::After
                    || (color == ColorType::Indexed && state.palette_len == 0)
                {
                    return Err(DecodeError::InvalidData);
                }
                state.data_phase = DataPhase::In;
                budget.append(&mut state.compressed, bytes)?;
            }
            b"fdAT" => {
                if !state.data_phase.has_idat()
                    || !matches!(state.frame_data, FrameData::Fdat { .. })
                    || state.control.is_none()
                    || state.animation.is_none()
                    || bytes.len() < 4
                {
                    return Err(DecodeError::InvalidData);
                }
                let mut f = Reader::new(bytes);
                state.check_sequence(f.be32()?)?;
                budget.append(&mut state.compressed, f.take(f.remaining())?)?;
                state.frame_data = FrameData::Fdat { seen: true };
            }
            b"IEND" => {
                if !bytes.is_empty() || !state.data_phase.has_idat() || r.remaining() != 0 {
                    return Err(DecodeError::InvalidData);
                }
                if state.animation.is_none() {
                    let pixels = header.raster(
                        width,
                        height,
                        &state.compressed,
                        &state.palette[..state.palette_len],
                        state.transparency,
                        &mut budget,
                    )?;
                    return Ok(Image::still(Format::Png, width, height, pixels));
                }
                // A non-default animation frame must have an fdAT stream.
                if state.frame_data == (FrameData::Fdat { seen: false }) {
                    return Err(DecodeError::InvalidData);
                }
                state.finish_frame(&header, &mut budget, true)?;
                let (count, plays) = state.animation.expect("animation was checked");
                if state.frames.len() != count as usize {
                    return Err(DecodeError::InvalidData);
                }
                return Ok(Image {
                    format: Format::Png,
                    color_space: ColorSpace::Srgb,
                    frames: state.frames,
                    is_animated: true,
                    loop_count: if plays == 0 {
                        LoopCount::Infinite
                    } else {
                        LoopCount::Finite(plays)
                    },
                });
            }
            _ if kind[0] & 32 == 0 => return Err(DecodeError::UnsupportedFeature),
            _ => {}
        }
    }
}

fn chunk<'a>(r: &mut Reader<'a>) -> Result<(&'a [u8], &'a [u8])> {
    let len = r.be32()? as usize;
    if len > 0x7fff_ffff {
        return Err(DecodeError::InvalidData);
    }
    let kind = r.take(4)?;
    if !kind.iter().all(u8::is_ascii_alphabetic) || kind[2] & 32 != 0 {
        return Err(DecodeError::InvalidData);
    }
    let bytes = r.take(len)?;
    let expected = r.be32()?;
    if chunk_crc(kind, bytes) != expected {
        return Err(DecodeError::InvalidData);
    }
    Ok((kind, bytes))
}

static CRC32: Crc<u32> = Crc::<u32>::new(&CRC_32_ISO_HDLC);

fn chunk_crc(kind: &[u8], data: &[u8]) -> u32 {
    let mut digest = CRC32.digest();
    digest.update(kind);
    digest.update(data);
    digest.finalize()
}

fn over(d: &mut [u8], s: &[u8]) {
    let sa = u32::from(s[3]);
    let da = u32::from(d[3]) * (255 - sa);
    let alpha = sa * 255 + da;
    if alpha == 0 {
        d.fill(0);
        return;
    }
    for channel in 0..3 {
        d[channel] = ((u32::from(s[channel]) * sa * 255 + u32::from(d[channel]) * da + alpha / 2)
            / alpha) as u8;
    }
    d[3] = ((alpha + 127) / 255) as u8;
}

const ADAM7: [(usize, usize, usize, usize); 7] = [
    (0, 0, 8, 8),
    (4, 0, 8, 8),
    (0, 4, 4, 8),
    (2, 0, 4, 4),
    (0, 2, 2, 4),
    (1, 0, 2, 2),
    (0, 1, 1, 2),
];

impl Header {
    fn raster(
        &self,
        width: u32,
        height: u32,
        compressed: &[u8],
        palette: &[[u8; 4]],
        transparency: Option<[u16; 3]>,
        budget: &mut Budget,
    ) -> Result<Vec<u8>> {
        // Adam7 writes seven sparse passes into one output; ordinary PNG uses one full-size pass.
        let passes = if self.interlace == InterlaceMethod::Adam7 {
            &ADAM7[..]
        } else {
            &[(0, 0, 1, 1)][..]
        };
        let width = width as usize;
        let height = height as usize;
        let mut expected = 0usize;
        for &(x, y, dx, dy) in passes {
            let w = width.saturating_sub(x).div_ceil(dx);
            let rows = height.saturating_sub(y).div_ceil(dy);
            if w > 0 && rows > 0 {
                expected = expected
                    .checked_add(
                        png_row_bytes(w, self.channels, self.depth)?
                            .checked_add(1)
                            .ok_or(DecodeError::ImageTooLarge)?
                            .checked_mul(rows)
                            .ok_or(DecodeError::ImageTooLarge)?,
                    )
                    .ok_or(DecodeError::ImageTooLarge)?;
            }
        }
        budget.claim(expected)?;
        // Decompress into an exactly sized, fallibly allocated buffer; no unbounded growth.
        let mut raw = Vec::new();
        raw.try_reserve_exact(expected)
            .map_err(|_| DecodeError::ImageTooLarge)?;
        raw.resize(expected, 0);
        let mut inflater = miniz_oxide::inflate::core::DecompressorOxide::new();
        // Chunk CRCs already cover the compressed stream, so skip the redundant Adler-32.
        let flags = miniz_oxide::inflate::core::inflate_flags::TINFL_FLAG_PARSE_ZLIB_HEADER
            | miniz_oxide::inflate::core::inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF
            | miniz_oxide::inflate::core::inflate_flags::TINFL_FLAG_IGNORE_ADLER32;
        let (status, consumed, written) =
            miniz_oxide::inflate::core::decompress(&mut inflater, compressed, &mut raw, 0, flags);
        if status != miniz_oxide::inflate::TINFLStatus::Done
            || consumed != compressed.len()
            || written != expected
        {
            return Err(DecodeError::InvalidData);
        }
        let mut pixels = budget.zeroed(pixel_len(width as u32, height as u32)?)?;
        let bpp = (self.channels * self.depth as usize).div_ceil(8);
        let mut cursor = 0;
        for &(x, y, dx, dy) in passes {
            let w = width.saturating_sub(x).div_ceil(dx);
            let rows = height.saturating_sub(y).div_ceil(dy);
            if w == 0 || rows == 0 {
                continue;
            }
            let bytes = png_row_bytes(w, self.channels, self.depth)?;
            let mut previous = None;
            for row in 0..rows {
                let filter = raw[cursor];
                cursor += 1;
                let scan_start = cursor;
                let scan = if let Some(previous_start) = previous {
                    let (before, current) = raw.split_at_mut(scan_start);
                    let previous = &before[previous_start..previous_start + bytes];
                    let scan = &mut current[..bytes];
                    unfilter(scan, Some(previous), bpp, filter)?;
                    scan
                } else {
                    let scan = &mut raw[scan_start..scan_start + bytes];
                    unfilter(scan, None, bpp, filter)?;
                    scan
                };
                let start = (y + row * dy) * width + x;
                let dest = pixels.as_chunks_mut::<4>().0[start..]
                    .iter_mut()
                    .step_by(dx)
                    .take(w);
                self.expand(scan, dest, palette, transparency)?;
                previous = Some(scan_start);
                cursor += bytes;
            }
        }
        Ok(pixels)
    }
}

impl Header {
    // Converts one unfiltered scanline to RGBA8, choosing the sample layout once per row.
    fn expand<'a>(
        &self,
        scan: &[u8],
        dest: impl Iterator<Item = &'a mut [u8; 4]>,
        palette: &[[u8; 4]],
        transparency: Option<[u16; 3]>,
    ) -> Result<()> {
        let alpha = |key: bool| if key { 0 } else { 255 };
        match (self.color, self.depth) {
            (ColorType::TruecolorAlpha, 8) => {
                for (p, s) in dest.zip(scan.as_chunks::<4>().0) {
                    *p = *s;
                }
            }
            (ColorType::TruecolorAlpha, _) => {
                for (p, s) in dest.zip(scan.as_chunks::<8>().0) {
                    *p = core::array::from_fn(|i| scale_16_to_8(sample16(s, i)));
                }
            }
            (ColorType::Truecolor, 8) => {
                let key = transparency.map(|t| t.map(|v| v as u8));
                for (p, s) in dest.zip(scan.as_chunks::<3>().0) {
                    *p = [s[0], s[1], s[2], alpha(key == Some(*s))];
                }
            }
            (ColorType::Truecolor, _) => {
                for (p, s) in dest.zip(scan.as_chunks::<6>().0) {
                    let samples = core::array::from_fn(|i| sample16(s, i));
                    let [r, g, b] = samples.map(scale_16_to_8);
                    *p = [r, g, b, alpha(transparency == Some(samples))];
                }
            }
            (ColorType::GrayscaleAlpha, 8) => {
                for (p, s) in dest.zip(scan.as_chunks::<2>().0) {
                    *p = [s[0], s[0], s[0], s[1]];
                }
            }
            (ColorType::GrayscaleAlpha, _) => {
                for (p, s) in dest.zip(scan.as_chunks::<4>().0) {
                    let value = scale_16_to_8(sample16(s, 0));
                    *p = [value, value, value, scale_16_to_8(sample16(s, 1))];
                }
            }
            (ColorType::Grayscale, 16) => {
                for (p, s) in dest.zip(scan.as_chunks::<2>().0) {
                    let sample = sample16(s, 0);
                    let value = scale_16_to_8(sample);
                    *p = [
                        value,
                        value,
                        value,
                        alpha(transparency.is_some_and(|t| t[0] == sample)),
                    ];
                }
            }
            (ColorType::Grayscale | ColorType::Indexed, depth) => {
                let depth = depth as usize;
                let mask = ((1u16 << depth) - 1) as u8;
                for (col, p) in dest.enumerate() {
                    let bit = col * depth;
                    let sample = (scan[bit / 8] >> (8 - depth - bit % 8)) & mask;
                    *p = if self.color == ColorType::Indexed {
                        *palette
                            .get(sample as usize)
                            .ok_or(DecodeError::InvalidData)?
                    } else {
                        let value = (u16::from(sample) * 255 / u16::from(mask)) as u8;
                        let key = transparency.is_some_and(|t| t[0] == u16::from(sample));
                        [value, value, value, alpha(key)]
                    };
                }
            }
        }
        Ok(())
    }
}

fn png_row_bytes(width: usize, channels: usize, depth: u8) -> Result<usize> {
    width
        .checked_mul(channels)
        .and_then(|samples| samples.checked_mul(depth as usize))
        .and_then(|bits| bits.checked_add(7))
        .map(|bits| bits / 8)
        .ok_or(DecodeError::ImageTooLarge)
}

// Reads the big-endian 16-bit sample at `index`.
const fn sample16(bytes: &[u8], index: usize) -> u16 {
    u16::from_be_bytes([bytes[index * 2], bytes[index * 2 + 1]])
}

const fn scale_16_to_8(value: u16) -> u8 {
    ((value as u32 + 128) / 257) as u8
}

fn unfilter(row: &mut [u8], previous: Option<&[u8]>, bpp: usize, filter: u8) -> Result<()> {
    if filter > 4 || previous.is_some_and(|previous| row.len() != previous.len()) {
        return Err(DecodeError::InvalidData);
    }
    if let (2, Some(previous)) = (filter, previous) {
        add_bytes(row, previous);
        return Ok(());
    }
    // Rows of 8- and 16-bit samples hold whole pixels; packed samples use one-byte pixels.
    match bpp {
        1 => unfilter_pixels::<1>(row, previous, filter),
        2 => unfilter_pixels::<2>(row, previous, filter),
        3 => unfilter_pixels::<3>(row, previous, filter),
        4 => unfilter_pixels::<4>(row, previous, filter),
        6 => unfilter_pixels::<6>(row, previous, filter),
        8 => unfilter_pixels::<8>(row, previous, filter),
        _ => unreachable!("PNG pixels are 1, 2, 3, 4, 6 or 8 bytes"),
    }
    Ok(())
}

// Carries the left pixel `a` and upper-left pixel `c` in registers across the row.
fn unfilter_pixels<const N: usize>(row: &mut [u8], previous: Option<&[u8]>, filter: u8) {
    let row = row.as_chunks_mut::<N>().0;
    let mut a = [0u8; N];
    match (filter, previous) {
        (1 | 4, None) | (1, Some(_)) => {
            for x in row {
                for k in 0..N {
                    x[k] = x[k].wrapping_add(a[k]);
                }
                a = *x;
            }
        }
        (3, None) => {
            for x in row {
                for k in 0..N {
                    x[k] = x[k].wrapping_add(a[k] / 2);
                }
                a = *x;
            }
        }
        (3, Some(previous)) => {
            for (x, b) in row.iter_mut().zip(previous.as_chunks::<N>().0) {
                for k in 0..N {
                    x[k] = x[k].wrapping_add(((u16::from(a[k]) + u16::from(b[k])) / 2) as u8);
                }
                a = *x;
            }
        }
        (4, Some(previous)) => {
            let mut c = [0u8; N];
            for (x, b) in row.iter_mut().zip(previous.as_chunks::<N>().0) {
                for k in 0..N {
                    x[k] = x[k].wrapping_add(paeth(a[k], b[k], c[k]));
                }
                a = *x;
                c = *b;
            }
        }
        _ => {}
    }
}

fn add_bytes(dest: &mut [u8], source: &[u8]) {
    let (dest_chunks, dest_tail) = dest.as_chunks_mut::<16>();
    let (source_chunks, source_tail) = source.as_chunks::<16>();
    for (dest, source) in dest_chunks.iter_mut().zip(source_chunks) {
        for (dest, source) in dest.iter_mut().zip(source) {
            *dest = dest.wrapping_add(*source);
        }
    }
    for (dest, source) in dest_tail.iter_mut().zip(source_tail) {
        *dest = dest.wrapping_add(*source);
    }
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (a16, b16, c16) = (i16::from(a), i16::from(b), i16::from(c));
    let pa = (b16 - c16).abs();
    let pb = (a16 - c16).abs();
    let pc = (a16 + b16 - 2 * c16).abs();
    // Selects rather than branches; ties prefer a, then b, as the specification requires.
    let (predictor, best) = if pb < pa { (b, pb) } else { (a, pa) };
    if pc < best { c } else { predictor }
}

// MARK: Encoder
fn write_chunk(
    out: &mut Vec<u8>,
    kind: &[u8; 4],
    data: &[u8],
) -> core::result::Result<(), EncodeError> {
    let len = u32::try_from(data.len()).map_err(|_| EncodeError::ImageTooLarge)?;
    out.try_reserve(
        data.len()
            .checked_add(12)
            .ok_or(EncodeError::ImageTooLarge)?,
    )
    .map_err(|_| EncodeError::ImageTooLarge)?;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&chunk_crc(kind, data).to_be_bytes());
    Ok(())
}

#[derive(Clone, Copy)]
enum PngColor {
    Gray(u8),
    Rgb,
    GrayAlpha,
    Rgba,
    Indexed(u8),
}

impl PngColor {
    const fn channels(self) -> usize {
        match self {
            Self::Gray(_) => 1,
            Self::Rgb => 3,
            Self::GrayAlpha => 2,
            Self::Rgba => 4,
            Self::Indexed(_) => 1,
        }
    }

    const fn code(self) -> u8 {
        match self {
            Self::Gray(_) => 0,
            Self::Rgb => 2,
            Self::GrayAlpha => 4,
            Self::Rgba => 6,
            Self::Indexed(_) => 3,
        }
    }

    const fn depth(self) -> u8 {
        match self {
            Self::Gray(depth) | Self::Indexed(depth) => depth,
            _ => 8,
        }
    }
}

fn png_palette_slot(lookup: &[u64; 512], key: u32) -> usize {
    let mut slot = (key.wrapping_mul(0x9e37_79b1) as usize) & (lookup.len() - 1);
    while lookup[slot] != 0 && lookup[slot] >> 9 != u64::from(key) {
        slot = (slot + 1) & (lookup.len() - 1);
    }
    slot
}

fn png_palette_lookup(palette: &[[u8; 4]]) -> [u64; 512] {
    let mut lookup = [0u64; 512];
    for (index, pixel) in palette.iter().enumerate() {
        let key = u32::from_be_bytes(*pixel);
        lookup[png_palette_slot(&lookup, key)] = (u64::from(key) << 9) | (index as u64 + 1);
    }
    lookup
}

fn png_palette<'a>(bitmaps: impl Iterator<Item = &'a Bitmap>) -> Option<Vec<[u8; 4]>> {
    let mut palette = Vec::with_capacity(256);
    let mut lookup = [0u64; 512];
    for bitmap in bitmaps {
        for pixel in bitmap.data.as_chunks::<4>().0 {
            let key = u32::from_be_bytes(*pixel);
            let slot = png_palette_slot(&lookup, key);
            if lookup[slot] == 0 {
                if palette.len() == 256 {
                    return None;
                }
                palette.push(*pixel);
                lookup[slot] = u64::from(key) << 9 | 1;
            }
        }
    }
    // Translucent entries first keep the tRNS chunk as short as possible.
    palette.sort_by_key(|pixel| pixel[3] == 255);
    Some(palette)
}

fn png_transparent_key<'a>(bitmaps: impl Iterator<Item = &'a Bitmap> + Clone) -> Option<[u8; 3]> {
    let mut key = None;
    for bitmap in bitmaps.clone() {
        for pixel in bitmap.data.as_chunks::<4>().0 {
            match pixel[3] {
                0 if key.is_none_or(|color| color == pixel[..3]) => {
                    key = Some([pixel[0], pixel[1], pixel[2]]);
                }
                255 => {}
                _ => return None,
            }
        }
    }
    let key = key?;
    for bitmap in bitmaps {
        if bitmap
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] == 255 && pixel[..3] == key)
        {
            return None;
        }
    }
    Some(key)
}

fn png_color<'a>(bitmaps: impl Iterator<Item = &'a Bitmap>, ignore_alpha: bool) -> PngColor {
    let mut color = false;
    let mut alpha = false;
    let mut gray_depth = 1;
    for bitmap in bitmaps {
        for p in bitmap.data.as_chunks::<4>().0 {
            color |= p[0] != p[1] || p[1] != p[2];
            alpha |= !ignore_alpha && p[3] != 255;
            gray_depth = gray_depth.max(if p[0] == 0 || p[0] == 255 {
                1
            } else if p[0] % 85 == 0 {
                2
            } else if p[0] % 17 == 0 {
                4
            } else {
                8
            });
            if color && alpha {
                return PngColor::Rgba;
            }
        }
    }
    match (color, alpha) {
        (false, false) => PngColor::Gray(gray_depth),
        (true, false) => PngColor::Rgb,
        (false, true) => PngColor::GrayAlpha,
        (true, true) => PngColor::Rgba,
    }
}

fn png_filter(row: &[u8], previous: &[u8], bpp: usize, filter: u8, output: &mut Vec<u8>) {
    output.clear();
    output.push(filter);
    let (head, tail) = row.split_at(bpp);
    let (up_head, up_tail) = previous.split_at(bpp);
    // Splitting off the first pixel removes the missing-left-neighbor branch from each loop.
    match filter {
        0 => output.extend_from_slice(row),
        1 => {
            output.extend_from_slice(head);
            output.extend(
                tail.iter()
                    .zip(row)
                    .map(|(&value, &left)| value.wrapping_sub(left)),
            );
        }
        2 => output.extend(
            row.iter()
                .zip(previous)
                .map(|(&value, &up)| value.wrapping_sub(up)),
        ),
        3 => {
            output.extend(
                head.iter()
                    .zip(up_head)
                    .map(|(&value, &up)| value.wrapping_sub(up / 2)),
            );
            output.extend(
                tail.iter()
                    .zip(up_tail)
                    .zip(row)
                    .map(|((&value, &up), &left)| {
                        value.wrapping_sub(((u16::from(left) + u16::from(up)) / 2) as u8)
                    }),
            );
        }
        _ => {
            output.extend(
                head.iter()
                    .zip(up_head)
                    .map(|(&value, &up)| value.wrapping_sub(up)),
            );
            output.extend(tail.iter().zip(up_tail).zip(row.iter().zip(previous)).map(
                |((&value, &up), (&left, &upper_left))| {
                    value.wrapping_sub(paeth(left, up, upper_left))
                },
            ));
        }
    }
}

fn png_best_filter(row: &[u8], previous: &[u8], bpp: usize) -> u8 {
    let mut scores = [0u64; 5];
    let mut score = |value: u8, left: u8, up: u8, upper_left: u8| {
        let predictors = [
            0,
            left,
            up,
            ((u16::from(left) + u16::from(up)) / 2) as u8,
            paeth(left, up, upper_left),
        ];
        for (score, predictor) in scores.iter_mut().zip(predictors) {
            let filtered = value.wrapping_sub(predictor);
            *score += u64::from(filtered.min(filtered.wrapping_neg()));
        }
    };
    for (&value, &up) in row[..bpp].iter().zip(previous) {
        score(value, 0, up, 0);
    }
    for ((&value, &up), (&left, &upper_left)) in row[bpp..]
        .iter()
        .zip(&previous[bpp..])
        .zip(row.iter().zip(previous))
    {
        score(value, left, up, upper_left);
    }
    scores
        .iter()
        .enumerate()
        .min_by_key(|&(_, score)| score)
        .expect("PNG has filters")
        .0 as u8
}

fn png_brute_filter(
    row: &[u8],
    previous: &[u8],
    bpp: usize,
    output: &[u8],
    scratch: &mut Vec<u8>,
    trial: &mut Vec<u8>,
) -> u8 {
    // Deflating each candidate after a window of earlier output lets matches against
    // previous rows count, which per-row byte statistics cannot see.
    const WINDOW: usize = 16 * 1024;
    let context = &output[output.len().saturating_sub(WINDOW)..];
    (0..5)
        .min_by_key(|&filter| {
            png_filter(row, previous, bpp, filter, scratch);
            trial.clear();
            trial.extend_from_slice(context);
            trial.extend_from_slice(scratch);
            miniz_oxide::deflate::compress_to_vec(trial, 3).len()
        })
        .expect("PNG has filters")
}

fn png_filtered(
    raw: &[u8],
    row_len: usize,
    bpp: usize,
    mut choose: impl FnMut(&[u8], &[u8], &[u8]) -> u8,
) -> core::result::Result<Vec<u8>, EncodeError> {
    let mut output = Vec::new();
    output
        .try_reserve_exact(raw.len())
        .map_err(|_| EncodeError::ImageTooLarge)?;
    let zero = vec![0; row_len];
    let mut previous: &[u8] = &zero;
    let mut filtered_row = Vec::with_capacity(row_len + 1);
    for scanline in raw.chunks_exact(row_len + 1) {
        let row = &scanline[1..];
        let filter = choose(row, previous, &output);
        png_filter(row, previous, bpp, filter, &mut filtered_row);
        output.extend_from_slice(&filtered_row);
        previous = row;
    }
    Ok(output)
}

fn png_palette_index(pixel: &[u8; 4], lookup: &[u64; 512]) -> u8 {
    (lookup[png_palette_slot(lookup, u32::from_be_bytes(*pixel))] - 1) as u8
}

fn png_row(
    source: &[u8],
    color: PngColor,
    palette_lookup: Option<&[u64; 512]>,
    row_len: usize,
    row: &mut Vec<u8>,
) {
    row.clear();
    if let PngColor::Gray(depth) | PngColor::Indexed(depth) = color
        && depth < 8
    {
        row.resize(row_len, 0);
        for (x, pixel) in source.as_chunks::<4>().0.iter().enumerate() {
            let index = match color {
                PngColor::Gray(_) => pixel[0] / (255 / ((1 << depth) - 1)),
                PngColor::Indexed(_) => {
                    png_palette_index(pixel, palette_lookup.expect("indexed PNG has palette"))
                }
                _ => unreachable!(),
            };
            let bit = x * usize::from(depth);
            row[bit / 8] |= index << (8 - usize::from(depth) - bit % 8);
        }
    } else {
        for pixel in source.as_chunks::<4>().0 {
            match color {
                PngColor::Gray(_) => row.push(pixel[0]),
                PngColor::Rgb => row.extend_from_slice(&pixel[..3]),
                PngColor::GrayAlpha => row.extend_from_slice(&[pixel[0], pixel[3]]),
                PngColor::Rgba => row.extend_from_slice(pixel),
                PngColor::Indexed(_) => row.push(png_palette_index(
                    pixel,
                    palette_lookup.expect("indexed PNG has palette"),
                )),
            }
        }
    }
}

fn compressed_frame(
    bitmap: &Bitmap,
    color: PngColor,
    palette_lookup: Option<&[u64; 512]>,
    style: EncodingStyle,
) -> core::result::Result<Vec<u8>, EncodeError> {
    let source_stride = bitmap.width as usize * 4;
    let row_len = if let PngColor::Gray(depth) | PngColor::Indexed(depth) = color
        && depth < 8
    {
        (bitmap.width as usize * usize::from(depth)).div_ceil(8)
    } else {
        bitmap.width as usize * color.channels()
    };
    let len = row_len
        .checked_add(1)
        .and_then(|n| n.checked_mul(bitmap.height as usize))
        .ok_or(EncodeError::ImageTooLarge)?;
    let mut raw = Vec::new();
    raw.try_reserve_exact(len)
        .map_err(|_| EncodeError::ImageTooLarge)?;
    let mut row = Vec::with_capacity(row_len);
    for source in bitmap.data.chunks_exact(source_stride) {
        png_row(source, color, palette_lookup, row_len, &mut row);
        raw.push(0);
        raw.extend_from_slice(&row);
    }
    let bpp = if color.depth() < 8 {
        1
    } else {
        color.channels()
    };
    let mut best: Option<Vec<u8>> = None;
    let mut consider = |data: &[u8], level| {
        let candidate = miniz_oxide::deflate::compress_to_vec_zlib(data, level);
        if best
            .as_ref()
            .is_none_or(|best| candidate.len() < best.len())
        {
            best = Some(candidate);
        }
    };
    let msad = |row: &[u8], previous: &[u8], _: &[u8]| png_best_filter(row, previous, bpp);
    if style == EncodingStyle::NormalCompression {
        consider(&raw, 6);
        // Palette and packed samples rarely benefit from prediction. Unfiltered rows
        // also often win on screenshots, whose repeated content deflate matches directly.
        if color.depth() == 8 && !matches!(color, PngColor::Indexed(_)) {
            consider(&png_filtered(&raw, row_len, bpp, msad)?, 6);
        }
        return Ok(best.expect("PNG has a candidate"));
    }
    // Level 10 usually wins, but level 6 occasionally parses the likeliest candidates better.
    consider(&raw, 10);
    let filtered = png_filtered(&raw, row_len, bpp, msad)?;
    consider(&filtered, 6);
    consider(&filtered, 10);
    let mut scratch = Vec::with_capacity(row_len + 1);
    let mut trial = Vec::new();
    let brute = png_filtered(&raw, row_len, bpp, |row, previous, output| {
        png_brute_filter(row, previous, bpp, output, &mut scratch, &mut trial)
    })?;
    consider(&brute, 10);
    Ok(best.expect("PNG has a candidate"))
}

const fn png_palette_depth(len: usize) -> u8 {
    match len {
        0..=2 => 1,
        3..=4 => 2,
        5..=16 => 4,
        _ => 8,
    }
}

pub(super) fn encode(
    bitmap: &Bitmap,
    animation: Option<(LoopCount, &[Frame])>,
    style: EncodingStyle,
) -> core::result::Result<Vec<u8>, EncodeError> {
    let best = encode_layouts(bitmap, animation, style)?;
    if style == EncodingStyle::NormalCompression {
        return Ok(best);
    }
    let cleared = match animation {
        Some((loop_count, frames)) => {
            let cleared: Vec<_> = frames
                .iter()
                .map(|frame| clear_transparent(&frame.bitmap))
                .collect();
            if cleared.iter().all(Option::is_none) {
                return Ok(best);
            }
            let frames: Vec<_> = frames
                .iter()
                .zip(cleared)
                .map(|(frame, cleared)| {
                    Frame::new(
                        cleared.unwrap_or_else(|| Bitmap {
                            data: frame.bitmap.data.clone(),
                            ..frame.bitmap
                        }),
                        frame.delay,
                    )
                })
                .collect();
            encode_layouts(&frames[0].bitmap, Some((loop_count, &frames)), style)?
        }
        None => match clear_transparent(bitmap) {
            Some(cleared) => encode_layouts(&cleared, None, style)?,
            None => return Ok(best),
        },
    };
    Ok(if cleared.len() < best.len() {
        cleared
    } else {
        best
    })
}

// Fully transparent pixels look the same in any color, and uniform ones compress best.
fn clear_transparent(bitmap: &Bitmap) -> Option<Bitmap> {
    let hidden = |pixel: &[u8; 4]| pixel[3] == 0 && *pixel != [0; 4];
    if !bitmap.data.as_chunks::<4>().0.iter().any(hidden) {
        return None;
    }
    let mut data = bitmap.data.clone();
    for pixel in data.as_chunks_mut::<4>().0 {
        if pixel[3] == 0 {
            *pixel = [0; 4];
        }
    }
    Some(Bitmap { data, ..*bitmap })
}

fn encode_layouts(
    bitmap: &Bitmap,
    animation: Option<(LoopCount, &[Frame])>,
    style: EncodingStyle,
) -> core::result::Result<Vec<u8>, EncodeError> {
    let bitmaps: Vec<_> = match animation {
        Some((_, frames)) => frames.iter().map(|frame| &frame.bitmap).collect(),
        None => vec![bitmap],
    };
    let palette = png_palette(bitmaps.iter().copied());
    let key = png_transparent_key(bitmaps.iter().copied());
    if style == EncodingStyle::NormalCompression {
        // Pick the layout that is usually smallest instead of compressing each one:
        // a palette unless packed grayscale is as small, then a color key over alpha.
        let gray_depth = match png_color(bitmaps.iter().copied(), false) {
            PngColor::Gray(depth) => Some(depth),
            _ => None,
        };
        let palette = palette.filter(|palette| {
            gray_depth.is_none_or(|depth| png_palette_depth(palette.len()) < depth)
        });
        let key = key.filter(|_| palette.is_none());
        return encode_png_variant(bitmap, animation, style, palette.as_deref(), key);
    }
    let mut best = encode_png_variant(bitmap, animation, style, None, None)?;
    let candidates = [
        palette.map(|palette| (Some(palette), None)),
        key.map(|key| (None, Some(key))),
    ];
    for (palette, key) in candidates.into_iter().flatten() {
        let candidate = encode_png_variant(bitmap, animation, style, palette.as_deref(), key)?;
        if candidate.len() < best.len() {
            best = candidate;
        }
    }
    Ok(best)
}

fn encode_png_variant(
    bitmap: &Bitmap,
    animation: Option<(LoopCount, &[Frame])>,
    style: EncodingStyle,
    palette: Option<&[[u8; 4]]>,
    transparent_key: Option<[u8; 3]>,
) -> core::result::Result<Vec<u8>, EncodeError> {
    let color = if let Some(palette) = palette {
        PngColor::Indexed(png_palette_depth(palette.len()))
    } else if let Some((_, frames)) = animation {
        png_color(
            frames.iter().map(|frame| &frame.bitmap),
            transparent_key.is_some(),
        )
    } else {
        png_color(core::iter::once(bitmap), transparent_key.is_some())
    };
    let palette_lookup = palette.map(png_palette_lookup);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&bitmap.width.to_be_bytes());
    ihdr.extend_from_slice(&bitmap.height.to_be_bytes());
    ihdr.extend_from_slice(&[color.depth(), color.code(), 0, 0, 0]);
    write_chunk(&mut out, b"IHDR", &ihdr)?;
    if let Some(key) = transparent_key {
        if matches!(color, PngColor::Gray(_)) {
            let depth = color.depth();
            let value = key[0] / (255 / ((1 << depth) - 1));
            write_chunk(&mut out, b"tRNS", &u16::from(value).to_be_bytes())?;
        } else {
            let samples = [0, key[0], 0, key[1], 0, key[2]];
            write_chunk(&mut out, b"tRNS", &samples)?;
        }
    }
    if let Some(palette) = palette {
        let mut colors = Vec::with_capacity(palette.len() * 3);
        for pixel in palette {
            colors.extend_from_slice(&pixel[..3]);
        }
        write_chunk(&mut out, b"PLTE", &colors)?;
        if let Some(last) = palette.iter().rposition(|pixel| pixel[3] != 255) {
            let alpha: Vec<_> = palette[..=last].iter().map(|pixel| pixel[3]).collect();
            write_chunk(&mut out, b"tRNS", &alpha)?;
        }
    }
    if let Some((loop_count, frames)) = animation {
        let plays = match loop_count {
            LoopCount::Infinite => 0,
            LoopCount::Finite(n) if n != 0 => n,
            _ => return Err(EncodeError::InvalidTiming),
        };
        let count = u32::try_from(frames.len()).map_err(|_| EncodeError::ImageTooLarge)?;
        let mut actl = Vec::with_capacity(8);
        actl.extend_from_slice(&count.to_be_bytes());
        actl.extend_from_slice(&plays.to_be_bytes());
        write_chunk(&mut out, b"acTL", &actl)?;
        let mut sequence = 0u32;
        for (i, frame) in frames.iter().enumerate() {
            let nanos = frame.delay.as_nanos();
            let mut divisor = 1_000_000_000u128;
            let mut remainder = nanos;
            while remainder != 0 {
                (divisor, remainder) = (remainder, divisor % remainder);
            }
            let delay = u16::try_from(nanos / divisor).map_err(|_| EncodeError::InvalidTiming)?;
            let timebase =
                u16::try_from(1_000_000_000 / divisor).map_err(|_| EncodeError::InvalidTiming)?;
            let region = if i != 0 {
                Some(super::encoder::crop_changed(
                    &frames[i - 1].bitmap,
                    &frame.bitmap,
                )?)
            } else {
                None
            };
            let (mut x, mut y, mut frame_bitmap) = if let Some((x, y, bitmap)) = &region {
                (*x, *y, bitmap)
            } else {
                (0, 0, &frame.bitmap)
            };
            let mut compressed =
                compressed_frame(frame_bitmap, color, palette_lookup.as_ref(), style)?;
            if region.is_some() && style == EncodingStyle::MaxCompression {
                let full = compressed_frame(&frame.bitmap, color, palette_lookup.as_ref(), style)?;
                if full.len() < compressed.len() {
                    (x, y, frame_bitmap) = (0, 0, &frame.bitmap);
                    compressed = full;
                }
            }
            let mut control = Vec::with_capacity(26);
            control.extend_from_slice(&sequence.to_be_bytes());
            control.extend_from_slice(&frame_bitmap.width.to_be_bytes());
            control.extend_from_slice(&frame_bitmap.height.to_be_bytes());
            control.extend_from_slice(&x.to_be_bytes());
            control.extend_from_slice(&y.to_be_bytes());
            control.extend_from_slice(&delay.to_be_bytes());
            control.extend_from_slice(&timebase.to_be_bytes());
            control.extend_from_slice(&[0, 0]);
            write_chunk(&mut out, b"fcTL", &control)?;
            sequence = sequence.checked_add(1).ok_or(EncodeError::ImageTooLarge)?;
            if i == 0 {
                write_chunk(&mut out, b"IDAT", &compressed)?;
            } else {
                let mut data = Vec::new();
                data.try_reserve_exact(
                    compressed
                        .len()
                        .checked_add(4)
                        .ok_or(EncodeError::ImageTooLarge)?,
                )
                .map_err(|_| EncodeError::ImageTooLarge)?;
                data.extend_from_slice(&sequence.to_be_bytes());
                data.extend_from_slice(&compressed);
                write_chunk(&mut out, b"fdAT", &data)?;
                sequence = sequence.checked_add(1).ok_or(EncodeError::ImageTooLarge)?;
            }
        }
    } else {
        write_chunk(
            &mut out,
            b"IDAT",
            &compressed_frame(bitmap, color, palette_lookup.as_ref(), style)?,
        )?;
    }
    write_chunk(&mut out, b"IEND", &[])?;
    Ok(out)
}

pub(super) fn encode_animation(
    frames: &[Frame],
    loop_count: LoopCount,
    style: EncodingStyle,
) -> core::result::Result<Vec<u8>, EncodeError> {
    encode(&frames[0].bitmap, Some((loop_count, frames)), style)
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        EncodeOptions, Format, encode, encode_animation, encode_animation_with_options,
        encode_with_options,
    };

    fn smallest() -> EncodeOptions {
        EncodeOptions {
            style: EncodingStyle::MaxCompression,
            ..EncodeOptions::default()
        }
    }

    use crate::MAX_BYTES;

    #[test]
    fn indexed_lookup_handles_full_palette() {
        let mut pixels = Vec::new();
        for y in 0..16u8 {
            for x in 0..16u8 {
                pixels.extend_from_slice(&[x * 16, y * 16, x ^ y, 255]);
            }
        }
        let bitmap = Bitmap::new(16, 16, pixels).unwrap();
        let palette = png_palette(core::iter::once(&bitmap)).unwrap();
        assert_eq!(palette.len(), 256);
        let encoded = encode_png_variant(
            &bitmap,
            None,
            EncodingStyle::MaxCompression,
            Some(&palette),
            None,
        )
        .expect("indexed PNG");
        assert_eq!(crate::decode(&encoded).unwrap().pixels(), bitmap.data());
    }

    #[test]
    fn max_png_uses_transparent_color_without_an_alpha_channel() {
        let mut pixels = Vec::new();
        for y in 0..32u8 {
            for x in 0..32u8 {
                pixels.extend_from_slice(&[x * 8, y * 8, x ^ y, 255]);
            }
        }
        pixels[3] = 0;
        let bitmap = Bitmap::new(32, 32, pixels).unwrap();
        let normal = encode(&bitmap, Format::Png).unwrap();
        let max = encode_with_options(&bitmap, Format::Png, smallest()).unwrap();
        assert!(max.windows(4).any(|bytes| bytes == b"tRNS"));
        assert!(max.len() < normal.len());
        assert_eq!(crate::decode(&max).unwrap().pixels(), bitmap.data());

        let gray = Bitmap::new(2, 1, vec![85, 85, 85, 0, 170, 170, 170, 255]).unwrap();
        let encoded = encode_png_variant(
            &gray,
            None,
            EncodingStyle::NormalCompression,
            None,
            Some([85, 85, 85]),
        )
        .unwrap();
        assert_eq!(encoded[24], 2);
        assert_eq!(crate::decode(&encoded).unwrap().pixels(), gray.data());

        let collision = Bitmap::new(2, 1, vec![85, 85, 85, 0, 85, 85, 85, 255]).unwrap();
        assert_eq!(png_transparent_key(core::iter::once(&collision)), None);

        let mut second = bitmap.data().to_vec();
        second[4..8].copy_from_slice(&[255, 0, 0, 255]);
        let frames = [
            Frame::new(bitmap, Duration::from_millis(100)),
            Frame::new(
                Bitmap::new(32, 32, second).unwrap(),
                Duration::from_millis(100),
            ),
        ];
        let encoded = encode_png_variant(
            &frames[0].bitmap,
            Some((LoopCount::Infinite, &frames)),
            EncodingStyle::MaxCompression,
            None,
            Some([0, 0, 0]),
        )
        .unwrap();
        let decoded = crate::decode(&encoded).unwrap();
        for (actual, expected) in decoded.frames().iter().zip(&frames) {
            assert_eq!(actual.pixels(), expected.pixels());
        }
    }

    #[test]
    fn format_values_are_validated() {
        assert!(matches!(
            ColorType::try_from(6),
            Ok(ColorType::TruecolorAlpha)
        ));
        assert!(ColorType::try_from(1).is_err());
        assert!(matches!(
            InterlaceMethod::try_from(1),
            Ok(InterlaceMethod::Adam7)
        ));
        assert!(InterlaceMethod::try_from(2).is_err());
        assert!(DisposeOp::try_from(3).is_err());
        assert!(BlendOp::try_from(2).is_err());
    }

    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        out.extend_from_slice(&chunk_crc(kind, data).to_be_bytes());
    }

    fn png_header(width: u32, height: u32, depth: u8, color: u8) -> Vec<u8> {
        png_header_with_interlace(width, height, depth, color, 0)
    }

    fn png_header_with_interlace(
        width: u32,
        height: u32,
        depth: u8,
        color: u8,
        interlace: u8,
    ) -> Vec<u8> {
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut header = width.to_be_bytes().to_vec();
        header.extend_from_slice(&height.to_be_bytes());
        header.extend_from_slice(&[depth, color, 0, 0, interlace]);
        chunk(&mut out, b"IHDR", &header);
        out
    }

    fn png_finish(mut out: Vec<u8>, scanlines: &[u8]) -> Vec<u8> {
        chunk(
            &mut out,
            b"IDAT",
            &miniz_oxide::deflate::compress_to_vec_zlib(scanlines, 6),
        );
        chunk(&mut out, b"IEND", &[]);
        out
    }

    #[test]
    fn decodes_16_bit_png_types_to_rgba8() {
        let mut grayscale = png_header(2, 1, 16, 0);
        chunk(&mut grayscale, b"tRNS", &[0xab, 0xcd]);
        assert_eq!(
            decode(&png_finish(grayscale, &[0, 0x12, 0x34, 0xab, 0xcd]))
                .expect("16-bit grayscale")
                .pixels(),
            &[0x12, 0x12, 0x12, 255, 0xab, 0xab, 0xab, 0]
        );

        let mut truecolor = png_header(1, 1, 16, 2);
        chunk(
            &mut truecolor,
            b"tRNS",
            &[0x12, 0x35, 0x56, 0x78, 0x9a, 0xbc],
        );
        assert_eq!(
            decode(&png_finish(
                truecolor,
                &[0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc]
            ))
            .expect("16-bit truecolor")
            .pixels(),
            &[0x12, 0x56, 0x9a, 255]
        );

        assert_eq!(
            decode(&png_finish(
                png_header(1, 1, 16, 4),
                &[0, 0x12, 0x34, 0xab, 0xcd]
            ))
            .expect("16-bit grayscale-alpha")
            .pixels(),
            &[0x12, 0x12, 0x12, 0xab]
        );
        assert_eq!(
            decode(&png_finish(
                png_header_with_interlace(1, 1, 16, 6, 1),
                &[0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0]
            ))
            .expect("16-bit Adam7 RGBA")
            .pixels(),
            &[0x12, 0x56, 0x9a, 0xde]
        );
    }

    #[test]
    fn packed_grayscale_palette_and_transparency() {
        for depth in [1, 2, 4, 8] {
            let max = ((1u16 << depth) - 1) as u8;
            let mut h = png_header(2, 1, depth, 0);
            chunk(&mut h, b"tRNS", &[0, max]);
            let scan = if depth == 8 {
                vec![0, 0, max]
            } else {
                vec![0, max << (8 - 2 * depth)]
            };
            assert_eq!(
                decode(&png_finish(h, &scan)).expect("gray").pixels(),
                &[0, 0, 0, 255, 255, 255, 255, 0]
            );
            let mut h = png_header(2, 1, depth, 3);
            chunk(&mut h, b"PLTE", &[255, 0, 0, 0, 255, 0]);
            chunk(&mut h, b"tRNS", &[255, 50]);
            let scan = if depth == 8 {
                vec![0, 0, 1]
            } else {
                vec![0, 1 << (8 - 2 * depth)]
            };
            assert_eq!(
                decode(&png_finish(h, &scan)).expect("palette").pixels(),
                &[255, 0, 0, 255, 0, 255, 0, 50]
            );
        }
        let mut h = png_header(1, 1, 8, 2);
        chunk(&mut h, b"tRNS", &[0, 10, 0, 20, 0, 30]);
        assert_eq!(
            decode(&png_finish(h, &[0, 10, 20, 30]))
                .expect("RGB transparency")
                .pixels(),
            &[10, 20, 30, 0]
        );
    }

    fn frame_control(out: &mut Vec<u8>, seq: u32, area: [u32; 4], disposal: u8, blend: u8) {
        let mut bytes = seq.to_be_bytes().to_vec();
        for v in area {
            bytes.extend_from_slice(&v.to_be_bytes());
        }
        bytes.extend_from_slice(&[0, 1, 0, 10, disposal, blend]);
        chunk(out, b"fcTL", &bytes);
    }

    #[test]
    fn single_frame_apng_retains_animation_metadata() {
        let scanline = [0, 255, 0, 0, 255];
        let still = decode(&png_finish(png_header(1, 1, 8, 6), &scanline)).expect("static PNG");
        assert!(!still.is_animated());
        assert_eq!(still.loop_count(), LoopCount::Finite(1));

        let mut animated = png_header(1, 1, 8, 6);
        chunk(&mut animated, b"acTL", &[0, 0, 0, 1, 0, 0, 0, 0]);
        frame_control(&mut animated, 0, [1, 1, 0, 0], 0, 0);
        let image = decode(&png_finish(animated, &scanline)).expect("single-frame APNG");
        assert!(image.is_animated());
        assert_eq!(image.loop_count(), LoopCount::Infinite);
        assert_eq!(image.frames().len(), 1);
        assert_eq!(image.pixels(), still.pixels());
    }

    #[test]
    fn apng_blending_disposal_and_excluded_default() {
        for excluded in [false, true] {
            let mut out = png_header(2, 1, 8, 6);
            chunk(&mut out, b"acTL", &[0, 0, 0, 4, 0, 0, 0, 2]);
            let compressed =
                miniz_oxide::deflate::compress_to_vec_zlib(&[0, 255, 0, 0, 255, 255, 0, 0, 255], 6);
            if excluded {
                chunk(&mut out, b"IDAT", &compressed);
            }
            frame_control(&mut out, 0, [2, 1, 0, 0], 0, 0);
            let mut seq = 1u32;
            if excluded {
                let mut bytes = seq.to_be_bytes().to_vec();
                bytes.extend_from_slice(&compressed);
                chunk(&mut out, b"fdAT", &bytes);
                seq += 1;
            } else {
                chunk(&mut out, b"IDAT", &compressed);
            }
            frame_control(&mut out, seq, [1, 1, 0, 0], 2, 1);
            seq += 1;
            let mut bytes = seq.to_be_bytes().to_vec();
            bytes.extend_from_slice(&miniz_oxide::deflate::compress_to_vec_zlib(
                &[0, 0, 0, 255, 128],
                6,
            ));
            chunk(&mut out, b"fdAT", &bytes);
            seq += 1;
            frame_control(&mut out, seq, [1, 1, 1, 0], 1, 0);
            seq += 1;
            let mut bytes = seq.to_be_bytes().to_vec();
            bytes.extend_from_slice(&miniz_oxide::deflate::compress_to_vec_zlib(
                &[0, 0, 255, 0, 255],
                6,
            ));
            chunk(&mut out, b"fdAT", &bytes);
            seq += 1;
            frame_control(&mut out, seq, [1, 1, 0, 0], 0, 1);
            seq += 1;
            let mut bytes = seq.to_be_bytes().to_vec();
            bytes.extend_from_slice(&miniz_oxide::deflate::compress_to_vec_zlib(&[0; 5], 6));
            chunk(&mut out, b"fdAT", &bytes);
            chunk(&mut out, b"IEND", &[]);
            let image = decode(&out).expect("APNG");
            assert!(image.is_animated());
            assert_eq!(image.loop_count(), LoopCount::Finite(2));
            assert_eq!(image.frames().len(), 4);
            assert_eq!(image.frames()[3].pixels(), &[255, 0, 0, 255, 0, 0, 0, 0]);
            assert_eq!(
                image.frames()[1].pixels(),
                &[127, 0, 128, 255, 255, 0, 0, 255]
            );
            assert_eq!(
                image.frames()[2].pixels(),
                &[255, 0, 0, 255, 0, 255, 0, 255]
            );
            assert!(
                image
                    .frames()
                    .iter()
                    .all(|f| f.delay() == Duration::from_millis(100))
            );
        }
    }

    #[test]
    fn allocation_limits_and_bad_png_streams() {
        assert_eq!(
            pixel_len(u32::MAX, u32::MAX),
            Err(DecodeError::ImageTooLarge)
        );
        assert_eq!(
            decode(&png_header(100_000, 100_000, 8, 6)),
            Err(DecodeError::ImageTooLarge)
        );
        assert!(decode(&png_finish(png_header(1, 1, 8, 6), &[0; 10000])).is_err());
        assert!(decode(&png_finish(png_header(1, 1, 8, 6), &[9, 0, 0, 0, 0])).is_err());
        let mut budget = Budget::default();
        budget.claim(MAX_BYTES).expect("limit");
        assert_eq!(budget.claim(1), Err(DecodeError::ImageTooLarge));
    }

    #[test]
    fn zlib_trailer_is_required_but_its_checksum_is_not_verified() {
        let mut zlib = miniz_oxide::deflate::compress_to_vec_zlib(&[0, 1, 2, 3, 4], 6);
        let len = zlib.len();
        zlib[len - 1] ^= 0xff;
        let png = |zlib: &[u8]| {
            let mut out = png_header(1, 1, 8, 6);
            chunk(&mut out, b"IDAT", zlib);
            chunk(&mut out, b"IEND", &[]);
            out
        };
        assert_eq!(
            decode(&png(&zlib))
                .expect("chunk CRCs cover the data")
                .pixels(),
            [1, 2, 3, 4]
        );
        assert!(decode(&png(&zlib[..len - 4])).is_err());
    }

    #[test]
    fn paeth_matches_specification() {
        for a in 0..=255u8 {
            for b in 0..=255u8 {
                for c in 0..=255u8 {
                    let p = i16::from(a) + i16::from(b) - i16::from(c);
                    let pa = (p - i16::from(a)).abs();
                    let pb = (p - i16::from(b)).abs();
                    let pc = (p - i16::from(c)).abs();
                    let expected = if pa <= pb && pa <= pc {
                        a
                    } else if pb <= pc {
                        b
                    } else {
                        c
                    };
                    assert_eq!(paeth(a, b, c), expected, "{a} {b} {c}");
                }
            }
        }
    }

    #[test]
    fn png_all_filters_reconstruct_rows() {
        let rows = [
            [10u8, 20, 30, 255, 40, 50, 60, 128],
            [15, 10, 50, 200, 20, 70, 90, 100],
        ];
        for filter in 0..=4 {
            let mut scan = Vec::new();
            for (y, row) in rows.iter().enumerate() {
                scan.push(filter);
                for i in 0..row.len() {
                    let a = if i >= 4 { row[i - 4] } else { 0 };
                    let b = if y > 0 { rows[y - 1][i] } else { 0 };
                    let c = if y > 0 && i >= 4 {
                        rows[y - 1][i - 4]
                    } else {
                        0
                    };
                    let predict = match filter {
                        0 => 0,
                        1 => a,
                        2 => b,
                        3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                        4 => {
                            let p = i32::from(a) + i32::from(b) - i32::from(c);
                            let (_, v) = [
                                (i32::from(a) - p).abs(),
                                (i32::from(b) - p).abs(),
                                (i32::from(c) - p).abs(),
                            ]
                            .into_iter()
                            .zip([a, b, c])
                            .min_by_key(|v| v.0)
                            .expect("three predictors");
                            v
                        }
                        _ => unreachable!(),
                    };
                    scan.push(row[i].wrapping_sub(predict));
                }
            }
            assert_eq!(
                decode(&png_finish(png_header(2, 2, 8, 6), &scan))
                    .expect("filtered PNG")
                    .pixels(),
                rows.concat()
            );
        }
    }

    #[test]
    fn png_16_bit_filters_reconstruct_rows() {
        let rows = [
            [
                0x12u8, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x21, 0x43, 0x65, 0x87, 0xa9,
                0xcb, 0xed, 0x0f,
            ],
            [
                0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc,
                0xfe, 0x10,
            ],
        ];
        for filter in 0..=4 {
            let mut scan = Vec::new();
            for (y, row) in rows.iter().enumerate() {
                scan.push(filter);
                for i in 0..row.len() {
                    let a = if i >= 8 { row[i - 8] } else { 0 };
                    let b = if y > 0 { rows[y - 1][i] } else { 0 };
                    let c = if y > 0 && i >= 8 {
                        rows[y - 1][i - 8]
                    } else {
                        0
                    };
                    let predictor = match filter {
                        0 => 0,
                        1 => a,
                        2 => b,
                        3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                        4 => paeth(a, b, c),
                        _ => unreachable!(),
                    };
                    scan.push(row[i].wrapping_sub(predictor));
                }
            }
            assert_eq!(
                decode(&png_finish(png_header(2, 2, 16, 6), &scan))
                    .expect("filtered 16-bit PNG")
                    .pixels(),
                &[
                    0x12, 0x56, 0x9a, 0xde, 0x21, 0x65, 0xa9, 0xec, 0x23, 0x67, 0xab, 0xee, 0x32,
                    0x76, 0xba, 0xfd,
                ]
            );
        }
    }

    #[test]
    fn scales_16_bit_samples_to_nearest_8_bit_value() {
        assert_eq!(scale_16_to_8(0), 0);
        assert_eq!(scale_16_to_8(129), 1);
        assert_eq!(scale_16_to_8(65_535), 255);
    }

    #[test]
    fn image_rs_16_bit_apng_and_grayscale_alpha() {
        let image =
            decode(include_bytes!("../tests/images/png/apng/rgba16.png")).expect("16-bit APNG");
        assert_eq!(image.frames().len(), 3);
        for (frame, expected) in
            image
                .frames()
                .iter()
                .zip([[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]])
        {
            assert!(
                frame
                    .pixels()
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|pixel| *pixel == expected)
            );
        }
        assert_eq!(
            decode(&png_finish(png_header(2, 1, 8, 4), &[0, 20, 30, 40, 50]))
                .expect("gray alpha")
                .pixels(),
            &[20, 20, 20, 30, 40, 40, 40, 50]
        );
    }

    #[cfg(feature = "png")]
    #[test]
    fn compact_png_filters_and_reduces_channels() {
        let mut pixels = Vec::new();
        for y in 0..32 {
            for x in 0..32 {
                let shade = (x + y) as u8;
                pixels.extend_from_slice(&[shade, shade, shade, 255]);
            }
        }
        let bitmap = Bitmap::new(32, 32, pixels).unwrap();
        let normal = encode(&bitmap, Format::Png).unwrap();
        let small = encode_with_options(&bitmap, Format::Png, smallest()).unwrap();
        assert_eq!(normal[25], 0);
        assert!(small.len() <= normal.len());
        assert_eq!(crate::decode(&normal).unwrap().pixels(), bitmap.data());
        assert_eq!(crate::decode(&small).unwrap().pixels(), bitmap.data());
    }

    #[cfg(feature = "png")]
    #[test]
    fn normal_png_packs_grayscale_depths() {
        for (depth, shades) in [
            (1, &[0, 255][..]),
            (2, &[0, 85, 170, 255][..]),
            (
                4,
                &[
                    0, 17, 34, 51, 68, 85, 102, 119, 136, 153, 170, 187, 204, 221, 238, 255,
                ][..],
            ),
        ] {
            let mut pixels = Vec::new();
            for i in 0..512 {
                let shade = shades[(i * 17 + i / 7) % shades.len()];
                pixels.extend_from_slice(&[shade, shade, shade, 255]);
            }
            let bitmap = Bitmap::new(32, 16, pixels).unwrap();
            for options in [EncodeOptions::default(), smallest()] {
                let encoded = encode_with_options(&bitmap, Format::Png, options).unwrap();
                assert_eq!(encoded[24], depth);
                assert_eq!(encoded[25], 0);
                assert_eq!(crate::decode(&encoded).unwrap().pixels(), bitmap.data());
            }
        }
    }

    #[cfg(feature = "png")]
    #[test]
    fn fixed_png_filters_round_trip() {
        let mut pixels = Vec::new();
        for y in 0..8 {
            for x in 0..8 {
                pixels.extend_from_slice(&[x * 17, y * 31, (x + y) * 9, 255]);
            }
        }
        let bitmap = Bitmap::new(8, 8, pixels).unwrap();
        let row_len = 8 * 4;
        let mut scanlines = Vec::new();
        for row in bitmap.data().chunks_exact(row_len) {
            scanlines.push(0);
            scanlines.extend_from_slice(row);
        }
        for filter in 1..=4 {
            let filtered = png_filtered(&scanlines, row_len, 4, |_, _, _| filter).unwrap();
            let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
            write_chunk(&mut png, b"IHDR", &[0, 0, 0, 8, 0, 0, 0, 8, 8, 6, 0, 0, 0]).unwrap();
            write_chunk(
                &mut png,
                b"IDAT",
                &miniz_oxide::deflate::compress_to_vec_zlib(&filtered, 6),
            )
            .unwrap();
            write_chunk(&mut png, b"IEND", &[]).unwrap();
            assert_eq!(crate::decode(&png).unwrap().pixels(), bitmap.data());
        }
    }

    #[cfg(feature = "png")]
    #[test]
    fn compact_png_uses_indexed_transparency() {
        let colors = [
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 0],
            [255, 255, 0, 128],
        ];
        let mut pixels = Vec::new();
        for y in 0..64 {
            for x in 0..64 {
                pixels.extend_from_slice(&colors[(x * 13 + y * 7 + x * y) % 4]);
            }
        }
        let bitmap = Bitmap::new(64, 64, pixels).unwrap();
        for options in [EncodeOptions::default(), smallest()] {
            let encoded = encode_with_options(&bitmap, Format::Png, options).unwrap();
            assert_eq!(encoded[25], 3);
            assert_eq!(crate::decode(&encoded).unwrap().pixels(), bitmap.data());
        }
    }

    #[cfg(feature = "png")]
    #[test]
    fn max_png_clears_hidden_colors() {
        let mut pixels = Vec::new();
        for i in 0..64 * 64u32 {
            let noise = i.wrapping_mul(2_654_435_761).to_le_bytes();
            pixels.extend_from_slice(&if i % 64 < 32 {
                [noise[1], noise[2], noise[3], 0]
            } else {
                [10, 20, 30, 255]
            });
        }
        let bitmap = Bitmap::new(64, 64, pixels).unwrap();
        let normal = encode(&bitmap, Format::Png).unwrap();
        assert_eq!(crate::decode(&normal).unwrap().pixels(), bitmap.data());
        let small = encode_with_options(&bitmap, Format::Png, smallest()).unwrap();
        assert!(small.len() * 4 < normal.len());
        for (actual, expected) in crate::decode(&small)
            .unwrap()
            .pixels()
            .as_chunks::<4>()
            .0
            .iter()
            .zip(bitmap.data().as_chunks::<4>().0)
        {
            assert_eq!(*actual, if expected[3] == 0 { [0; 4] } else { *expected });
        }
    }

    #[cfg(feature = "png")]
    #[test]
    fn apng_crops_changed_area() {
        let first = [0, 0, 0, 0].repeat(64);
        let mut second = first.clone();
        second[(3 * 8 + 5) * 4..(3 * 8 + 5) * 4 + 4].copy_from_slice(&[20, 30, 40, 255]);
        let frames = [
            Frame::new(Bitmap::new(8, 8, first).unwrap(), Duration::from_millis(50)),
            Frame::new(Bitmap::new(8, 8, second).unwrap(), Duration::from_secs(120)),
        ];
        let normal = encode_animation(Format::Png, &frames, LoopCount::Infinite).unwrap();
        let small =
            encode_animation_with_options(Format::Png, &frames, LoopCount::Infinite, smallest())
                .unwrap();
        assert!(small.len() <= normal.len());
        for encoded in [normal, small] {
            let control = encoded
                .windows(4)
                .enumerate()
                .filter(|(_, bytes)| *bytes == b"fcTL")
                .nth(1)
                .unwrap()
                .0;
            assert_eq!(
                encoded[control + 8..control + 24],
                [0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 5, 0, 0, 0, 3]
            );
            let decoded = crate::decode(&encoded).unwrap();
            for (actual, expected) in decoded.frames().iter().zip(&frames) {
                assert_eq!(actual.pixels(), expected.pixels());
                assert_eq!(actual.delay(), expected.delay());
            }
        }
    }

    #[cfg(feature = "png")]
    #[test]
    fn apng_delay_keeps_representable_nanoseconds() {
        let bitmap = Bitmap::new(1, 1, vec![255, 0, 0, 255]).unwrap();
        let frame = Frame::new(bitmap, Duration::from_micros(500));
        let encoded = encode_animation(Format::Png, &[frame], LoopCount::Infinite).unwrap();
        assert_eq!(
            crate::decode(&encoded).unwrap().frames()[0].delay(),
            Duration::from_micros(500)
        );

        let frame = Frame::new(
            Bitmap::new(1, 1, vec![255, 0, 0, 255]).unwrap(),
            Duration::from_nanos(1),
        );
        assert_eq!(
            encode_animation(Format::Png, &[frame], LoopCount::Infinite),
            Err(EncodeError::InvalidTiming)
        );
    }
}
