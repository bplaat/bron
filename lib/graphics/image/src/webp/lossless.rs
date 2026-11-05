/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use alloc::vec::Vec;

use super::super::{Budget, DecodeError, Result};

type Pixel = [u8; 4];

pub(super) struct Bits<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> Bits<'a> {
    pub(super) const fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0 }
    }

    fn read(&mut self, count: usize) -> Result<usize> {
        if count > 24
            || self
                .position
                .checked_add(count)
                .is_none_or(|end| end.div_ceil(8) > self.data.len())
        {
            return Err(DecodeError::InvalidData);
        }
        let mut value = 0;
        for bit in 0..count {
            value |= usize::from((self.data[self.position / 8] >> (self.position % 8)) & 1) << bit;
            self.position += 1;
        }
        Ok(value)
    }
}

struct Huffman {
    nodes: Vec<([usize; 2], Option<usize>)>,
}

impl Huffman {
    fn build(lengths: &[u8], budget: &mut Budget) -> Result<Self> {
        let mut counts = [0usize; 16];
        for &length in lengths {
            if length > 15 {
                return Err(DecodeError::InvalidData);
            }
            if length != 0 {
                counts[usize::from(length)] += 1;
            }
        }
        let total: usize = counts.iter().sum();
        if total == 0 {
            return Err(DecodeError::InvalidData);
        }
        if total == 1 {
            let symbol = lengths.iter().position(|&n| n != 0).expect("one symbol");
            return Ok(Self {
                nodes: alloc::vec![([0; 2], Some(symbol))],
            });
        }
        let mut remaining = 1i32;
        for &count in &counts[1..] {
            remaining = remaining * 2 - count as i32;
            if remaining < 0 {
                return Err(DecodeError::InvalidData);
            }
        }
        if remaining != 0 {
            return Err(DecodeError::InvalidData);
        }
        let mut next = [0usize; 16];
        for i in 1..16 {
            next[i] = (next[i - 1] + counts[i - 1]) << 1;
        }
        let mut nodes = budget.zeroed::<([usize; 2], Option<usize>)>(total * 2 - 1)?;
        let mut used = 1;
        for (symbol, &length) in lengths.iter().enumerate() {
            if length == 0 {
                continue;
            }
            let code = next[usize::from(length)];
            next[usize::from(length)] += 1;
            let mut index = 0;
            for bit in (0..length).rev() {
                let branch = (code >> bit) & 1;
                if nodes[index].0[branch] == 0 {
                    if used == nodes.len() {
                        return Err(DecodeError::InvalidData);
                    }
                    nodes[index].0[branch] = used;
                    used += 1;
                }
                index = nodes[index].0[branch];
            }
            nodes[index].1 = Some(symbol);
        }
        Ok(Self { nodes })
    }

    fn read(bits: &mut Bits<'_>, alphabet: usize, budget: &mut Budget) -> Result<Self> {
        let mut lengths = budget.zeroed::<u8>(alphabet)?;
        if bits.read(1)? != 0 {
            let count = bits.read(1)? + 1;
            let width = 1 + 7 * bits.read(1)?;
            let first = bits.read(width)?;
            if first >= alphabet {
                return Err(DecodeError::InvalidData);
            }
            lengths[first] = 1;
            if count == 2 {
                let second = bits.read(8)?;
                if second >= alphabet {
                    return Err(DecodeError::InvalidData);
                }
                lengths[second] = 1;
            }
        } else {
            const ORDER: [usize; 19] = [
                17, 18, 0, 1, 2, 3, 4, 5, 16, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
            ];
            let count = bits.read(4)? + 4;
            let mut code_lengths = [0; 19];
            for &i in &ORDER[..count] {
                code_lengths[i] = bits.read(3)? as u8;
            }
            let code = Self::build(&code_lengths, budget)?;
            let symbols = if bits.read(1)? == 0 {
                alphabet
            } else {
                let width = 2 + 2 * bits.read(3)?;
                bits.read(width)? + 2
            };
            if symbols > alphabet {
                return Err(DecodeError::InvalidData);
            }
            let mut position = 0;
            let mut previous = 8;
            for _ in 0..symbols {
                if position == alphabet {
                    break;
                }
                let value = code.symbol(bits)?;
                let (length, count) = match value {
                    0..=15 => {
                        if value != 0 {
                            previous = value as u8;
                        }
                        (value as u8, 1)
                    }
                    16 => (previous, bits.read(2)? + 3),
                    17 => (0, bits.read(3)? + 3),
                    18 => (0, bits.read(7)? + 11),
                    _ => return Err(DecodeError::InvalidData),
                };
                let end = position + count;
                if end > alphabet {
                    return Err(DecodeError::InvalidData);
                }
                lengths[position..end].fill(length);
                position = end;
            }
        }
        Self::build(&lengths, budget)
    }

    fn symbol(&self, bits: &mut Bits<'_>) -> Result<usize> {
        let mut position = 0;
        for _ in 0..=15 {
            if let Some(value) = self.nodes[position].1 {
                return Ok(value);
            }
            position = self.nodes[position].0[bits.read(1)?];
            if position == 0 {
                return Err(DecodeError::InvalidData);
            }
        }
        Err(DecodeError::InvalidData)
    }
}

struct Transform {
    kind: usize,
    width: usize,
    bits: usize,
    pixels: Vec<Pixel>,
}

fn add(a: Pixel, b: Pixel) -> Pixel {
    core::array::from_fn(|i| a[i].wrapping_add(b[i]))
}
fn average(a: Pixel, b: Pixel) -> Pixel {
    core::array::from_fn(|i| ((u16::from(a[i]) + u16::from(b[i])) / 2) as u8)
}
fn clamp(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

pub(super) fn decode(data: &[u8], budget: &mut Budget) -> Result<(usize, usize, Vec<Pixel>)> {
    if data.first() != Some(&0x2f) {
        return Err(DecodeError::InvalidHeader);
    }
    let mut bits = Bits::new(&data[1..]);
    let width = bits.read(14)? + 1;
    let height = bits.read(14)? + 1;
    bits.read(1)?;
    if bits.read(3)? != 0 {
        return Err(DecodeError::UnsupportedFeature);
    }
    let pixels = stream(&mut bits, width, height, budget)?;
    Ok((width, height, pixels))
}

pub(super) fn alpha(
    data: &[u8],
    width: usize,
    height: usize,
    budget: &mut Budget,
) -> Result<Vec<Pixel>> {
    stream(&mut Bits::new(data), width, height, budget)
}

fn stream(
    bits: &mut Bits<'_>,
    width: usize,
    height: usize,
    budget: &mut Budget,
) -> Result<Vec<Pixel>> {
    let mut transforms = Vec::new();
    let mut current_width = width;
    let mut used = 0;
    while bits.read(1)? != 0 {
        let kind = bits.read(2)?;
        if used & (1 << kind) != 0 {
            return Err(DecodeError::InvalidData);
        }
        used |= 1 << kind;
        let original_width = current_width;
        let (size, pixels) = match kind {
            0 | 1 => {
                let size = bits.read(3)? + 2;
                (
                    size,
                    image(
                        bits,
                        current_width.div_ceil(1 << size),
                        height.div_ceil(1 << size),
                        false,
                        budget,
                    )?,
                )
            }
            2 => (0, Vec::new()),
            3 => {
                let count = bits.read(8)? + 1;
                let mut palette = image(bits, count, 1, false, budget)?;
                for i in 1..palette.len() {
                    palette[i] = add(palette[i], palette[i - 1]);
                }
                let size = if count <= 2 {
                    3
                } else if count <= 4 {
                    2
                } else if count <= 16 {
                    1
                } else {
                    0
                };
                current_width = current_width.div_ceil(1 << size);
                (size, palette)
            }
            _ => unreachable!(),
        };
        transforms.push(Transform {
            kind,
            width: original_width,
            bits: size,
            pixels,
        });
    }
    let mut pixels = image(bits, current_width, height, true, budget)?;
    for transform in transforms.into_iter().rev() {
        let width = transform.width;
        let block_width = width.div_ceil(1 << transform.bits);
        match transform.kind {
            0 => {
                for y in 0..height {
                    for x in 0..width {
                        let i = y * width + x;
                        let prediction = if y == 0 {
                            if x == 0 {
                                [0, 0, 0, 255]
                            } else {
                                pixels[i - 1]
                            }
                        } else if x == 0 {
                            pixels[i - width]
                        } else {
                            let mode = transform.pixels
                                [(y >> transform.bits) * block_width + (x >> transform.bits)][1];
                            let (l, t, tl) =
                                (pixels[i - 1], pixels[i - width], pixels[i - width - 1]);
                            let tr = if x + 1 == width {
                                pixels[y * width]
                            } else {
                                pixels[i - width + 1]
                            };
                            match mode {
                                0 => [0, 0, 0, 255],
                                1 => l,
                                2 => t,
                                3 => tr,
                                4 => tl,
                                5 => average(average(l, tr), t),
                                6 => average(l, tl),
                                7 => average(l, t),
                                8 => average(tl, t),
                                9 => average(t, tr),
                                10 => average(average(l, tl), average(t, tr)),
                                11 => {
                                    let mut left = 0;
                                    let mut top = 0;
                                    for c in 0..4 {
                                        left += (i32::from(t[c]) - i32::from(tl[c])).abs();
                                        top += (i32::from(l[c]) - i32::from(tl[c])).abs();
                                    }
                                    if left < top { l } else { t }
                                }
                                12 => core::array::from_fn(|c| {
                                    clamp(i32::from(l[c]) + i32::from(t[c]) - i32::from(tl[c]))
                                }),
                                13 => {
                                    let a = average(l, t);
                                    core::array::from_fn(|c| {
                                        clamp(
                                            i32::from(a[c])
                                                + (i32::from(a[c]) - i32::from(tl[c])) / 2,
                                        )
                                    })
                                }
                                _ => return Err(DecodeError::InvalidData),
                            }
                        };
                        pixels[i] = add(pixels[i], prediction);
                    }
                }
            }
            1 => {
                for y in 0..height {
                    for x in 0..width {
                        let c = transform.pixels
                            [(y >> transform.bits) * block_width + (x >> transform.bits)];
                        let p = &mut pixels[y * width + x];
                        let delta =
                            |a: u8, b: u8| ((i16::from(a as i8) * i16::from(b as i8)) >> 5) as u8;
                        p[0] = p[0].wrapping_add(delta(c[2], p[1]));
                        p[2] = p[2]
                            .wrapping_add(delta(c[1], p[1]))
                            .wrapping_add(delta(c[0], p[0]));
                    }
                }
            }
            2 => {
                for p in &mut pixels {
                    p[0] = p[0].wrapping_add(p[1]);
                    p[2] = p[2].wrapping_add(p[1]);
                }
            }
            3 => {
                let bits = 8 >> transform.bits;
                let mask = (1 << bits) - 1;
                let mut expanded = budget.zeroed::<Pixel>(
                    width
                        .checked_mul(height)
                        .ok_or(DecodeError::ImageTooLarge)?,
                )?;
                for y in 0..height {
                    for x in 0..width {
                        let index =
                            usize::from(pixels[y * current_width + (x >> transform.bits)][1])
                                >> ((x & ((1 << transform.bits) - 1)) * bits)
                                & mask;
                        expanded[y * width + x] =
                            transform.pixels.get(index).copied().unwrap_or([0; 4]);
                    }
                }
                pixels = expanded;
            }
            _ => unreachable!(),
        }
        current_width = width;
    }
    Ok(pixels)
}

fn prefix(bits: &mut Bits<'_>, symbol: usize) -> Result<usize> {
    if symbol < 4 {
        return Ok(symbol + 1);
    }
    let extra = (symbol - 2) >> 1;
    Ok(((2 + (symbol & 1)) << extra) + bits.read(extra)? + 1)
}

fn image(
    bits: &mut Bits<'_>,
    width: usize,
    height: usize,
    main: bool,
    budget: &mut Budget,
) -> Result<Vec<Pixel>> {
    let has_cache = bits.read(1)? != 0;
    let cache_bits = if has_cache { bits.read(4)? } else { 0 };
    if has_cache && !(1..=11).contains(&cache_bits) {
        return Err(DecodeError::InvalidData);
    }
    let cache_size = if cache_bits == 0 { 0 } else { 1 << cache_bits };
    let mut cache = budget.zeroed::<Pixel>(cache_size)?;
    let mut meta_bits = 0;
    let mut meta = Vec::new();
    let mut group_count = 1;
    let mut meta_width = 0;
    if main && bits.read(1)? != 0 {
        meta_bits = bits.read(3)? + 2;
        meta_width = width.div_ceil(1 << meta_bits);
        meta = image(
            bits,
            meta_width,
            height.div_ceil(1 << meta_bits),
            false,
            budget,
        )?;
        group_count = meta
            .iter()
            .map(|p| usize::from(p[0]) * 256 + usize::from(p[1]) + 1)
            .max()
            .ok_or(DecodeError::InvalidData)?;
    }
    budget.claim(group_count * size_of::<[Huffman; 5]>())?;
    let mut groups = Vec::new();
    groups
        .try_reserve_exact(group_count)
        .map_err(|_| DecodeError::ImageTooLarge)?;
    for _ in 0..group_count {
        groups.push([
            Huffman::read(bits, 280 + cache_size, budget)?,
            Huffman::read(bits, 256, budget)?,
            Huffman::read(bits, 256, budget)?,
            Huffman::read(bits, 256, budget)?,
            Huffman::read(bits, 40, budget)?,
        ]);
    }
    let count = width
        .checked_mul(height)
        .ok_or(DecodeError::ImageTooLarge)?;
    let mut out = budget.zeroed::<Pixel>(count)?;
    let mut position = 0;
    while position < count {
        let group = if meta.is_empty() {
            0
        } else {
            let p = meta[((position / width) >> meta_bits) * meta_width
                + ((position % width) >> meta_bits)];
            usize::from(p[0]) * 256 + usize::from(p[1])
        };
        let codes = &groups[group];
        let symbol = codes[0].symbol(bits)?;
        let start = position;
        if symbol < 256 {
            out[position] = [
                codes[1].symbol(bits)? as u8,
                symbol as u8,
                codes[2].symbol(bits)? as u8,
                codes[3].symbol(bits)? as u8,
            ];
            position += 1;
        } else if symbol < 280 {
            let length = prefix(bits, symbol - 256)?;
            let distance_code = codes[4].symbol(bits)?;
            let distance = prefix(bits, distance_code)?;
            let distance = if distance > 120 {
                distance - 120
            } else {
                let (x, y) = DISTANCE[distance - 1];
                (i64::from(x) + i64::from(y) * width as i64).max(1) as usize
            };
            if distance > position || length > count - position {
                return Err(DecodeError::InvalidData);
            }
            for _ in 0..length {
                out[position] = out[position - distance];
                position += 1;
            }
        } else {
            out[position] = *cache.get(symbol - 280).ok_or(DecodeError::InvalidData)?;
            position += 1;
        }
        if cache_size != 0 {
            for &p in &out[start..position] {
                let argb = u32::from_be_bytes([p[3], p[0], p[1], p[2]]);
                let index = argb.wrapping_mul(0x1e35_a7bd) >> (32 - cache_bits);
                cache[index as usize] = p;
            }
        }
    }
    Ok(out)
}

#[rustfmt::skip]
const DISTANCE: [(i8, i8); 120] = [
    ( 0, 1), ( 1, 0), ( 1, 1), (-1, 1), ( 0, 2), ( 2, 0), ( 1, 2), (-1, 2),
    ( 2, 1), (-2, 1), ( 2, 2), (-2, 2), ( 0, 3), ( 3, 0), ( 1, 3), (-1, 3),
    ( 3, 1), (-3, 1), ( 2, 3), (-2, 3), ( 3, 2), (-3, 2), ( 0, 4), ( 4, 0),
    ( 1, 4), (-1, 4), ( 4, 1), (-4, 1), ( 3, 3), (-3, 3), ( 2, 4), (-2, 4),
    ( 4, 2), (-4, 2), ( 0, 5), ( 3, 4), (-3, 4), ( 4, 3), (-4, 3), ( 5, 0),
    ( 1, 5), (-1, 5), ( 5, 1), (-5, 1), ( 2, 5), (-2, 5), ( 5, 2), (-5, 2),
    ( 4, 4), (-4, 4), ( 3, 5), (-3, 5), ( 5, 3), (-5, 3), ( 0, 6), ( 6, 0),
    ( 1, 6), (-1, 6), ( 6, 1), (-6, 1), ( 2, 6), (-2, 6), ( 6, 2), (-6, 2),
    ( 4, 5), (-4, 5), ( 5, 4), (-5, 4), ( 3, 6), (-3, 6), ( 6, 3), (-6, 3),
    ( 0, 7), ( 7, 0), ( 1, 7), (-1, 7), ( 5, 5), (-5, 5), ( 7, 1), (-7, 1),
    ( 4, 6), (-4, 6), ( 6, 4), (-6, 4), ( 2, 7), (-2, 7), ( 7, 2), (-7, 2),
    ( 3, 7), (-3, 7), ( 7, 3), (-7, 3), ( 5, 6), (-5, 6), ( 6, 5), (-6, 5),
    ( 8, 0), ( 4, 7), (-4, 7), ( 7, 4), (-7, 4), ( 8, 1), ( 8, 2), ( 6, 6),
    (-6, 6), ( 8, 3), ( 5, 7), (-5, 7), ( 7, 5), (-7, 5), ( 8, 4), ( 6, 7),
    (-6, 7), ( 7, 6), (-7, 6), ( 8, 5), ( 7, 7), (-7, 7), ( 8, 6), ( 8, 7),
];
