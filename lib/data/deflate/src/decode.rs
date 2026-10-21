/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! DEFLATE decompression with two-level Huffman lookup tables.

use alloc::vec::Vec;

use crate::{CODE_LENGTH_ORDER, DIST_BASE, DIST_EXTRA, Error, LEN_BASE, LEN_EXTRA};

// Table entries pack the code length in bits 0..4, a subtable size in bits 4..8, extra
// bits in 8..12, the kind in 12..14 and a value or subtable offset in 16..32. A zero
// code length marks an unused code.
const KIND_LITERAL: u32 = 0;
const KIND_MATCH: u32 = 1;
const KIND_END: u32 = 2;
const KIND_LINK: u32 = 3;

const LITLEN_BITS: u32 = 10;
const DIST_BITS: u32 = 8;
const CODE_LENGTH_BITS: u32 = 7;

const fn entry(kind: u32, value: u32, extra: u8) -> u32 {
    value << 16 | kind << 12 | (extra as u32) << 8
}

fn litlen_entry(symbol: usize) -> Option<u32> {
    match symbol {
        0..256 => Some(entry(KIND_LITERAL, symbol as u32, 0)),
        256 => Some(entry(KIND_END, 0, 0)),
        257..286 => Some(entry(
            KIND_MATCH,
            u32::from(LEN_BASE[symbol - 257]),
            LEN_EXTRA[symbol - 257],
        )),
        _ => None,
    }
}

fn dist_entry(symbol: usize) -> Option<u32> {
    (symbol < 30).then(|| entry(KIND_MATCH, u32::from(DIST_BASE[symbol]), DIST_EXTRA[symbol]))
}

const fn code_length_entry(symbol: usize) -> Option<u32> {
    Some(entry(KIND_LITERAL, symbol as u32, 0))
}

// MARK: Tables
/// The most codes in one table: the 288 fixed literal and length codes.
const MAX_CODES: usize = 288;

#[derive(Default)]
struct Table {
    entries: Vec<u32>,
    bits: u32,
}

impl Table {
    /// Builds a lookup table for canonical codes, rejecting over-subscribed code lengths.
    fn new(lens: &[u8], bits: u32, symbol: fn(usize) -> Option<u32>) -> Result<Self, Error> {
        let mut table = Self::default();
        table.build(lens, bits, symbol)?;
        Ok(table)
    }

    /// Rebuilds the table in place, reusing its entry storage across blocks.
    fn build(
        &mut self,
        lens: &[u8],
        bits: u32,
        symbol: fn(usize) -> Option<u32>,
    ) -> Result<(), Error> {
        debug_assert!(lens.len() <= MAX_CODES && bits <= LITLEN_BITS);
        let mut count = [0u32; 16];
        for &len in lens {
            count[len as usize] += 1;
        }
        let mut left = 1i32;
        for &n in &count[1..] {
            left = (left << 1) - n as i32;
            if left < 0 {
                return Err(Error::InvalidData);
            }
        }
        let mut next = [0u32; 16];
        for len in 1..16 {
            next[len] = (next[len - 1] + count[len - 1] * u32::from(len > 1)) << 1;
        }
        let mut codes = [0u32; MAX_CODES];
        let codes = &mut codes[..lens.len()];
        for (code, &len) in codes.iter_mut().zip(lens) {
            if len > 0 {
                *code = next[len as usize].reverse_bits() >> (32 - len);
                next[len as usize] += 1;
            }
        }

        // Codes longer than the primary table share a subtable per primary prefix.
        let primary = 1usize << bits;
        let mut sub_bits = [0u32; 1 << LITLEN_BITS];
        let sub_bits = &mut sub_bits[..primary];
        for (&code, &len) in codes.iter().zip(lens) {
            let len = u32::from(len);
            if len > bits {
                let prefix = code as usize & (primary - 1);
                sub_bits[prefix] = sub_bits[prefix].max(len - bits);
            }
        }
        let entries = &mut self.entries;
        entries.clear();
        entries.resize(primary, 0);
        for (prefix, &sub) in sub_bits.iter().enumerate() {
            if sub > 0 {
                entries[prefix] = entry(KIND_LINK, entries.len() as u32, 0) | sub << 4;
                entries.resize(entries.len() + (1 << sub), 0);
            }
        }
        for (index, (&code, &len)) in codes.iter().zip(lens).enumerate() {
            let len = u32::from(len);
            if len == 0 {
                continue;
            }
            let Some(value) = symbol(index) else {
                continue;
            };
            let value = value | len;
            if len <= bits {
                for slot in (code as usize..primary).step_by(1 << len) {
                    entries[slot] = value;
                }
            } else {
                let link = entries[code as usize & (primary - 1)];
                let (offset, sub) = ((link >> 16) as usize, (link >> 4) & 15);
                for slot in ((code >> bits) as usize..1 << sub).step_by(1 << (len - bits)) {
                    entries[offset + slot] = value;
                }
            }
        }
        self.bits = bits;
        Ok(())
    }

    fn decode(&self, input: &mut Bits) -> Result<u32, Error> {
        let peek = input.buffer as usize;
        let mut value = self.entries[peek & ((1 << self.bits) - 1)];
        if (value >> 12) & 3 == KIND_LINK {
            let sub = (value >> 4) & 15;
            value = self.entries[(value >> 16) as usize + ((peek >> self.bits) & ((1 << sub) - 1))];
        }
        let len = value & 15;
        if len == 0 {
            return Err(Error::InvalidData);
        }
        input.consume(len);
        Ok(value)
    }
}

// MARK: Bit reader
struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    buffer: u64,
    count: u32,
}

impl Bits<'_> {
    /// Fills the buffer to at least 56 bits, reading zeros past the end of the input.
    fn refill(&mut self) -> Result<(), Error> {
        if let Some(word) = self.data.get(self.pos..).and_then(<[u8]>::first_chunk::<8>) {
            self.buffer |= u64::from_le_bytes(*word) << self.count;
            self.pos += (63 - self.count as usize) / 8;
            self.count |= 56;
            return Ok(());
        }
        while self.count <= 56 {
            self.buffer |= u64::from(self.data.get(self.pos).copied().unwrap_or(0)) << self.count;
            self.pos += 1;
            self.count += 8;
        }
        if self.consumed() > self.data.len() {
            return Err(Error::InvalidData);
        }
        Ok(())
    }

    const fn consume(&mut self, count: u32) {
        self.buffer >>= count;
        self.count -= count;
    }

    const fn take(&mut self, count: u32) -> u32 {
        let value = (self.buffer & ((1 << count) - 1)) as u32;
        self.consume(count);
        value
    }

    const fn align(&mut self) {
        self.consume(self.count % 8);
    }

    const fn consumed(&self) -> usize {
        self.pos - self.count as usize / 8
    }
}

// MARK: Output
fn reserve(out: &mut Vec<u8>, additional: usize, limit: usize) -> Result<(), Error> {
    if additional > limit - out.len() {
        return Err(Error::TooLarge);
    }
    if out.capacity() - out.len() < additional {
        let grow = out
            .len()
            .max(1 << 15)
            .max(additional)
            .min(limit - out.len());
        out.try_reserve(grow).map_err(|_| Error::OutOfMemory)?;
    }
    Ok(())
}

// MARK: Blocks
fn inflate_block(
    input: &mut Bits,
    litlen: &Table,
    dist: &Table,
    out: &mut Vec<u8>,
    limit: usize,
) -> Result<(), Error> {
    loop {
        // One refill covers the longest length code, distance code and their extra bits.
        input.refill()?;
        let value = litlen.decode(input)?;
        match (value >> 12) & 3 {
            KIND_LITERAL => {
                reserve(out, 1, limit)?;
                out.push((value >> 16) as u8);
            }
            KIND_END => return Ok(()),
            _ => {
                let len = (value >> 16) as usize + input.take((value >> 8) & 15) as usize;
                let value = dist.decode(input)?;
                let distance = (value >> 16) as usize + input.take((value >> 8) & 15) as usize;
                if distance > out.len() {
                    return Err(Error::InvalidData);
                }
                reserve(out, len, limit)?;
                let start = out.len() - distance;
                if distance >= len {
                    out.extend_from_within(start..start + len);
                } else if distance == 1 {
                    out.resize(out.len() + len, out[start]);
                } else {
                    // Copying everything since `start` doubles the repeated pattern each step.
                    let end = out.len() + len;
                    while out.len() < end {
                        let count = (end - out.len()).min(out.len() - start);
                        out.extend_from_within(start..start + count);
                    }
                }
            }
        }
    }
}

/// Tables rebuilt for every dynamic block, kept to reuse their storage.
#[derive(Default)]
struct DynamicTables {
    code_length: Table,
    litlen: Table,
    dist: Table,
}

fn read_dynamic_tables(input: &mut Bits, tables: &mut DynamicTables) -> Result<(), Error> {
    input.refill()?;
    let hlit = input.take(5) as usize + 257;
    let hdist = input.take(5) as usize + 1;
    let hclen = input.take(4) as usize + 4;
    if hlit > 286 || hdist > 30 {
        return Err(Error::InvalidData);
    }
    let mut code_lengths = [0u8; 19];
    for &symbol in &CODE_LENGTH_ORDER[..hclen] {
        input.refill()?;
        code_lengths[symbol] = input.take(3) as u8;
    }
    tables
        .code_length
        .build(&code_lengths, CODE_LENGTH_BITS, code_length_entry)?;
    let mut lens = [0u8; 286 + 30];
    let total = hlit + hdist;
    let mut index = 0;
    while index < total {
        input.refill()?;
        let symbol = tables.code_length.decode(input)? >> 16;
        let (value, repeat) = match symbol {
            0..16 => (symbol as u8, 1),
            16 if index > 0 => (lens[index - 1], 3 + input.take(2) as usize),
            17 => (0, 3 + input.take(3) as usize),
            18 => (0, 11 + input.take(7) as usize),
            _ => return Err(Error::InvalidData),
        };
        if index + repeat > total {
            return Err(Error::InvalidData);
        }
        lens[index..index + repeat].fill(value);
        index += repeat;
    }
    if lens[256] == 0 {
        return Err(Error::InvalidData);
    }
    tables
        .litlen
        .build(&lens[..hlit], LITLEN_BITS, litlen_entry)?;
    tables.dist.build(&lens[hlit..total], DIST_BITS, dist_entry)
}

fn fixed_tables() -> Result<(Table, Table), Error> {
    let mut lens = [8u8; 288];
    lens[144..256].fill(9);
    lens[256..280].fill(7);
    Ok((
        Table::new(&lens, LITLEN_BITS, litlen_entry)?,
        Table::new(&[5; 30], DIST_BITS, dist_entry)?,
    ))
}

/// Decompresses one DEFLATE stream and returns the number of input bytes it used.
pub(crate) fn decompress(data: &[u8], limit: usize, out: &mut Vec<u8>) -> Result<usize, Error> {
    // Keep an exact capacity reserved from a size hint, otherwise guess from the input size.
    if out.capacity() == 0 {
        reserve(out, data.len().saturating_mul(4).min(limit), limit)?;
    }
    let mut input = Bits {
        data,
        pos: 0,
        buffer: 0,
        count: 0,
    };
    let mut fixed = None;
    let mut dynamic = DynamicTables::default();
    loop {
        input.refill()?;
        let last = input.take(1) == 1;
        match input.take(2) {
            0 => {
                input.align();
                let start = input.consumed();
                let header = data.get(start..start + 4).ok_or(Error::InvalidData)?;
                let len = u16::from_le_bytes([header[0], header[1]]);
                if len != !u16::from_le_bytes([header[2], header[3]]) {
                    return Err(Error::InvalidData);
                }
                let len = len as usize;
                let bytes = data
                    .get(start + 4..start + 4 + len)
                    .ok_or(Error::InvalidData)?;
                reserve(out, len, limit)?;
                out.extend_from_slice(bytes);
                input = Bits {
                    data,
                    pos: start + 4 + len,
                    buffer: 0,
                    count: 0,
                };
            }
            1 => {
                if fixed.is_none() {
                    fixed = Some(fixed_tables()?);
                }
                if let Some((litlen, dist)) = &fixed {
                    inflate_block(&mut input, litlen, dist, out, limit)?;
                }
            }
            2 => {
                read_dynamic_tables(&mut input, &mut dynamic)?;
                inflate_block(&mut input, &dynamic.litlen, &dynamic.dist, out, limit)?;
            }
            _ => return Err(Error::InvalidData),
        }
        if last {
            break;
        }
    }
    input.align();
    let consumed = input.consumed();
    if consumed > data.len() {
        return Err(Error::InvalidData);
    }
    Ok(consumed)
}
