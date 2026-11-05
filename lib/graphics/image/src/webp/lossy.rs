/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use alloc::vec::Vec;

use super::super::{Budget, DecodeError, Result, pixel_len};

#[path = "tables.rs"]
mod tables;

struct Boolean<'a> {
    data: &'a [u8],
    position: usize,
    range: u32,
    value: u32,
    count: usize,
}

impl<'a> Boolean<'a> {
    fn new(data: &'a [u8]) -> Result<Self> {
        let first = *data.first().ok_or(DecodeError::InvalidData)?;
        Ok(Self {
            data,
            position: 2,
            range: 255,
            value: u32::from(first) << 8 | u32::from(data.get(1).copied().unwrap_or(0)),
            count: 0,
        })
    }
    fn bit(&mut self, probability: u8) -> Result<bool> {
        let split = 1 + (((self.range - 1) * u32::from(probability)) >> 8);
        let value = self.value >= split << 8;
        if value {
            self.value -= split << 8;
            self.range -= split;
        } else {
            self.range = split;
        }
        while self.range < 128 {
            self.range <<= 1;
            self.value <<= 1;
            self.count += 1;
            if self.count == 8 {
                if self.position > self.data.len() + 2 {
                    return Err(DecodeError::InvalidData);
                }
                self.value |= u32::from(self.data.get(self.position).copied().unwrap_or(0));
                self.position += 1;
                self.count = 0;
            }
        }
        Ok(value)
    }
    fn literal(&mut self, bits: usize) -> Result<i32> {
        let mut value = 0;
        for _ in 0..bits {
            value = value * 2 + i32::from(self.bit(128)?);
        }
        Ok(value)
    }
    fn signed(&mut self, bits: usize) -> Result<i32> {
        let value = self.literal(bits)?;
        Ok(if self.bit(128)? { -value } else { value })
    }
    fn optional(&mut self, bits: usize) -> Result<i32> {
        if self.bit(128)? {
            self.signed(bits)
        } else {
            Ok(0)
        }
    }
    fn tree(&mut self, tree: &[i8], probabilities: &[u8]) -> Result<usize> {
        let mut index = 0;
        for _ in 0..16 {
            let next = tree[index + usize::from(self.bit(probabilities[index / 2])?)];
            if next <= 0 {
                return Ok((-next) as usize);
            }
            index = next as usize;
        }
        Err(DecodeError::InvalidData)
    }
}

#[derive(Clone, Default)]
struct Macroblock {
    segment: usize,
    skip: bool,
    y: usize,
    uv: usize,
    modes: [usize; 16],
    nonzero: bool,
}

struct Plane {
    pixels: Vec<u8>,
    width: usize,
    height: usize,
}
impl Plane {
    fn new(width: usize, height: usize, budget: &mut Budget) -> Result<Self> {
        Ok(Self {
            pixels: budget.zeroed(
                width
                    .checked_mul(height)
                    .ok_or(DecodeError::ImageTooLarge)?,
            )?,
            width,
            height,
        })
    }
    fn get(&self, x: isize, y: isize) -> i32 {
        if y < 0 {
            return 127;
        }
        if x < 0 {
            return 129;
        }
        i32::from(
            self.pixels
                [(y as usize).min(self.height - 1) * self.width + (x as usize).min(self.width - 1)],
        )
    }
    fn set(&mut self, x: usize, y: usize, value: i32) {
        self.pixels[y * self.width + x] = value.clamp(0, 255) as u8;
    }
    fn predict(&mut self, x: usize, y: usize, size: usize, mode: usize) {
        let dc = if x == 0 && y == 0 {
            128
        } else {
            let mut total = 0;
            let mut count = 0;
            if x != 0 {
                for i in 0..size {
                    total += self.get(x as isize - 1, (y + i) as isize);
                }
                count += size;
            }
            if y != 0 {
                for i in 0..size {
                    total += self.get((x + i) as isize, y as isize - 1);
                }
                count += size;
            }
            (total + count as i32 / 2) / count as i32
        };
        let corner = self.get(x as isize - 1, y as isize - 1);
        for row in 0..size {
            let left = self.get(x as isize - 1, (y + row) as isize);
            for col in 0..size {
                let top = self.get((x + col) as isize, y as isize - 1);
                self.set(
                    x + col,
                    y + row,
                    match mode {
                        0 => dc,
                        1 => top,
                        2 => left,
                        _ => left + top - corner,
                    },
                );
            }
        }
    }
    fn predict4(&mut self, x: usize, y: usize, mode: usize) {
        let mut a = [0; 8];
        let mut l = [0; 4];
        let mut e = [0; 9];
        for (i, value) in a.iter_mut().enumerate() {
            let (px, py) = if i >= 4 && x % 16 == 12 {
                (x + i, y / 16 * 16)
            } else {
                (x + i, y)
            };
            *value = self.get(px as isize, py as isize - 1);
        }
        for i in 0..4 {
            l[i] = self.get(x as isize - 1, (y + i) as isize);
            e[3 - i] = l[i];
            e[5 + i] = a[i];
        }
        e[4] = self.get(x as isize - 1, y as isize - 1);
        let avg = |a: i32, b: i32| (a + b + 1) >> 1;
        let smooth = |a: i32, b: i32, c: i32| (a + b * 2 + c + 2) >> 2;
        let se = |i: usize| smooth(e[i - 1], e[i], e[i + 1]);
        let ae = |i: usize| avg(e[i], e[i + 1]);
        let sa = |i: usize| smooth(a[i.saturating_sub(1)], a[i.min(7)], a[(i + 1).min(7)]);
        let aa = |i: usize| avg(a[i], a[(i + 1).min(7)]);
        let sl = |i: usize| {
            smooth(
                if i == 0 { e[4] } else { l[i - 1] },
                l[i.min(3)],
                l[(i + 1).min(3)],
            )
        };
        let al = |i: usize| avg(l[i.min(3)], l[(i + 1).min(3)]);
        let dc = (a[..4].iter().sum::<i32>() + l.iter().sum::<i32>() + 4) >> 3;
        for (row, &left) in l.iter().enumerate() {
            for col in 0..4 {
                let value = match mode {
                    0 => dc,
                    1 => left + a[col] - e[4],
                    2 => smooth(if col == 0 { e[4] } else { a[col - 1] }, a[col], a[col + 1]),
                    3 => sl(row),
                    4 => sa(row + col + 1),
                    5 => se((4 + col) - row),
                    6 => {
                        let t = col as isize * 2 - row as isize;
                        if t >= 0 {
                            if row % 2 == 0 {
                                ae(4 + col - row / 2)
                            } else {
                                se(4 + col - row / 2)
                            }
                        } else {
                            se((5 + t) as usize)
                        }
                    }
                    7 => {
                        if row == 3 && col == 3 {
                            sa(6)
                        } else if row == 2 && col == 3 {
                            sa(5)
                        } else if row % 2 == 0 {
                            aa(col + row / 2)
                        } else {
                            sa(col + row / 2 + 1)
                        }
                    }
                    8 => {
                        let t = row as isize * 2 - col as isize;
                        if t >= 0 {
                            if col % 2 == 0 {
                                ae(3 - row + col / 2)
                            } else {
                                se(4 - row + col / 2)
                            }
                        } else {
                            se((3 - t) as usize)
                        }
                    }
                    9 => {
                        let p = row + col / 2;
                        if p >= 3 {
                            l[3]
                        } else if col % 2 == 0 {
                            al(p)
                        } else {
                            sl(p + 1)
                        }
                    }
                    _ => 0,
                };
                self.set(x + col, y + row, value);
            }
        }
    }
    fn residue(&mut self, x: usize, y: usize, coefficients: [i32; 16]) {
        let mut temp = [0; 16];
        for col in 0..4 {
            let out = idct([
                coefficients[col],
                coefficients[col + 4],
                coefficients[col + 8],
                coefficients[col + 12],
            ]);
            for row in 0..4 {
                temp[row * 4 + col] = out[row];
            }
        }
        for row in 0..4 {
            let out = idct(temp[row * 4..row * 4 + 4].try_into().expect("row"));
            for (col, &residue) in out.iter().enumerate() {
                let value = self.get((x + col) as isize, (y + row) as isize) + ((residue + 4) >> 3);
                self.set(x + col, y + row, value);
            }
        }
    }
}

const fn idct(v: [i32; 4]) -> [i32; 4] {
    let a = v[0] + v[2];
    let b = v[0] - v[2];
    let c = ((v[1] as i64 * 35468) >> 16) as i32 - v[3] - ((v[3] as i64 * 20091) >> 16) as i32;
    let d = v[1] + ((v[1] as i64 * 20091) >> 16) as i32 + ((v[3] as i64 * 35468) >> 16) as i32;
    [a + d, b + c, b - c, a - d]
}
fn walsh(v: [i32; 16]) -> [i32; 16] {
    let mut out = [0; 16];
    for col in 0..4 {
        let a = v[col] + v[col + 12];
        let b = v[col + 4] + v[col + 8];
        let c = v[col + 4] - v[col + 8];
        let d = v[col] - v[col + 12];
        out[col] = a + b;
        out[col + 4] = c + d;
        out[col + 8] = a - b;
        out[col + 12] = d - c;
    }
    for row in 0..4 {
        let i = row * 4;
        let a = out[i] + out[i + 3];
        let b = out[i + 1] + out[i + 2];
        let c = out[i + 1] - out[i + 2];
        let d = out[i] - out[i + 3];
        out[i] = (a + b + 3) >> 3;
        out[i + 1] = (c + d + 3) >> 3;
        out[i + 2] = (a - b + 3) >> 3;
        out[i + 3] = (d - c + 3) >> 3;
    }
    out
}

fn coefficients(
    reader: &mut Boolean<'_>,
    probabilities: &[u8; 1056],
    kind: usize,
    context: usize,
    quant: [i32; 2],
) -> Result<([i32; 16], bool)> {
    const BANDS: [usize; 16] = [0, 1, 2, 3, 6, 4, 5, 6, 6, 6, 6, 6, 6, 6, 6, 7];
    const ZIGZAG: [usize; 16] = [0, 1, 4, 8, 5, 2, 3, 6, 9, 12, 13, 10, 7, 11, 14, 15];
    const TREE: [i8; 22] = [
        -11, 2, 0, 4, -1, 6, 8, 12, -2, 10, -3, -4, 14, 16, -5, -6, 18, 20, -7, -8, -9, -10,
    ];
    const EXTRA: [&[u8]; 6] = [
        &[159],
        &[165, 145],
        &[173, 148, 140],
        &[176, 155, 140, 135],
        &[180, 157, 141, 134, 130],
        &[254, 254, 243, 230, 196, 177, 153, 140, 133, 130, 129],
    ];
    let mut out = [0; 16];
    let mut context = context;
    let mut zero = false;
    let mut nonzero = false;
    for i in usize::from(kind == 0)..16 {
        let index = (kind * 8 + BANDS[i]) * 33 + context * 11;
        let probs = &probabilities[index..index + 11];
        let token = if zero {
            let mut node = 2usize;
            loop {
                let value = TREE[node + usize::from(reader.bit(probs[node / 2])?)];
                if value <= 0 {
                    break (-value) as usize;
                }
                node = value as usize;
            }
        } else {
            reader.tree(&TREE, probs)?
        };
        if token == 11 {
            break;
        }
        if token == 0 {
            context = 0;
            zero = true;
            continue;
        }
        zero = false;
        nonzero = true;
        let mut value = if token < 5 {
            token as i32
        } else {
            let mut extra = 0;
            for &prob in EXTRA[token - 5] {
                extra = extra * 2 + i32::from(reader.bit(prob)?);
            }
            [5, 7, 11, 19, 35, 67][token - 5] + extra
        };
        context = if value == 1 { 1 } else { 2 };
        if reader.bit(128)? {
            value = -value;
        }
        out[ZIGZAG[i]] = value * quant[usize::from(i != 0)];
    }
    Ok((out, nonzero))
}

pub(super) fn decode(data: &[u8], budget: &mut Budget) -> Result<(usize, usize, Vec<[u8; 4]>)> {
    let header = data.get(..10).ok_or(DecodeError::InvalidData)?;
    let tag = u32::from(header[0]) | (u32::from(header[1]) << 8) | (u32::from(header[2]) << 16);
    if tag & 1 != 0 || &header[3..6] != b"\x9d\x01\x2a" {
        return Err(DecodeError::InvalidHeader);
    }
    if (tag >> 1) & 7 > 3 {
        return Err(DecodeError::UnsupportedFeature);
    }
    let width = usize::from(u16::from_le_bytes([header[6], header[7]]) & 0x3fff);
    let height = usize::from(u16::from_le_bytes([header[8], header[9]]) & 0x3fff);
    pixel_len(width as u32, height as u32)?;
    let split = 10 + (tag >> 5) as usize;
    let first = data.get(10..split).ok_or(DecodeError::InvalidData)?;
    let mut reader = Boolean::new(first)?;
    if reader.bit(128)? {
        return Err(DecodeError::UnsupportedFeature);
    }
    reader.bit(128)?;
    let segmented = reader.bit(128)?;
    let mut map = false;
    let mut absolute = false;
    let mut segment_q = [0; 4];
    let mut segment_filter = [0; 4];
    let mut segment_probs = [255; 3];
    if segmented {
        map = reader.bit(128)?;
        let update = reader.bit(128)?;
        if update {
            absolute = reader.bit(128)?;
            for q in &mut segment_q {
                *q = reader.optional(7)?;
            }
            for level in &mut segment_filter {
                *level = reader.optional(6)?;
            }
        }
        if map {
            for prob in &mut segment_probs {
                if reader.bit(128)? {
                    *prob = reader.literal(8)? as u8;
                }
            }
        }
    }
    let simple = reader.bit(128)?;
    let filter = reader.literal(6)?;
    let sharpness = reader.literal(3)?;
    let mut reference_delta = [0; 4];
    let mut mode_delta = [0; 4];
    let adjust = reader.bit(128)?;
    if adjust && reader.bit(128)? {
        for delta in &mut reference_delta {
            if reader.bit(128)? {
                *delta = reader.signed(6)?;
            }
        }
        for delta in &mut mode_delta {
            if reader.bit(128)? {
                *delta = reader.signed(6)?;
            }
        }
    }
    let partitions = 1usize << reader.literal(2)?;
    let base_q = reader.literal(7)?;
    let mut delta = [0; 5];
    for d in &mut delta {
        *d = reader.optional(4)?;
    }
    reader.bit(128)?;
    let mut probabilities = tables::COEFFICIENTS;
    for (p, &update) in probabilities.iter_mut().zip(&tables::UPDATE) {
        if reader.bit(update)? {
            *p = reader.literal(8)? as u8;
        }
    }
    let skip_enabled = reader.bit(128)?;
    let skip_prob = if skip_enabled {
        reader.literal(8)? as u8
    } else {
        0
    };
    let columns = width.div_ceil(16);
    let rows = height.div_ceil(16);
    let mut blocks = budget.zeroed::<Macroblock>(columns * rows)?;
    let mut above = budget.zeroed::<usize>(columns * 4)?;
    for row in 0..rows {
        let mut left = [0; 4];
        for col in 0..columns {
            let block = &mut blocks[row * columns + col];
            if map {
                block.segment = reader.tree(&[2, 4, 0, -1, -2, -3], &segment_probs)?;
            }
            block.skip = skip_enabled && reader.bit(skip_prob)?;
            block.y = reader.tree(&[-4, 2, 4, 6, 0, -1, -2, -3], &[145, 156, 163, 128])?;
            if block.y == 4 {
                for (y, left) in left.iter_mut().enumerate() {
                    for x in 0..4 {
                        let a = above[col * 4 + x];
                        let l = *left;
                        let mode = reader.tree(
                            &[
                                0, 2, -1, 4, -2, 6, 8, 12, -3, 10, -5, -6, -4, 14, -7, 16, -8, -9,
                            ],
                            &tables::MODES[(a * 10 + l) * 9..][..9],
                        )?;
                        block.modes[y * 4 + x] = mode;
                        above[col * 4 + x] = mode;
                        *left = mode;
                    }
                }
            } else {
                let mode = [0, 2, 3, 1][block.y];
                above[col * 4..col * 4 + 4].fill(mode);
                left.fill(mode);
            }
            block.uv = reader.tree(&[0, 2, -1, 4, -2, -3], &[142, 114, 183])?;
        }
    }
    let lengths = data
        .get(split..split + 3 * (partitions - 1))
        .ok_or(DecodeError::InvalidData)?;
    let mut offset = split + lengths.len();
    let mut tokens = Vec::new();
    for i in 0..partitions {
        let end = if i + 1 == partitions {
            data.len()
        } else {
            let n = &lengths[i * 3..i * 3 + 3];
            offset + usize::from(n[0]) + (usize::from(n[1]) << 8) + (usize::from(n[2]) << 16)
        };
        tokens.push(Boolean::new(
            data.get(offset..end).ok_or(DecodeError::InvalidData)?,
        )?);
        offset = end;
    }
    let mut planes = [
        Plane::new(columns * 16, rows * 16, budget)?,
        Plane::new(columns * 8, rows * 8, budget)?,
        Plane::new(columns * 8, rows * 8, budget)?,
    ];
    let mut above_y2 = budget.zeroed::<usize>(columns)?;
    let mut contexts = budget.zeroed::<[usize; 8]>(columns)?;
    for row in 0..rows {
        let mut left = [0usize; 8];
        let mut left_y2 = 0;
        for col in 0..columns {
            let block = &mut blocks[row * columns + col];
            let token = &mut tokens[row % partitions];
            let q = if segmented {
                if absolute {
                    segment_q[block.segment]
                } else {
                    base_q + segment_q[block.segment]
                }
            } else {
                base_q
            };
            let dc = |offset: i32| tables::DC[(q + offset).clamp(0, 127) as usize];
            let ac = |offset: i32| tables::AC[(q + offset).clamp(0, 127) as usize];
            let mut y2 = [0; 16];
            if block.y != 4 {
                let (coeff, nz) = if block.skip {
                    ([0; 16], false)
                } else {
                    coefficients(
                        token,
                        &probabilities,
                        1,
                        left_y2 + above_y2[col],
                        [dc(delta[1]) * 2, (ac(delta[2]) * 155 / 100).max(8)],
                    )?
                };
                left_y2 = usize::from(nz);
                above_y2[col] = left_y2;
                y2 = walsh(coeff);
                block.nonzero |= nz;
                planes[0].predict(col * 16, row * 16, 16, block.y);
            }
            for y in 0..4 {
                for x in 0..4 {
                    let bx = col * 16 + x * 4;
                    let by = row * 16 + y * 4;
                    if block.y == 4 {
                        planes[0].predict4(bx, by, block.modes[y * 4 + x]);
                    }
                    let (mut coeff, nz) = if block.skip {
                        ([0; 16], false)
                    } else {
                        coefficients(
                            token,
                            &probabilities,
                            if block.y == 4 { 3 } else { 0 },
                            left[y] + contexts[col][x],
                            [dc(delta[0]), ac(0)],
                        )?
                    };
                    if block.y != 4 {
                        coeff[0] = y2[y * 4 + x];
                    }
                    left[y] = usize::from(nz);
                    contexts[col][x] = left[y];
                    block.nonzero |= nz;
                    planes[0].residue(bx, by, coeff);
                }
            }
            for (plane, output) in planes.iter_mut().enumerate().skip(1) {
                output.predict(col * 8, row * 8, 8, block.uv);
                let base = 4 + (plane - 1) * 2;
                for y in 0..2 {
                    for x in 0..2 {
                        let (coeff, nz) = if block.skip {
                            ([0; 16], false)
                        } else {
                            coefficients(
                                token,
                                &probabilities,
                                2,
                                left[base + y] + contexts[col][base + x],
                                [dc(delta[3]).min(132), ac(delta[4])],
                            )?
                        };
                        left[base + y] = usize::from(nz);
                        contexts[col][base + x] = left[base + y];
                        block.nonzero |= nz;
                        output.residue(col * 8 + x * 4, row * 8 + y * 4, coeff);
                    }
                }
            }
        }
    }
    for row in 0..rows {
        for col in 0..columns {
            let block = &blocks[row * columns + col];
            let mut level = if segmented {
                if absolute {
                    segment_filter[block.segment]
                } else {
                    filter + segment_filter[block.segment]
                }
            } else {
                filter
            };
            if adjust {
                level += reference_delta[0];
                if block.y == 4 {
                    level += mode_delta[0];
                }
            }
            level = level.clamp(0, 63);
            if level != 0 {
                for (i, plane) in planes.iter_mut().enumerate() {
                    if simple && i != 0 {
                        continue;
                    }
                    let size = if i == 0 { 16 } else { 8 };
                    filter_block(
                        plane,
                        (col * size, row * size),
                        size,
                        level,
                        sharpness,
                        simple,
                        block.y == 4 || block.nonzero,
                    );
                }
            }
        }
    }
    let mut output = budget.zeroed::<[u8; 4]>(width * height)?;
    for y in 0..height {
        for x in 0..width {
            let sample = |plane: &Plane| {
                let ix = x.div_ceil(2);
                let iy = y.div_ceil(2);
                let (xa, xb) = if x == 0 { (0, 0) } else { (ix - 1, ix) };
                let (ya, yb) = if y == 0 { (0, 0) } else { (iy - 1, iy) };
                let wx = if x & 1 == 0 { 3 } else { 1 };
                let wy = if y & 1 == 0 { 3 } else { 1 };
                let a = plane.get(xa as isize, ya as isize) * (4 - wx)
                    + plane.get(xb as isize, ya as isize) * wx;
                let b = plane.get(xa as isize, yb as isize) * (4 - wx)
                    + plane.get(xb as isize, yb as isize) * wx;
                ((a * (4 - wy) + b * wy + 8) >> 4) - 128
            };
            let yy = (planes[0].get(x as isize, y as isize) - 16) * 19077;
            let u = sample(&planes[1]);
            let v = sample(&planes[2]);
            output[y * width + x] = [
                ((yy + 26149 * v + 8192) >> 14).clamp(0, 255) as u8,
                ((yy - 6419 * u - 13320 * v + 8192) >> 14).clamp(0, 255) as u8,
                ((yy + 33050 * u + 8192) >> 14).clamp(0, 255) as u8,
                255,
            ];
        }
    }
    Ok((width, height, output))
}

fn filter_block(
    plane: &mut Plane,
    position: (usize, usize),
    size: usize,
    level: i32,
    sharpness: i32,
    simple: bool,
    internal: bool,
) {
    let (x, y) = position;
    let mut interior = level;
    if sharpness != 0 {
        interior >>= if sharpness > 4 { 2 } else { 1 };
        interior = interior.min(9 - sharpness);
    }
    interior = interior.max(1);
    let hev = if level >= 40 {
        2
    } else if level >= 15 {
        1
    } else {
        0
    };
    for vertical in [true, false] {
        if (if vertical { x } else { y }) != 0 {
            for i in 0..size {
                let index = if vertical {
                    (y + i) * plane.width + x
                } else {
                    y * plane.width + x + i
                };
                filter_edge(
                    &mut plane.pixels,
                    index,
                    if vertical { 1 } else { plane.width },
                    2 * (level + 2) + interior,
                    interior,
                    hev,
                    simple,
                    true,
                );
            }
        }
        if internal {
            for offset in (4..size).step_by(4) {
                for i in 0..size {
                    let index = if vertical {
                        (y + i) * plane.width + x + offset
                    } else {
                        (y + offset) * plane.width + x + i
                    };
                    filter_edge(
                        &mut plane.pixels,
                        index,
                        if vertical { 1 } else { plane.width },
                        2 * level + interior,
                        interior,
                        hev,
                        simple,
                        false,
                    );
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn filter_edge(
    pixels: &mut [u8],
    index: usize,
    step: usize,
    edge: i32,
    interior: i32,
    threshold: i32,
    simple: bool,
    macroblock: bool,
) {
    let mut v = [0i32; 8];
    for (i, value) in v.iter_mut().enumerate() {
        *value = i32::from(pixels[(index as isize + (i as isize - 4) * step as isize) as usize]);
    }
    if 2 * (v[3] - v[4]).abs() + (v[2] - v[5]).abs() / 2 > edge {
        return;
    }
    if !simple
        && ((v[0] - v[1]).abs() > interior
            || (v[1] - v[2]).abs() > interior
            || (v[2] - v[3]).abs() > interior
            || (v[4] - v[5]).abs() > interior
            || (v[5] - v[6]).abs() > interior
            || (v[6] - v[7]).abs() > interior)
    {
        return;
    }
    let hev = (v[2] - v[3]).abs() > threshold || (v[5] - v[4]).abs() > threshold;
    let signed = |n: i32| n.clamp(-128, 127);
    if !simple && macroblock && !hev {
        let adjustment = signed(signed(v[2] - v[5]) + 3 * (v[4] - v[3]));
        for (before, after, factor) in [(3, 4, 27), (2, 5, 18), (1, 6, 9)] {
            let delta = (factor * adjustment + 63) >> 7;
            v[before] = (v[before] + delta).clamp(0, 255);
            v[after] = (v[after] - delta).clamp(0, 255);
        }
    } else {
        let adjustment = signed(
            if simple || hev {
                signed(v[2] - v[5])
            } else {
                0
            } + 3 * (v[4] - v[3]),
        );
        let delta = signed(adjustment + 4) >> 3;
        v[4] = (v[4] - delta).clamp(0, 255);
        v[3] = (v[3] + (signed(adjustment + 3) >> 3)).clamp(0, 255);
        if !simple && !hev {
            let half = (delta + 1) >> 1;
            v[5] = (v[5] - half).clamp(0, 255);
            v[2] = (v[2] + half).clamp(0, 255);
        }
    }
    for (i, value) in v.iter().enumerate() {
        pixels[(index as isize + (i as isize - 4) * step as isize) as usize] = *value as u8;
    }
}
