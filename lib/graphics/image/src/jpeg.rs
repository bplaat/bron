/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use alloc::vec;
use alloc::vec::Vec;

use super::{
    Bitmap, Budget, DecodeError, EncodeError, EncodingStyle, Format, Image, Reader, Result,
    pixel_len, roundf, truncf,
};

const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

// Codes up to this length decode with a single table lookup.
const LOOKUP_BITS: u32 = 9;

// MARK: Decoder
struct Huffman {
    first: [u32; 17],
    count: [u32; 17],
    offset: [usize; 17],
    values: [u8; 256],
    // Code length in the high byte and symbol in the low byte; zero marks longer codes.
    lookup: [u16; 1 << LOOKUP_BITS],
}

impl Huffman {
    fn read(r: &mut Reader<'_>) -> Result<Self> {
        // Store canonical-code ranges by bit length for direct symbol lookup while decoding.
        let counts = r.take(16)?;
        let mut first = [0; 17];
        let mut count = [0; 17];
        let mut offset = [0; 17];
        let mut code = 0;
        let mut total = 0;
        for len in 1..=16 {
            first[len] = code;
            count[len] = u32::from(counts[len - 1]);
            offset[len] = total;
            total += count[len] as usize;
            code += count[len];
            // JPEG reserves the all-ones code for entropy padding.
            if code >= 1 << len {
                return Err(DecodeError::InvalidData);
            }
            code <<= 1;
        }
        if total == 0 || total > 256 {
            return Err(DecodeError::InvalidData);
        }
        let mut values = [0; 256];
        values[..total].copy_from_slice(r.take(total)?);
        let mut lookup = [0; 1 << LOOKUP_BITS];
        for len in 1..=LOOKUP_BITS as usize {
            let spread = LOOKUP_BITS as usize - len;
            for i in 0..count[len] {
                let entry = (len as u16) << 8 | u16::from(values[offset[len] + i as usize]);
                let start = ((first[len] + i) as usize) << spread;
                lookup[start..start + (1 << spread)].fill(entry);
            }
        }
        Ok(Self {
            first,
            count,
            offset,
            values,
            lookup,
        })
    }

    fn symbol(&self, bits: &mut Bits<'_, '_>) -> Result<u8> {
        if bits.count < 16 {
            bits.fill();
        }
        let entry = self.lookup[(bits.acc >> (64 - LOOKUP_BITS)) as usize];
        let len = u32::from(entry >> 8);
        if entry != 0 && len <= bits.count {
            bits.consume(len);
            return Ok(entry as u8);
        }
        for len in LOOKUP_BITS as usize + 1..=16 {
            if len as u32 > bits.count {
                break;
            }
            let delta = ((bits.acc >> (64 - len)) as u32).wrapping_sub(self.first[len]);
            if delta < self.count[len] {
                bits.consume(len as u32);
                return Ok(self.values[self.offset[len] + delta as usize]);
            }
        }
        Err(DecodeError::InvalidData)
    }
}

/// An MSB-first entropy bit reader that removes byte stuffing and stops before markers.
struct Bits<'a, 'b> {
    r: &'b mut Reader<'a>,
    acc: u64,
    count: u32,
}

impl<'a, 'b> Bits<'a, 'b> {
    const fn new(r: &'b mut Reader<'a>) -> Self {
        Self {
            r,
            acc: 0,
            count: 0,
        }
    }

    fn fill(&mut self) {
        // JPEG escapes literal 0xff entropy bytes with a zero byte.
        while self.count <= 56 {
            let Some(&byte) = self.r.data.get(self.r.pos) else {
                return;
            };
            if byte == 0xff {
                if self.r.data.get(self.r.pos + 1) != Some(&0) {
                    return;
                }
                self.r.pos += 2;
            } else {
                self.r.pos += 1;
            }
            self.acc |= u64::from(byte) << (56 - self.count);
            self.count += 8;
        }
    }

    const fn consume(&mut self, count: u32) {
        self.acc <<= count;
        self.count -= count;
    }

    fn get(&mut self, count: u8) -> Result<u32> {
        let count = u32::from(count);
        if count == 0 {
            return Ok(0);
        }
        if self.count < count {
            self.fill();
            if self.count < count {
                return Err(DecodeError::InvalidData);
            }
        }
        let value = (self.acc >> (64 - count)) as u32;
        self.consume(count);
        Ok(value)
    }

    fn signed(&mut self, size: u8) -> Result<i32> {
        if size == 0 {
            return Ok(0);
        }
        let value = self.get(size)? as i32;
        Ok(if value < 1 << (size - 1) {
            value + 1 - (1 << size)
        } else {
            value
        })
    }

    const fn align(&mut self) -> Result<()> {
        // Only the one-bit padding of the current byte may remain before the next marker.
        let padding = self.count % 8;
        if self.count >= 8 || (padding != 0 && self.acc >> (64 - padding) != (1 << padding) - 1) {
            return Err(DecodeError::InvalidData);
        }
        self.acc = 0;
        self.count = 0;
        Ok(())
    }
}

struct Component {
    id: u8,
    h: usize,
    v: usize,
    quant_id: usize,
    blocks_w: usize,
    blocks_h: usize,
    actual_w: usize,
    actual_h: usize,
    coefficients: Vec<i32>,
    quant: Option<[u16; 64]>,
    approximation: [i8; 64],
}

/// A bilinear tap between two neighboring samples, weighted in 1/256 steps.
#[derive(Clone, Copy)]
struct Tap {
    near: usize,
    far: usize,
    weight: u32,
}

impl Tap {
    // Centers output samples on the component grid, matching libjpeg's fancy upsampling at 2x.
    fn table(len: usize, factor: usize, max: usize, budget: &mut Budget) -> Result<Vec<Self>> {
        budget.claim(len * size_of::<Self>())?;
        let last = (len * factor).div_ceil(max) - 1;
        Ok((0..len)
            .map(|i| {
                let position = ((i as f32 + 0.5) * factor as f32 / max as f32 - 0.5).max(0.0);
                let near = (position as usize).min(last);
                Self {
                    near,
                    far: (near + 1).min(last),
                    weight: roundf((position - truncf(position)) * 256.0) as u32,
                }
            })
            .collect())
    }
}

/// Upsamples one subsampled component plane to full-resolution rows.
struct Upsampler {
    columns: Vec<Tap>,
    rows: Vec<Tap>,
    double: bool,
    blend: Vec<u16>,
    output: Vec<u8>,
}

impl Upsampler {
    fn new(
        c: &Component,
        width: usize,
        height: usize,
        max: (usize, usize),
        budget: &mut Budget,
    ) -> Result<Self> {
        let double = c.h * 2 == max.0;
        Ok(Self {
            columns: if double {
                Vec::new()
            } else {
                Tap::table(width, c.h, max.0, budget)?
            },
            rows: Tap::table(height, c.v, max.1, budget)?,
            double,
            blend: budget.zeroed((width * c.h).div_ceil(max.0))?,
            output: budget.zeroed(width)?,
        })
    }

    fn row(&mut self, plane: &[u8], stride: usize, y: usize) -> &[u8] {
        let Tap { near, far, weight } = self.rows[y];
        let len = self.blend.len();
        let top = &plane[near * stride..][..len];
        let bottom = &plane[far * stride..][..len];
        let weight = weight as u16;
        for ((blend, &top), &bottom) in self.blend.iter_mut().zip(top).zip(bottom) {
            *blend = u16::from(top) * (256 - weight) + u16::from(bottom) * weight;
        }
        if self.double {
            // Output pairs straddle each sample with fixed 3/4 and 1/4 taps; edges repeat.
            let blend = &self.blend;
            let last = len - 1;
            let pair = |i: usize, output: &mut [u8]| {
                let near = u32::from(blend[i]) * 3 + 512;
                output[0] = ((near + u32::from(blend[i.saturating_sub(1)])) >> 10) as u8;
                if let Some(output) = output.get_mut(1) {
                    *output = ((near + u32::from(blend[(i + 1).min(last)])) >> 10) as u8;
                }
            };
            pair(0, &mut self.output[..]);
            let interior = self.output.as_chunks_mut::<2>().0.iter_mut().skip(1);
            for (output, window) in interior.zip(blend.windows(3)) {
                let near = u32::from(window[1]) * 3 + 512;
                output[0] = ((near + u32::from(window[0])) >> 10) as u8;
                output[1] = ((near + u32::from(window[2])) >> 10) as u8;
            }
            if last > 0 {
                pair(last, &mut self.output[last * 2..]);
            }
        } else {
            for (output, tap) in self.output.iter_mut().zip(&self.columns) {
                *output = ((u32::from(self.blend[tap.near]) * (256 - tap.weight)
                    + u32::from(self.blend[tap.far]) * tap.weight
                    + 32768)
                    >> 16) as u8;
            }
        }
        &self.output
    }
}

struct Jpeg {
    width: u32,
    height: u32,
    max_h: usize,
    max_v: usize,
    mcus_w: usize,
    mcus_h: usize,
    progressive: bool,
    components: Vec<Component>,
    quant: [Option<[u16; 64]>; 4],
    dc: [Option<Huffman>; 4],
    ac: [Option<Huffman>; 4],
    restart: usize,
    adobe: Option<u8>,
    jfif: bool,
    orientation: u16,
}

pub(super) fn decode(data: &[u8]) -> Result<Image> {
    Jpeg::decode(data)
}

impl Jpeg {
    const fn new() -> Self {
        Self {
            width: 0,
            height: 0,
            max_h: 0,
            max_v: 0,
            mcus_w: 0,
            mcus_h: 0,
            progressive: false,
            components: Vec::new(),
            quant: [None; 4],
            dc: [None, None, None, None],
            ac: [None, None, None, None],
            restart: 0,
            adobe: None,
            jfif: false,
            orientation: 1,
        }
    }

    fn decode(data: &[u8]) -> Result<Image> {
        let mut r = Reader::new(&data[2..]);
        let mut budget = Budget::default();
        let mut jpeg = Self::new();
        let mut scans = 0;
        loop {
            let m = marker(&mut r)?;
            if m == 0xd9 {
                if scans == 0
                    || jpeg
                        .components
                        .iter()
                        .any(|component| component.approximation[0] < 0)
                {
                    return Err(DecodeError::InvalidData);
                }
                return jpeg.render(&mut budget);
            }
            if matches!(m, 0xd0..=0xd8 | 0x01) {
                return Err(DecodeError::InvalidData);
            }
            let len = r.be16()?;
            if len < 2 {
                return Err(DecodeError::InvalidData);
            }
            let bytes = r.take(len as usize - 2)?;
            let mut segment = Reader::new(bytes);
            match m {
                0xc0..=0xc2 => jpeg.header(&mut segment, m == 0xc2, &mut budget)?,
                0xc4 => {
                    while segment.remaining() > 0 {
                        let id = segment.byte()?;
                        if id & 15 > 3 || id >> 4 > 1 {
                            return Err(DecodeError::InvalidData);
                        }
                        budget.claim(size_of::<Huffman>() + 256)?;
                        let table = Huffman::read(&mut segment)?;
                        if id >> 4 == 0 {
                            jpeg.dc[(id & 15) as usize] = Some(table);
                        } else {
                            jpeg.ac[(id & 15) as usize] = Some(table);
                        }
                    }
                }
                0xdb => {
                    while segment.remaining() > 0 {
                        let id = segment.byte()?;
                        if id & 15 > 3 || id >> 4 > 1 {
                            return Err(DecodeError::InvalidData);
                        }
                        let mut q = [0; 64];
                        for k in ZIGZAG {
                            q[k] = if id >> 4 == 0 {
                                u16::from(segment.byte()?)
                            } else {
                                segment.be16()?
                            };
                            if q[k] == 0 {
                                return Err(DecodeError::InvalidData);
                            }
                        }
                        jpeg.quant[(id & 15) as usize] = Some(q);
                    }
                }
                0xdd => {
                    jpeg.restart = segment.be16()? as usize;
                    if segment.remaining() != 0 {
                        return Err(DecodeError::InvalidData);
                    }
                }
                0xda => {
                    jpeg.scan(&mut segment, &mut r)?;
                    scans += 1;
                }
                0xe0 => {
                    if bytes.starts_with(b"JFIF\0") {
                        jpeg.jfif = true;
                    }
                }
                0xe1 => {
                    if let Some(orientation) = exif_orientation(bytes) {
                        jpeg.orientation = orientation;
                    }
                }
                0xee => {
                    if bytes.starts_with(b"Adobe") && bytes.len() >= 12 {
                        jpeg.adobe = Some(bytes[11]);
                    }
                }
                0xe2..=0xed | 0xef | 0xfe => {}
                _ => return Err(DecodeError::UnsupportedFeature),
            }
        }
    }
}

fn marker(r: &mut Reader<'_>) -> Result<u8> {
    if r.byte()? != 255 {
        return Err(DecodeError::InvalidData);
    }
    loop {
        let byte = r.byte()?;
        if byte == 0 {
            return Err(DecodeError::InvalidData);
        }
        if byte != 255 {
            return Ok(byte);
        }
    }
}

impl Jpeg {
    fn header(&mut self, r: &mut Reader<'_>, progressive: bool, budget: &mut Budget) -> Result<()> {
        if !self.components.is_empty() {
            return Err(DecodeError::InvalidData);
        }
        if r.byte()? != 8 {
            return Err(DecodeError::UnsupportedFeature);
        }
        self.height = u32::from(r.be16()?);
        self.width = u32::from(r.be16()?);
        pixel_len(self.width, self.height)?;
        self.progressive = progressive;
        let count = r.byte()? as usize;
        if !matches!(count, 1 | 3 | 4) {
            return Err(DecodeError::UnsupportedFeature);
        }
        let mut total = 0;
        for _ in 0..count {
            let id = r.byte()?;
            let sample = r.byte()?;
            let h = (sample >> 4) as usize;
            let v = (sample & 15) as usize;
            let q = r.byte()? as usize;
            if !(1..=4).contains(&h)
                || !(1..=4).contains(&v)
                || q > 3
                || self.components.iter().any(|c| c.id == id)
            {
                return Err(DecodeError::InvalidHeader);
            }
            self.max_h = self.max_h.max(h);
            self.max_v = self.max_v.max(v);
            total += h * v;
            self.components.push(Component {
                id,
                h,
                v,
                quant_id: q,
                blocks_w: 0,
                blocks_h: 0,
                actual_w: 0,
                actual_h: 0,
                coefficients: Vec::new(),
                quant: None,
                approximation: [-1; 64],
            });
        }
        if total > 10 || r.remaining() != 0 {
            return Err(DecodeError::InvalidHeader);
        }
        self.mcus_w = (self.width as usize).div_ceil(self.max_h * 8);
        self.mcus_h = (self.height as usize).div_ceil(self.max_v * 8);
        for c in &mut self.components {
            c.blocks_w = self.mcus_w * c.h;
            c.blocks_h = self.mcus_h * c.v;
            c.actual_w = (self.width as usize * c.h).div_ceil(self.max_h * 8);
            c.actual_h = (self.height as usize * c.v).div_ceil(self.max_v * 8);
            c.coefficients = budget.zeroed(c.blocks_w * c.blocks_h * 64)?;
        }
        Ok(())
    }

    fn scan(&mut self, s: &mut Reader<'_>, r: &mut Reader<'_>) -> Result<()> {
        // Sequential scans fill coefficients once; progressive scans transmit and refine them
        // over multiple passes.
        let count = s.byte()? as usize;
        if count == 0 || count > self.components.len() {
            return Err(DecodeError::InvalidData);
        }
        let mut selectors = [(0usize, 0usize, 0usize); 4];
        for n in 0..count {
            let id = s.byte()?;
            let index = self
                .components
                .iter()
                .position(|c| c.id == id)
                .ok_or(DecodeError::InvalidData)?;
            if selectors[..n].iter().any(|sel| sel.0 == index) {
                return Err(DecodeError::InvalidData);
            }
            let tables = s.byte()?;
            if tables >> 4 > 3 || tables & 15 > 3 {
                return Err(DecodeError::InvalidData);
            }
            selectors[n] = (index, (tables >> 4) as usize, (tables & 15) as usize);
        }
        let start = s.byte()? as usize;
        let end = s.byte()? as usize;
        let approx = s.byte()?;
        let high = approx >> 4;
        let low = approx & 15;
        if s.remaining() != 0 || start > end || end > 63 || high > 13 || low > 13 {
            return Err(DecodeError::InvalidData);
        }
        if self.progressive {
            if (start == 0 && end != 0)
                || (start != 0 && count != 1)
                || (high != 0 && high != low + 1)
            {
                return Err(DecodeError::InvalidData);
            }
        } else if start != 0 || end != 63 || approx != 0 {
            return Err(DecodeError::InvalidData);
        }
        for &(index, dc, ac) in &selectors[..count] {
            let c = &mut self.components[index];
            for a in &mut c.approximation[start..=end] {
                if (high == 0 && *a != -1) || (high != 0 && *a != high as i8) {
                    return Err(DecodeError::InvalidData);
                }
                *a = low as i8;
            }
            if c.quant.is_none() {
                c.quant = Some(self.quant[c.quant_id].ok_or(DecodeError::InvalidData)?);
            }
            if (start == 0 && high == 0 && self.dc[dc].is_none())
                || (end != 0 && self.ac[ac].is_none())
            {
                return Err(DecodeError::InvalidData);
            }
        }
        let first = &self.components[selectors[0].0];
        let (cols, rows) = if count == 1 {
            (first.actual_w, first.actual_h)
        } else {
            (self.mcus_w, self.mcus_h)
        };
        let mut bits = Bits::new(r);
        let mut predictors = [0i32; 4];
        let mut eob = 0u32;
        let mut restart_index = 0;
        for mcu in 0..cols * rows {
            if mcu != 0 && self.restart != 0 && mcu % self.restart == 0 {
                if eob != 0 {
                    return Err(DecodeError::InvalidData);
                }
                bits.align()?;
                if marker(bits.r)? != 0xd0 + restart_index {
                    return Err(DecodeError::InvalidData);
                }
                restart_index = (restart_index + 1) & 7;
                predictors.fill(0);
            }
            for &(index, dc, ac) in &selectors[..count] {
                let c = &mut self.components[index];
                let (bh, bv) = if count == 1 { (1, 1) } else { (c.h, c.v) };
                for y in 0..bv {
                    for x in 0..bh {
                        let block_x = (mcu % cols) * bh + x;
                        let block_y = (mcu / cols) * bv + y;
                        let offset = (block_y * c.blocks_w + block_x) * 64;
                        let block = &mut c.coefficients[offset..offset + 64];
                        if !self.progressive {
                            let size = self.dc[dc]
                                .as_ref()
                                .expect("DC table checked")
                                .symbol(&mut bits)?;
                            if size > 11 {
                                return Err(DecodeError::InvalidData);
                            }
                            predictors[index] = predictors[index]
                                .checked_add(bits.signed(size)?)
                                .filter(|v| v.abs() <= 65535)
                                .ok_or(DecodeError::InvalidData)?;
                            block[0] = predictors[index];
                            ac_first(
                                block,
                                &mut bits,
                                self.ac[ac].as_ref().expect("AC table checked"),
                                1,
                                63,
                                0,
                                &mut eob,
                                false,
                            )?;
                        } else if start == 0 {
                            if high == 0 {
                                let size = self.dc[dc]
                                    .as_ref()
                                    .expect("DC table checked")
                                    .symbol(&mut bits)?;
                                if size > 11 {
                                    return Err(DecodeError::InvalidData);
                                }
                                predictors[index] = predictors[index]
                                    .checked_add(bits.signed(size)?)
                                    .filter(|v| v.abs() <= 65535)
                                    .ok_or(DecodeError::InvalidData)?;
                                block[0] = predictors[index] << low;
                            } else {
                                block[0] |= (bits.get(1)? as i32) << low;
                            }
                        } else if high == 0 {
                            ac_first(
                                block,
                                &mut bits,
                                self.ac[ac].as_ref().expect("AC table checked"),
                                start,
                                end,
                                low,
                                &mut eob,
                                true,
                            )?;
                        } else {
                            ac_refine(
                                block,
                                &mut bits,
                                self.ac[ac].as_ref().expect("AC table checked"),
                                start,
                                end,
                                low,
                                &mut eob,
                            )?;
                        }
                    }
                }
            }
        }
        if eob != 0 {
            return Err(DecodeError::InvalidData);
        }
        bits.align()?;
        Ok(())
    }

    fn render(mut self, budget: &mut Budget) -> Result<Image> {
        // Transform component blocks, upsample chroma, convert to RGB, then apply EXIF orientation.
        let mut planes = budget.zeroed::<Vec<u8>>(self.components.len())?;
        for (c, output) in self.components.iter_mut().zip(&mut planes) {
            let stride = c.blocks_w * 8;
            let mut plane = budget.zeroed::<u8>(stride * c.blocks_h * 8)?;
            let table = idct_table(&c.quant.ok_or(DecodeError::InvalidData)?);
            let coefficients = core::mem::take(&mut c.coefficients);
            for by in 0..c.blocks_h {
                for bx in 0..c.blocks_w {
                    let offset = (by * c.blocks_w + bx) * 64;
                    idct(
                        &coefficients[offset..offset + 64],
                        &table,
                        &mut plane[(by * 8 * stride + bx * 8)..],
                        stride,
                    );
                }
            }
            drop(coefficients);
            *output = plane;
        }
        let mut pixels = budget.zeroed(pixel_len(self.width, self.height)?)?;
        let rgb = !self.jfif
            && (self.adobe == Some(0)
                || self
                    .components
                    .iter()
                    .map(|c| c.id)
                    .eq(b"RGB".iter().copied()));
        if self.adobe.is_some_and(|v| v > 2) {
            return Err(DecodeError::UnsupportedFeature);
        }
        let width = self.width as usize;
        let mut upsamplers = Vec::with_capacity(self.components.len());
        for c in &self.components {
            upsamplers.push(if c.h == self.max_h && c.v == self.max_v {
                None
            } else {
                Some(Upsampler::new(
                    c,
                    width,
                    self.height as usize,
                    (self.max_h, self.max_v),
                    budget,
                )?)
            });
        }
        for (y, row) in pixels.chunks_exact_mut(width * 4).enumerate() {
            let mut rows: [&[u8]; 4] = [&[]; 4];
            for (i, (c, upsampler)) in self.components.iter().zip(&mut upsamplers).enumerate() {
                let stride = c.blocks_w * 8;
                rows[i] = match upsampler {
                    Some(upsampler) => upsampler.row(&planes[i], stride, y),
                    None => &planes[i][y * stride..][..width],
                };
            }
            let row = row.as_chunks_mut::<4>().0;
            match self.components.len() {
                1 => {
                    for (p, &l) in row.iter_mut().zip(rows[0]) {
                        *p = [l, l, l, 255];
                    }
                }
                3 => {
                    for (p, ((&a, &b), &c)) in
                        row.iter_mut().zip(rows[0].iter().zip(rows[1]).zip(rows[2]))
                    {
                        let [r, g, b] = if rgb { [a, b, c] } else { ycbcr(a, b, c) };
                        *p = [r, g, b, 255];
                    }
                }
                4 => {
                    for (x, p) in row.iter_mut().enumerate() {
                        let values = [rows[0][x], rows[1][x], rows[2][x], rows[3][x]];
                        let color = if self.adobe == Some(2) {
                            ycbcr(values[0], values[1], values[2]).map(|v| 255 - v)
                        } else if self.adobe.is_some() {
                            [values[0], values[1], values[2]]
                        } else {
                            [255 - values[0], 255 - values[1], 255 - values[2]]
                        };
                        let black = if self.adobe.is_some() {
                            values[3]
                        } else {
                            255 - values[3]
                        };
                        for k in 0..3 {
                            p[k] = ((u16::from(color[k]) * u16::from(black) + 127) / 255) as u8;
                        }
                        p[3] = 255;
                    }
                }
                _ => unreachable!("component count checked"),
            }
        }
        let (width, height, pixels) =
            orient(self.width, self.height, pixels, self.orientation, budget)?;
        Ok(Image::still(Format::Jpeg, width, height, pixels))
    }
}

#[allow(clippy::too_many_arguments)]
fn ac_first(
    block: &mut [i32],
    bits: &mut Bits<'_, '_>,
    table: &Huffman,
    start: usize,
    end: usize,
    low: u8,
    eob: &mut u32,
    progressive: bool,
) -> Result<()> {
    if *eob > 0 {
        *eob -= 1;
        return Ok(());
    }
    let mut k = start;
    while k <= end {
        let value = table.symbol(bits)?;
        let run = (value >> 4) as usize;
        let size = value & 15;
        if size == 0 {
            if run == 15 {
                k += 16;
                if k > end + 1 {
                    return Err(DecodeError::InvalidData);
                }
            } else {
                if !progressive && run != 0 {
                    return Err(DecodeError::InvalidData);
                }
                *eob = (1 << run) + bits.get(run as u8)? - 1;
                break;
            }
        } else {
            k += run;
            if k > end || size > 10 {
                return Err(DecodeError::InvalidData);
            }
            block[ZIGZAG[k]] = bits.signed(size)? << low;
            k += 1;
        }
    }
    Ok(())
}

fn refine(value: &mut i32, bits: &mut Bits<'_, '_>, bit: i32) -> Result<()> {
    if bits.get(1)? != 0 && *value & bit == 0 {
        *value += if *value > 0 { bit } else { -bit };
    }
    Ok(())
}

fn ac_refine(
    block: &mut [i32],
    bits: &mut Bits<'_, '_>,
    table: &Huffman,
    start: usize,
    end: usize,
    low: u8,
    eob: &mut u32,
) -> Result<()> {
    let bit = 1 << low;
    let mut k = start;
    if *eob == 0 {
        while k <= end {
            let symbol = table.symbol(bits)?;
            let mut run = (symbol >> 4) as usize;
            let size = symbol & 15;
            let new = if size == 1 {
                if bits.get(1)? != 0 { bit } else { -bit }
            } else if size == 0 {
                if run < 15 {
                    *eob = (1 << run) + bits.get(run as u8)?;
                    break;
                }
                run = 16;
                0
            } else {
                return Err(DecodeError::InvalidData);
            };
            while k <= end {
                let value = &mut block[ZIGZAG[k]];
                if *value != 0 {
                    refine(value, bits, bit)?;
                } else {
                    if run == 0 {
                        break;
                    }
                    run -= 1;
                    if run == 0 && new == 0 {
                        k += 1;
                        break;
                    }
                }
                k += 1;
            }
            if run != 0 {
                return Err(DecodeError::InvalidData);
            }
            if new != 0 {
                if k > end {
                    return Err(DecodeError::InvalidData);
                }
                block[ZIGZAG[k]] = new;
                k += 1;
            }
        }
    }
    if *eob > 0 {
        for &index in &ZIGZAG[k..=end] {
            if block[index] != 0 {
                refine(&mut block[index], bits, bit)?;
            }
        }
        *eob -= 1;
    }
    Ok(())
}

// AAN scale factors: 1 for k = 0, otherwise cos(k * PI / 16) * sqrt(2).
const AAN_SCALE: [f32; 8] = [
    1.0,
    1.387_039_8,
    1.306_563,
    1.175_875_6,
    1.0,
    0.785_694_96,
    0.541_196_1,
    0.275_899_38,
];

/// Folds the AAN scale factors and the final division by 8 into a dequantization table.
fn idct_table(quant: &[u16; 64]) -> [f32; 64] {
    core::array::from_fn(|i| f32::from(quant[i]) * AAN_SCALE[i / 8] * AAN_SCALE[i % 8] / 8.0)
}

// The AAN 8-point inverse DCT butterfly, as in libjpeg's jidctflt.
fn idct_1d(v: [f32; 8]) -> [f32; 8] {
    use core::f32::consts::SQRT_2;
    let tmp10 = v[0] + v[4];
    let tmp11 = v[0] - v[4];
    let tmp13 = v[2] + v[6];
    let tmp12 = (v[2] - v[6]) * SQRT_2 - tmp13;
    let even = [tmp10 + tmp13, tmp11 + tmp12, tmp11 - tmp12, tmp10 - tmp13];
    let z13 = v[5] + v[3];
    let z10 = v[5] - v[3];
    let z11 = v[1] + v[7];
    let z12 = v[1] - v[7];
    let tmp7 = z11 + z13;
    let tmp11 = (z11 - z13) * SQRT_2;
    let z5 = (z10 + z12) * 1.847_759;
    let tmp10 = 1.082_392_2 * z12 - z5;
    let tmp12 = -2.613_126 * z10 + z5;
    let tmp6 = tmp12 - tmp7;
    let tmp5 = tmp11 - tmp6;
    let tmp4 = tmp10 + tmp5;
    [
        even[0] + tmp7,
        even[1] + tmp6,
        even[2] + tmp5,
        even[3] - tmp4,
        even[3] + tmp4,
        even[2] - tmp5,
        even[1] - tmp6,
        even[0] - tmp7,
    ]
}

fn idct(block: &[i32], table: &[f32; 64], dest: &mut [u8], stride: usize) {
    // Float-to-u8 casts saturate, which clamps every output sample to 0..=255.
    if block[1..].iter().all(|v| *v == 0) {
        let value = roundf(block[0] as f32 * table[0] + 128.0) as u8;
        for row in dest.chunks_mut(stride).take(8) {
            row[..8].fill(value);
        }
        return;
    }
    let mut columns = [[0.0f32; 8]; 8];
    for (x, column) in columns.iter_mut().enumerate() {
        let input = core::array::from_fn(|y| block[y * 8 + x] as f32 * table[y * 8 + x]);
        *column = if input[1..].iter().all(|v| *v == 0.0) {
            [input[0]; 8]
        } else {
            idct_1d(input)
        };
    }
    for (y, dest) in dest.chunks_mut(stride).take(8).enumerate() {
        let row = idct_1d(core::array::from_fn(|x| columns[x][y]));
        for (dest, value) in dest.iter_mut().zip(row) {
            *dest = roundf(value + 128.0) as u8;
        }
    }
}

fn ycbcr(y: u8, cb: u8, cr: u8) -> [u8; 3] {
    let y = i32::from(y) * 65536;
    let cb = i32::from(cb) - 128;
    let cr = i32::from(cr) - 128;
    [
        (y + 91881 * cr + 32768) >> 16,
        (y - 22554 * cb - 46802 * cr + 32768) >> 16,
        (y + 116130 * cb + 32768) >> 16,
    ]
    .map(|v| v.clamp(0, 255) as u8)
}

fn exif_orientation(bytes: &[u8]) -> Option<u16> {
    let data = bytes.strip_prefix(b"Exif\0\0")?;
    let little = match data.get(..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let short = |p: usize| -> Option<u16> {
        let a = data.get(p..p.checked_add(2)?)?.try_into().ok()?;
        Some(if little {
            u16::from_le_bytes(a)
        } else {
            u16::from_be_bytes(a)
        })
    };
    let long = |p: usize| -> Option<u32> {
        let a = data.get(p..p.checked_add(4)?)?.try_into().ok()?;
        Some(if little {
            u32::from_le_bytes(a)
        } else {
            u32::from_be_bytes(a)
        })
    };
    if short(2)? != 42 {
        return None;
    }
    let ifd = long(4)? as usize;
    let count = short(ifd)? as usize;
    for i in 0..count {
        let offset = ifd.checked_add(2 + i * 12)?;
        if short(offset)? == 0x112 && short(offset + 2)? == 3 && long(offset + 4)? == 1 {
            return short(offset + 8).filter(|v| (1..=8).contains(v));
        }
    }
    None
}

fn orient(
    width: u32,
    height: u32,
    mut pixels: Vec<u8>,
    orientation: u16,
    budget: &mut Budget,
) -> Result<(u32, u32, Vec<u8>)> {
    if orientation == 1 {
        return Ok((width, height, pixels));
    }
    if (2..=4).contains(&orientation) {
        let pixel_chunks = pixels.as_chunks_mut::<4>().0;
        let row_width = width as usize;
        let row_height = height as usize;
        match orientation {
            2 => {
                for row in pixel_chunks.chunks_exact_mut(row_width) {
                    row.reverse();
                }
            }
            3 => pixel_chunks.reverse(),
            4 => {
                for y in 0..row_height / 2 {
                    for x in 0..row_width {
                        pixel_chunks.swap(y * row_width + x, (row_height - 1 - y) * row_width + x);
                    }
                }
            }
            _ => unreachable!(),
        }
        return Ok((width, height, pixels));
    }
    let (w, h) = (height, width);
    let mut output = budget.zeroed(pixels.len())?;
    for y in 0..height {
        for x in 0..width {
            let (dx, dy) = match orientation {
                5 => (y, x),
                6 => (height - 1 - y, x),
                7 => (height - 1 - y, width - 1 - x),
                8 => (y, width - 1 - x),
                _ => unreachable!("orientation was checked"),
            };
            let src = (y as usize * width as usize + x as usize) * 4;
            let dst = (dy as usize * w as usize + dx as usize) * 4;
            output[dst..dst + 4].copy_from_slice(&pixels[src..src + 4]);
        }
    }
    Ok((w, h, output))
}

// MARK: Encoder
const JPEG_LUMA_QUANT: [u8; 64] = [
    16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55, 14, 13, 16, 24, 40, 57, 69, 56,
    14, 17, 22, 29, 51, 87, 80, 62, 18, 22, 37, 56, 68, 109, 103, 77, 24, 35, 55, 64, 81, 104, 113,
    92, 49, 64, 78, 87, 103, 121, 120, 101, 72, 92, 95, 98, 112, 100, 103, 99,
];
const JPEG_CHROMA_QUANT: [u8; 64] = [
    17, 18, 24, 47, 99, 99, 99, 99, 18, 21, 26, 66, 99, 99, 99, 99, 24, 26, 56, 99, 99, 99, 99, 99,
    47, 66, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
];
struct JpegBits<'a> {
    data: &'a mut Vec<u8>,
    bits: u64,
    count: u8,
}

impl<'a> JpegBits<'a> {
    const fn new(data: &'a mut Vec<u8>) -> Self {
        Self {
            data,
            bits: 0,
            count: 0,
        }
    }

    fn write(&mut self, value: u32, count: u8) {
        self.bits = (self.bits << count) | u64::from(value);
        self.count += count;
        if self.count >= 32 {
            self.count -= 32;
            let bytes = ((self.bits >> self.count) as u32).to_be_bytes();
            // Most words contain no 0xff byte, so they skip the byte stuffing loop.
            if bytes.contains(&0xff) {
                self.push_stuffed(&bytes);
            } else {
                self.data.extend_from_slice(&bytes);
            }
        }
    }

    fn push_stuffed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.data.push(byte);
            if byte == 0xff {
                self.data.push(0);
            }
        }
    }

    fn finish(&mut self) {
        let padding = (8 - self.count % 8) % 8;
        self.write((1 << padding) - 1, padding);
        if self.count != 0 {
            let bytes = (self.bits << (64 - self.count)).to_be_bytes();
            self.push_stuffed(&bytes[..usize::from(self.count / 8)]);
            self.count = 0;
        }
    }
}

fn jpeg_huffman(lengths: &[u8; 16], values: &[u8]) -> [(u16, u8); 256] {
    let mut table = [(0, 0); 256];
    let mut code = 0u16;
    let mut offset = 0;
    for (index, &count) in lengths.iter().enumerate() {
        for &symbol in &values[offset..offset + usize::from(count)] {
            table[usize::from(symbol)] = (code, (index + 1) as u8);
            code += 1;
        }
        offset += usize::from(count);
        code <<= 1;
    }
    table
}

fn jpeg_segment(out: &mut Vec<u8>, marker: u8, data: &[u8]) {
    out.extend_from_slice(&[0xff, marker]);
    out.extend_from_slice(&((data.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(data);
}

fn jpeg_quant(base: &[u8; 64], quality: u8) -> [u8; 64] {
    let scale = if quality < 50 {
        5000 / u32::from(quality)
    } else {
        200 - 2 * u32::from(quality)
    };
    base.map(|value| ((u32::from(value) * scale + 50) / 100).clamp(1, 255) as u8)
}

struct JpegPlanes {
    width: usize,
    height: usize,
    stride: usize,
    rows: usize,
    y: Vec<u8>,
    cb: Vec<u8>,
    cr: Vec<u8>,
}

impl JpegPlanes {
    // Converts once to fixed-point YCbCr planes padded to whole MCUs, so blocks need no bounds
    // clamping and every sampling layout shares the conversion.
    fn new(bitmap: &Bitmap, gray: bool) -> Self {
        let (width, height) = (bitmap.width as usize, bitmap.height as usize);
        let (stride, rows) = (width.next_multiple_of(16), height.next_multiple_of(16));
        let plane = |enabled: bool| vec![0; if enabled { stride * rows } else { 0 }];
        let (mut y, mut cb, mut cr) = (plane(true), plane(!gray), plane(!gray));
        for (index, row) in bitmap.data.chunks_exact(width * 4).enumerate() {
            let pixels = row.as_chunks::<4>().0;
            let lines = index * stride..(index + 1) * stride;
            if gray {
                // Gray pixels have equal channels, so luma is the red channel.
                for (luma, pixel) in y[lines.clone()].iter_mut().zip(pixels) {
                    *luma = pixel[0];
                }
            } else {
                for (((luma, blue), red), pixel) in y[lines.clone()]
                    .iter_mut()
                    .zip(&mut cb[lines.clone()])
                    .zip(&mut cr[lines.clone()])
                    .zip(pixels)
                {
                    let [r, g, b] = [0, 1, 2].map(|channel| i32::from(pixel[channel]));
                    *luma = ((19595 * r + 38470 * g + 7471 * b + 32768) >> 16) as u8;
                    *blue =
                        ((-11059 * r - 21709 * g + 32768 * b + (128 << 16) + 32767) >> 16) as u8;
                    *red = ((32768 * r - 27439 * g - 5329 * b + (128 << 16) + 32767) >> 16) as u8;
                }
            }
            for plane in [&mut y, &mut cb, &mut cr] {
                if let Some(line) = plane.get_mut(lines.clone()) {
                    let last = line[width - 1];
                    line[width..].fill(last);
                }
            }
        }
        for plane in [&mut y, &mut cb, &mut cr] {
            if !plane.is_empty() {
                let last = (height - 1) * stride;
                for row in height..rows {
                    plane.copy_within(last..last + stride, row * stride);
                }
            }
        }
        Self {
            width,
            height,
            stride,
            rows,
            y,
            cb,
            cr,
        }
    }

    fn downsample<const H: usize, const V: usize>(&self) -> (Vec<u8>, Vec<u8>) {
        let (count, stride) = ((H * V) as u16, self.stride / H);
        let mut sums = vec![0u16; stride];
        [&self.cb, &self.cr]
            .map(|plane| {
                let mut out = Vec::with_capacity(stride * (self.rows / V));
                for lines in plane.chunks_exact(self.stride * V) {
                    sums.fill(count / 2);
                    for line in lines.chunks_exact(self.stride) {
                        for (sum, samples) in sums.iter_mut().zip(line.as_chunks::<H>().0) {
                            *sum += samples.iter().map(|&value| u16::from(value)).sum::<u16>();
                        }
                    }
                    out.extend(sums.iter().map(|&sum| (sum / count) as u8));
                }
                out
            })
            .into()
    }

    fn blocks(&self, h: usize, v: usize, tables: &JpegTables) -> Vec<[i16; 64]> {
        let gray = self.cb.is_empty();
        let chroma = match (h, v) {
            _ if gray => None,
            (2, 2) => Some(self.downsample::<2, 2>()),
            (2, 1) => Some(self.downsample::<2, 1>()),
            (1, 2) => Some(self.downsample::<1, 2>()),
            _ => None,
        };
        let (cb, cr) = chroma
            .as_ref()
            .map_or((&self.cb[..], &self.cr[..]), |(cb, cr)| (&cb[..], &cr[..]));
        let per_mcu = if gray { 1 } else { h * v + 2 };
        let mcus = self.width.div_ceil(h * 8) * self.height.div_ceil(v * 8);
        let mut blocks = Vec::with_capacity(mcus * per_mcu);
        for y in (0..self.height).step_by(v * 8) {
            for x in (0..self.width).step_by(h * 8) {
                for by in 0..v {
                    for bx in 0..h {
                        let block = &self.y[(y + by * 8) * self.stride + x + bx * 8..];
                        blocks.push(jpeg_fdct(block, self.stride, &tables.luma));
                    }
                }
                if !gray {
                    let stride = self.stride / h;
                    for plane in [cb, cr] {
                        let block = &plane[y / v * stride + x / h..];
                        blocks.push(jpeg_fdct(block, stride, &tables.chroma));
                    }
                }
            }
        }
        blocks
    }
}

// The Arai-Agui-Nakajima DCT needs 5 multiplications; its output scale is folded into the
// quantizer reciprocals.
fn jpeg_aan(d: [f32; 8]) -> [f32; 8] {
    let (tmp0, tmp7) = (d[0] + d[7], d[0] - d[7]);
    let (tmp1, tmp6) = (d[1] + d[6], d[1] - d[6]);
    let (tmp2, tmp5) = (d[2] + d[5], d[2] - d[5]);
    let (tmp3, tmp4) = (d[3] + d[4], d[3] - d[4]);
    let (tmp10, tmp13) = (tmp0 + tmp3, tmp0 - tmp3);
    let (tmp11, tmp12) = (tmp1 + tmp2, tmp1 - tmp2);
    let z1 = (tmp12 + tmp13) * core::f32::consts::FRAC_1_SQRT_2;
    let (odd10, odd11, odd12) = (tmp4 + tmp5, tmp5 + tmp6, tmp6 + tmp7);
    let z5 = (odd10 - odd12) * 0.382_683_43;
    let z2 = odd10 * 0.541_196_1 + z5;
    let z4 = odd12 * 1.306_563 + z5;
    let z3 = odd11 * core::f32::consts::FRAC_1_SQRT_2;
    let (z11, z13) = (tmp7 + z3, tmp7 - z3);
    [
        tmp10 + tmp11,
        z11 + z4,
        tmp13 + z1,
        z13 - z2,
        tmp10 - tmp11,
        z13 + z2,
        tmp13 - z1,
        z11 - z4,
    ]
}

fn jpeg_fdct(plane: &[u8], stride: usize, scale: &[f32; 64]) -> [i16; 64] {
    let mut data = [[0.0f32; 8]; 8];
    for (y, row) in data.iter_mut().enumerate() {
        let samples = &plane[y * stride..y * stride + 8];
        *row = jpeg_aan(core::array::from_fn(|x| f32::from(samples[x]) - 128.0));
    }
    for x in 0..8 {
        let column = jpeg_aan(core::array::from_fn(|y| data[y][x]));
        for (row, value) in data.iter_mut().zip(column) {
            row[x] = value;
        }
    }
    core::array::from_fn(|index| roundf(data[index / 8][index % 8] * scale[index]) as i16)
}

fn jpeg_amplitude(value: i16) -> (u8, u16) {
    if value == 0 {
        return (0, 0);
    }
    let magnitude = value.unsigned_abs();
    let size = 16 - magnitude.leading_zeros() as u8;
    let bits = if value < 0 {
        (i32::from(value) - 1) as u16 & ((1 << size) - 1)
    } else {
        value as u16
    };
    (size, bits)
}

const JPEG_UNZIGZAG: [u8; 64] = {
    let mut table = [0; 64];
    let mut index = 0;
    while index < 64 {
        table[ZIGZAG[index]] = index as u8;
        index += 1;
    }
    table
};

// Nonzero AC coefficients become a zigzag-ordered bit mask, so zero runs need no scanning.
fn jpeg_block_symbols(
    block: &[i16; 64],
    previous: &mut i16,
    mut emit: impl FnMut(bool, usize, u16, u8),
) {
    let difference = block[0] - *previous;
    *previous = block[0];
    let (size, value) = jpeg_amplitude(difference.clamp(-2047, 2047));
    emit(true, size as usize, value, size);
    let mut natural = block
        .iter()
        .enumerate()
        .fold(0u64, |mask, (index, &value)| {
            mask | u64::from(value != 0) << index
        })
        & !1;
    let mut mask = 0u64;
    while natural != 0 {
        mask |= 1 << JPEG_UNZIGZAG[natural.trailing_zeros() as usize];
        natural &= natural - 1;
    }
    let mut last = 0;
    while mask != 0 {
        let index = mask.trailing_zeros() as usize;
        mask &= mask - 1;
        let mut zeros = index - last - 1;
        last = index;
        while zeros >= 16 {
            emit(false, 0xf0, 0, 0);
            zeros -= 16;
        }
        let (size, amplitude) = jpeg_amplitude(block[ZIGZAG[index]].clamp(-1023, 1023));
        emit(false, (zeros << 4) | usize::from(size), amplitude, size);
    }
    if last != 63 {
        emit(false, 0, 0, 0);
    }
}

struct JpegOptimizedHuffman {
    lengths: [u8; 16],
    values: Vec<u8>,
    codes: [(u16, u8); 256],
}

fn jpeg_least_frequency(frequencies: &[u32; 257], exclude: usize) -> Option<usize> {
    frequencies
        .iter()
        .enumerate()
        .filter(|&(index, &count)| index != exclude && count != 0)
        .min_by_key(|&(index, &count)| (count, usize::MAX - index))
        .map(|(index, _)| index)
}

fn jpeg_optimized_huffman(mut frequencies: [u32; 257]) -> JpegOptimizedHuffman {
    frequencies[256] = 1;
    let mut links = [-1i16; 257];
    let mut sizes = [0u8; 257];
    while let Some(first) = jpeg_least_frequency(&frequencies, usize::MAX) {
        let Some(second) = jpeg_least_frequency(&frequencies, first) else {
            break;
        };
        frequencies[first] += frequencies[second];
        frequencies[second] = 0;
        for root in [first, second] {
            let mut node = root;
            loop {
                sizes[node] += 1;
                if links[node] < 0 {
                    break;
                }
                node = links[node] as usize;
            }
            if root == first {
                links[node] = second as i16;
            }
        }
    }

    let mut counts = [0u16; 257];
    for &size in &sizes {
        if size != 0 {
            counts[size as usize] += 1;
        }
    }
    for size in (17..counts.len()).rev() {
        while counts[size] != 0 {
            let shorter = (1..size - 1)
                .rev()
                .find(|&index| counts[index] != 0)
                .expect("Huffman tree has a shorter code");
            counts[size] -= 2;
            counts[size - 1] += 1;
            counts[shorter + 1] += 2;
            counts[shorter] -= 1;
        }
    }
    let longest = (1..=16)
        .rev()
        .find(|&index| counts[index] != 0)
        .expect("Huffman tree has a code");
    counts[longest] -= 1; // Remove the pseudo-symbol reserved to avoid all-ones codes.

    let lengths = core::array::from_fn(|index| counts[index + 1] as u8);
    let mut values = Vec::new();
    for size in 1..sizes.len() {
        for (symbol, &actual) in sizes[..256].iter().enumerate() {
            if actual as usize == size {
                values.push(symbol as u8);
            }
        }
    }
    let codes = jpeg_huffman(&lengths, &values);
    JpegOptimizedHuffman {
        lengths,
        values,
        codes,
    }
}

pub(super) fn encode(
    bitmap: &Bitmap,
    quality: u8,
    style: EncodingStyle,
) -> core::result::Result<Vec<u8>, EncodeError> {
    if !(1..=100).contains(&quality) {
        return Err(EncodeError::InvalidQuality);
    }
    let width = u16::try_from(bitmap.width).map_err(|_| EncodeError::InvalidDimensions)?;
    let height = u16::try_from(bitmap.height).map_err(|_| EncodeError::InvalidDimensions)?;
    let pixels = bitmap.data.as_chunks::<4>().0;
    if pixels.iter().any(|pixel| pixel[3] != 255) {
        return Err(EncodeError::AlphaUnsupported);
    }
    let gray = pixels
        .iter()
        .all(|pixel| pixel[0] == pixel[1] && pixel[1] == pixel[2]);
    let max = style == EncodingStyle::MaxCompression;
    let samplings: &[(usize, usize)] = match (gray, max) {
        (true, _) => &[(1, 1)],
        (false, false) => &[(2, 2)],
        (false, true) => &[(2, 2), (2, 1), (1, 2), (1, 1)],
    };
    let tables = JpegTables::new(quality);
    let planes = JpegPlanes::new(bitmap, gray);
    let mut best: Option<Vec<u8>> = None;
    for &(h, v) in samplings {
        let blocks = planes.blocks(h, v, &tables);
        let candidate = jpeg_write(width, height, gray, (h, v), &tables, &blocks);
        if best
            .as_ref()
            .is_none_or(|best| candidate.len() < best.len())
        {
            best = Some(candidate);
        }
    }
    Ok(best.expect("JPEG has a sampling layout"))
}

struct JpegTables {
    quant: [[u8; 64]; 2],
    luma: [f32; 64],
    chroma: [f32; 64],
}

impl JpegTables {
    fn new(quality: u8) -> Self {
        let quant = [
            jpeg_quant(&JPEG_LUMA_QUANT, quality),
            jpeg_quant(&JPEG_CHROMA_QUANT, quality),
        ];
        const AAN: [f32; 8] = [
            1.0,
            1.387_039_8,
            1.306_563,
            1.175_875_6,
            1.0,
            0.785_694_96,
            0.541_196_1,
            0.275_899_38,
        ];
        let reciprocals = |quant: &[u8; 64]| {
            core::array::from_fn(|index| {
                1.0 / (f32::from(quant[index]) * AAN[index / 8] * AAN[index % 8] * 8.0)
            })
        };
        Self {
            luma: reciprocals(&quant[0]),
            chroma: reciprocals(&quant[1]),
            quant,
        }
    }
}

// Visits each symbol with its Huffman table index: luma DC, chroma DC, luma AC, chroma AC.
fn jpeg_symbols(
    blocks: &[[i16; 64]],
    gray: bool,
    (h, v): (usize, usize),
    mut emit: impl FnMut(usize, usize, u16, u8),
) {
    let per_mcu = if gray { 1 } else { h * v + 2 };
    let mut previous = [0i16; 3];
    for (index, block) in blocks.iter().enumerate() {
        let component = (index % per_mcu).saturating_sub(h * v - 1);
        let chroma = usize::from(component != 0);
        jpeg_block_symbols(
            block,
            &mut previous[component],
            |is_dc, symbol, bits, size| {
                emit(if is_dc { chroma } else { 2 + chroma }, symbol, bits, size);
            },
        );
    }
}

fn jpeg_optimized_tables(
    blocks: &[[i16; 64]],
    gray: bool,
    sampling: (usize, usize),
) -> [JpegOptimizedHuffman; 4] {
    let mut frequencies = [[0u32; 257]; 4];
    jpeg_symbols(blocks, gray, sampling, |table, symbol, _, _| {
        frequencies[table][symbol] += 1;
    });
    core::array::from_fn(|index| {
        if frequencies[index].iter().all(|&count| count == 0) {
            frequencies[index][0] = 1;
        }
        jpeg_optimized_huffman(frequencies[index])
    })
}

fn jpeg_write(
    width: u16,
    height: u16,
    gray: bool,
    (h, v): (usize, usize),
    tables: &JpegTables,
    blocks: &[[i16; 64]],
) -> Vec<u8> {
    // Tables built for the image are never larger than the Annex K examples: they
    // give frequent symbols short codes and list only the symbols that occur.
    let huffman = jpeg_optimized_tables(blocks, gray, (h, v));
    let components = if gray { 1 } else { 3 };
    let mut out = Vec::with_capacity((blocks.len() * 16 + 1024).min(1024 * 1024));
    out.extend_from_slice(&[0xff, 0xd8]);
    jpeg_segment(&mut out, 0xe0, b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0");
    let mut dqt = [0u8; 130];
    for (index, table) in tables
        .quant
        .iter()
        .take(if gray { 1 } else { 2 })
        .enumerate()
    {
        let start = index * 65;
        dqt[start] = index as u8;
        for (i, &natural) in ZIGZAG.iter().enumerate() {
            dqt[start + i + 1] = table[natural];
        }
    }
    jpeg_segment(&mut out, 0xdb, &dqt[..if gray { 65 } else { 130 }]);
    let mut sof = [0u8; 15];
    sof[0] = 8;
    sof[1..3].copy_from_slice(&height.to_be_bytes());
    sof[3..5].copy_from_slice(&width.to_be_bytes());
    sof[5] = components;
    for component in 0..components {
        sof[6 + usize::from(component) * 3..][..3].copy_from_slice(&[
            component + 1,
            if component == 0 {
                (h as u8) << 4 | v as u8
            } else {
                0x11
            },
            u8::from(component != 0),
        ]);
    }
    jpeg_segment(&mut out, 0xc0, &sof[..6 + usize::from(components) * 3]);
    for (index, (id, table)) in [0x00, 0x01, 0x10, 0x11]
        .into_iter()
        .zip(&huffman)
        .enumerate()
    {
        if gray && index % 2 == 1 {
            continue;
        }
        let (lengths, values) = (&table.lengths, &table.values[..]);
        let mut dht = [0u8; 179];
        dht[0] = id;
        dht[1..17].copy_from_slice(lengths);
        dht[17..17 + values.len()].copy_from_slice(values);
        jpeg_segment(&mut out, 0xc4, &dht[..17 + values.len()]);
    }
    let mut sos = [0u8; 10];
    sos[0] = components;
    for component in 0..components {
        sos[1 + usize::from(component) * 2..][..2]
            .copy_from_slice(&[component + 1, if component == 0 { 0 } else { 0x11 }]);
    }
    let sos_tail = 1 + usize::from(components) * 2;
    sos[sos_tail..sos_tail + 3].copy_from_slice(&[0, 63, 0]);
    jpeg_segment(&mut out, 0xda, &sos[..sos_tail + 3]);
    let mut bits = JpegBits::new(&mut out);
    jpeg_symbols(blocks, gray, (h, v), |table, symbol, amplitude, size| {
        let (code, count) = huffman[table].codes[symbol];
        bits.write(u32::from(code) << size | u32::from(amplitude), count + size);
    });
    bits.finish();
    out.extend_from_slice(&[0xff, 0xd9]);
    out
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EncodeOptions, Format, encode, encode_with_options};

    fn smallest() -> EncodeOptions {
        EncodeOptions {
            style: EncodingStyle::MaxCompression,
            ..EncodeOptions::default()
        }
    }

    #[test]
    fn jpeg_simple_orientations_reuse_pixel_buffer() {
        for orientation in 2..=4 {
            let pixels = (0..16).collect::<Vec<u8>>();
            let original = pixels.as_ptr();
            let mut budget = Budget::default();
            let (width, height, result) =
                orient(2, 2, pixels, orientation, &mut budget).expect("orientation");
            assert_eq!((width, height), (2, 2));
            assert_eq!(result.as_ptr(), original);
            assert_eq!(budget.used, 0);
        }
    }

    #[test]
    fn huffman_decodes_lookup_and_long_codes_until_a_marker() {
        // One code per length: symbol `s` is `s` one bits followed by a zero bit.
        let mut table = vec![1u8; 16];
        table.extend(0..16u8);
        let huffman = Huffman::read(&mut Reader::new(&table)).expect("Huffman table");
        let symbols = [15u8, 0, 9, 8, 10, 3, 14, 1];
        let mut stream = Vec::new();
        for &symbol in &symbols {
            stream.extend(core::iter::repeat_n(1, symbol as usize));
            stream.push(0);
        }
        stream.resize(stream.len().next_multiple_of(8), 1);
        let mut data = Vec::new();
        for byte in stream.chunks(8) {
            let byte = byte.iter().fold(0u8, |byte, bit| byte << 1 | bit);
            data.push(byte);
            if byte == 0xff {
                data.push(0);
            }
        }
        data.extend_from_slice(&[0xff, 0xd9]);
        let mut r = Reader::new(&data);
        let mut bits = Bits::new(&mut r);
        for symbol in symbols {
            assert_eq!(huffman.symbol(&mut bits), Ok(symbol));
        }
        assert_eq!(bits.align(), Ok(()));
        assert_eq!(r.remaining(), 2);
        let mut r = Reader::new(&data[data.len() - 2..]);
        assert!(huffman.symbol(&mut Bits::new(&mut r)).is_err());
    }

    #[test]
    fn idct_matches_reference_transform() {
        let mut state = 0x1234_5678u32;
        let mut quant = [0u16; 64];
        for q in &mut quant {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *q = (state >> 24) as u16 % 16 + 1;
        }
        for _ in 0..64 {
            let mut block = [0i32; 64];
            for value in &mut block {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *value = (state >> 25) as i32 % 32 - 16;
            }
            let mut actual = [0u8; 64];
            idct(&block, &idct_table(&quant), &mut actual, 8);
            for (i, actual) in actual.into_iter().enumerate() {
                let (x, y) = ((i % 8) as f64, (i / 8) as f64);
                let mut sum = 0.0;
                for (k, &value) in block.iter().enumerate() {
                    let (u, v) = ((k % 8) as f64, (k / 8) as f64);
                    let scale = |n: f64| {
                        if n == 0.0 {
                            core::f64::consts::FRAC_1_SQRT_2
                        } else {
                            1.0
                        }
                    };
                    sum += scale(u)
                        * scale(v)
                        * f64::from(value * i32::from(quant[k]))
                        * ((2.0 * x + 1.0) * u * core::f64::consts::PI / 16.0).cos()
                        * ((2.0 * y + 1.0) * v * core::f64::consts::PI / 16.0).cos();
                }
                let expected = (sum / 4.0 + 128.0).round().clamp(0.0, 255.0) as u8;
                assert!(actual.abs_diff(expected) <= 1, "{actual} vs {expected}");
            }
        }
    }

    #[test]
    fn jpeg_applies_all_exif_orientations() {
        let jpeg = include_bytes!("../tests/images/jpeg/progressive/test.jpg");
        let base = decode(jpeg).expect("base JPEG");
        let base_width = base.width();
        let base_height = base.height();
        for little in [false, true] {
            for orientation in 1u16..=8 {
                let short = |v: u16| {
                    if little {
                        v.to_le_bytes()
                    } else {
                        v.to_be_bytes()
                    }
                };
                let long = |v: u32| {
                    if little {
                        v.to_le_bytes()
                    } else {
                        v.to_be_bytes()
                    }
                };
                let mut exif = b"Exif\0\0".to_vec();
                exif.extend_from_slice(if little { b"II" } else { b"MM" });
                exif.extend_from_slice(&short(42));
                exif.extend_from_slice(&long(8));
                exif.extend_from_slice(&short(1));
                exif.extend_from_slice(&short(0x112));
                exif.extend_from_slice(&short(3));
                exif.extend_from_slice(&long(1));
                exif.extend_from_slice(&short(orientation));
                exif.extend_from_slice(&short(0));
                exif.extend_from_slice(&long(0));
                let mut data = vec![255, 216, 255, 225];
                data.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
                data.extend_from_slice(&exif);
                data.extend_from_slice(&jpeg[2..]);
                let image = decode(&data).expect("oriented JPEG");
                let (w, h) = if orientation >= 5 {
                    (base_height, base_width)
                } else {
                    (base_width, base_height)
                };
                assert_eq!((image.width(), image.height()), (w, h));
                for y in 0..h {
                    for x in 0..w {
                        let (sx, sy) = match orientation {
                            1 => (x, y),
                            2 => (base_width - 1 - x, y),
                            3 => (base_width - 1 - x, base_height - 1 - y),
                            4 => (x, base_height - 1 - y),
                            5 => (y, x),
                            6 => (y, base_height - 1 - x),
                            7 => (base_width - 1 - y, base_height - 1 - x),
                            8 => (base_width - 1 - y, x),
                            _ => unreachable!(),
                        };
                        let src = ((sy * base_width + sx) * 4) as usize;
                        let dst = ((y * w + x) * 4) as usize;
                        assert_eq!(&image.pixels()[dst..dst + 4], &base.pixels()[src..src + 4]);
                    }
                }
            }
        }
    }

    #[cfg(feature = "jpeg")]
    #[test]
    fn jpeg_rejects_alpha() {
        let bitmap = Bitmap::new(1, 1, vec![255, 0, 0, 128]).unwrap();
        assert_eq!(
            encode(&bitmap, Format::Jpeg),
            Err(EncodeError::AlphaUnsupported)
        );
    }

    #[cfg(feature = "jpeg")]
    #[test]
    fn jpeg_encodes_opaque_pixels() {
        let bitmap = Bitmap::new(8, 8, [64, 128, 192, 255].repeat(64)).unwrap();
        let decoded = crate::decode(&encode(&bitmap, Format::Jpeg).unwrap()).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (8, 8));
        for pixel in decoded.pixels().as_chunks::<4>().0 {
            assert!(pixel[0].abs_diff(64) < 8);
            assert!(pixel[1].abs_diff(128) < 8);
            assert!(pixel[2].abs_diff(192) < 8);
            assert_eq!(pixel[3], 255);
        }

        let mut pixels = Vec::new();
        for y in 0..24u8 {
            for x in 0..37u8 {
                pixels.extend_from_slice(&[x * 6, y * 10, 255 - x * 3 - y * 4, 255]);
            }
        }
        let bitmap = Bitmap::new(37, 24, pixels).unwrap();
        for options in [EncodeOptions::default(), smallest()] {
            let encoded = encode_with_options(&bitmap, Format::Jpeg, options).unwrap();
            let decoded = crate::decode(&encoded).unwrap();
            let error = decoded
                .pixels()
                .iter()
                .zip(bitmap.data())
                .map(|(&actual, &expected)| u32::from(actual.abs_diff(expected)))
                .sum::<u32>();
            assert!(error < bitmap.data().len() as u32 * 2, "{error}");
        }
    }

    #[cfg(feature = "jpeg")]
    #[test]
    fn compact_jpeg_preserves_quality_setting() {
        let bitmap = Bitmap::new(32, 32, [64, 128, 192, 255].repeat(1024)).unwrap();
        let fast = encode(&bitmap, Format::Jpeg).unwrap();
        let small = encode_with_options(&bitmap, Format::Jpeg, smallest()).unwrap();
        assert!(small.len() <= fast.len());
        let decoded = crate::decode(&small).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (32, 32));
    }

    #[test]
    fn optimized_huffman_limits_code_lengths() {
        let mut frequencies = [0u32; 257];
        let (mut previous, mut current) = (1u32, 1u32);
        for count in frequencies.iter_mut().take(30) {
            *count = current;
            (previous, current) = (current, previous + current);
        }
        let table = jpeg_optimized_huffman(frequencies);
        assert_eq!(table.values.len(), 30);
        assert_eq!(
            table
                .lengths
                .iter()
                .map(|&count| usize::from(count))
                .sum::<usize>(),
            30
        );
        for &(code, length) in &table.codes[..30] {
            assert!((1..=16).contains(&length));
            assert_ne!(u32::from(code), (1u32 << length) - 1);
        }

        let mut state = 7u32;
        for _ in 0..64 {
            let mut frequencies = [0u32; 257];
            for count in frequencies.iter_mut().take(162) {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                *count = if state & 3 == 0 { 0 } else { state % 1024 + 1 };
            }
            frequencies[0] = 1;
            let table = jpeg_optimized_huffman(frequencies);
            assert_eq!(
                table
                    .lengths
                    .iter()
                    .map(|&count| usize::from(count))
                    .sum::<usize>(),
                table.values.len()
            );
            for (symbol, &count) in frequencies.iter().enumerate().take(162) {
                if count != 0 {
                    let (code, length) = table.codes[symbol];
                    assert!((1..=16).contains(&length));
                    assert_ne!(u32::from(code), (1u32 << length) - 1);
                }
            }
        }
    }

    #[test]
    fn jpeg_high_quality_checkerboard_decodes() {
        let mut pixels = Vec::new();
        for y in 0..17 {
            for x in 0..19 {
                let color = if (x + y) % 2 == 0 { 0 } else { 255 };
                pixels.extend_from_slice(&[color, 255 - color, color, 255]);
            }
        }
        let bitmap = Bitmap::new(19, 17, pixels).unwrap();
        let tables = JpegTables::new(100);
        let planes = JpegPlanes::new(&bitmap, false);
        for sampling in [(1, 1), (2, 1), (1, 2), (2, 2)] {
            let blocks = planes.blocks(sampling.0, sampling.1, &tables);
            let decoded = crate::decode(&jpeg_write(19, 17, false, sampling, &tables, &blocks));
            assert_eq!(decoded.unwrap().pixels().len(), 19 * 17 * 4);
        }
        let gray = Bitmap::new(19, 17, [127, 127, 127, 255].repeat(19 * 17)).unwrap();
        let blocks = JpegPlanes::new(&gray, true).blocks(1, 1, &tables);
        let decoded = crate::decode(&jpeg_write(19, 17, true, (1, 1), &tables, &blocks)).unwrap();
        assert_eq!(decoded.pixels(), [127, 127, 127, 255].repeat(19 * 17));
        for style in [
            EncodingStyle::NormalCompression,
            EncodingStyle::MaxCompression,
        ] {
            let encoded = encode_with_options(
                &bitmap,
                Format::Jpeg,
                EncodeOptions {
                    style,
                    jpeg_quality: 100,
                },
            )
            .unwrap();
            let decoded = crate::decode(&encoded).unwrap();
            assert_eq!((decoded.width(), decoded.height()), (19, 17));
        }
    }
}
