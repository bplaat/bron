/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! DEFLATE compression: LZ77 parsing, block splitting and Huffman coding.

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

use crate::{CODE_LENGTH_ORDER, DIST_BASE, DIST_EXTRA, LEN_BASE, LEN_EXTRA, Level};

const WINDOW: usize = 1 << 15;
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;
const HASH_BITS: u32 = 15;
const NONE: usize = usize::MAX;

const LEN_SYMBOL: [u8; MAX_MATCH + 1] = {
    let mut table = [0; MAX_MATCH + 1];
    let (mut symbol, mut len) = (0, MIN_MATCH);
    while len <= MAX_MATCH {
        while symbol + 1 < LEN_BASE.len() && LEN_BASE[symbol + 1] as usize <= len {
            symbol += 1;
        }
        table[len] = symbol as u8;
        len += 1;
    }
    table
};

const fn dist_symbol(dist: usize) -> usize {
    if dist <= 4 {
        dist - 1
    } else {
        let log = (usize::BITS - 1 - (dist - 1).leading_zeros()) as usize;
        log * 2 + (((dist - 1) >> (log - 1)) & 1)
    }
}

// MARK: Parameters
#[derive(Clone, Copy)]
struct Params {
    /// Match candidates to visit per position.
    depth: usize,
    /// Match length that ends the search early.
    nice: usize,
    /// Match length below which a longer match one byte later is tried (0 for greedy).
    lazy: usize,
    /// Input bytes parsed together, which bounds memory use.
    segment: usize,
    /// Most blocks a segment may be split into.
    max_blocks: usize,
    /// Optimal parsing passes per cost model refinement (0 for hash chain parsing).
    passes: usize,
    /// Rounds of block splitting followed by parsing each block again.
    rounds: usize,
    /// Lengths tried at each end of a long match during optimal parsing.
    span: usize,
}

const fn params(level: Level) -> Params {
    match level {
        Level::Fast => Params {
            depth: 32,
            nice: 128,
            lazy: 0,
            segment: 1 << 16,
            max_blocks: 16,
            passes: 0,
            rounds: 0,
            span: 0,
        },
        Level::Default => Params {
            depth: 64,
            nice: MAX_MATCH,
            lazy: MAX_MATCH,
            segment: 1 << 18,
            max_blocks: 16,
            passes: 0,
            rounds: 0,
            span: 0,
        },
        Level::Best => Params {
            depth: 64,
            nice: MAX_MATCH,
            lazy: 0,
            segment: 1 << 20,
            max_blocks: 32,
            passes: 2,
            rounds: 1,
            span: 8,
        },
        Level::Crunch => Params {
            depth: 512,
            nice: MAX_MATCH,
            lazy: 0,
            segment: 1 << 20,
            max_blocks: 64,
            passes: 10,
            rounds: 2,
            span: 16,
        },
    }
}

// MARK: Symbols
/// A literal byte (`len == 0`, byte in `dist`) or a back reference.
#[derive(Clone, Copy)]
struct Symbol {
    len: u16,
    dist: u16,
}

impl Symbol {
    const fn literal(byte: u8) -> Self {
        Self {
            len: 0,
            dist: byte as u16,
        }
    }

    const fn bytes(self) -> usize {
        if self.len == 0 { 1 } else { self.len as usize }
    }
}

#[derive(Clone)]
struct Histogram {
    litlen: [u32; 286],
    dist: [u32; 30],
}

impl Histogram {
    const EMPTY: Self = Self {
        litlen: [0; 286],
        dist: [0; 30],
    };

    /// Counts the symbols of a block, including its end code.
    fn new(symbols: &[Symbol]) -> Self {
        let mut histogram = Self::EMPTY;
        for &symbol in symbols {
            histogram.add(symbol);
        }
        histogram.litlen[256] = 1;
        histogram
    }

    const fn add(&mut self, symbol: Symbol) {
        if symbol.len == 0 {
            self.litlen[symbol.dist as usize] += 1;
        } else {
            self.litlen[257 + LEN_SYMBOL[symbol.len as usize] as usize] += 1;
            self.dist[dist_symbol(symbol.dist as usize)] += 1;
        }
    }

    fn extra_bits(&self) -> u64 {
        let len = self.litlen[257..].iter().zip(LEN_EXTRA);
        let dist = self.dist.iter().zip(DIST_EXTRA);
        len.chain(dist)
            .map(|(&count, extra)| u64::from(count) * u64::from(extra))
            .sum()
    }
}

// MARK: Huffman codes
/// Computes optimal code lengths of at most `max_bits` for `freqs`.
fn code_lengths(freqs: &[u32], max_bits: usize, lens: &mut [u8]) {
    lens.fill(0);
    let mut leaves = [(0u32, 0u16); 316];
    let mut count = 0;
    for (symbol, &freq) in freqs.iter().enumerate() {
        if freq > 0 {
            leaves[count] = (freq, symbol as u16);
            count += 1;
        }
    }
    let leaves = &mut leaves[..count];
    // Two codes keep the tree complete, which strict decoders require.
    if count < 2 {
        let used = leaves.first().map_or(0, |&(_, symbol)| symbol as usize);
        lens[used] = 1;
        lens[usize::from(used == 0)] = 1;
        return;
    }
    leaves.sort_unstable();
    let mut depths = [0u32; 316];
    let depths = &mut depths[..count];
    for (depth, &(freq, _)) in depths.iter_mut().zip(leaves.iter()) {
        *depth = freq;
    }
    minimum_redundancy(depths);
    // The rarest symbol gets the longest code.
    if depths[0] as usize <= max_bits {
        for (&depth, &(_, symbol)) in depths.iter().zip(leaves.iter()) {
            lens[symbol as usize] = depth as u8;
        }
    } else {
        package_merge(leaves, max_bits, lens);
    }
}

/// Replaces ascending weights by their unlimited Huffman code lengths, in place
/// (Moffat and Katajainen).
fn minimum_redundancy(a: &mut [u32]) {
    let n = a.len();
    // Combine nodes; a[root..next] hold parent indices, a[next] the new weight.
    a[0] += a[1];
    let (mut root, mut leaf) = (0, 2);
    for next in 1..n - 1 {
        if leaf >= n || a[root] < a[leaf] {
            a[next] = a[root];
            a[root] = next as u32;
            root += 1;
        } else {
            a[next] = a[leaf];
            leaf += 1;
        }
        if leaf >= n || (root < next && a[root] < a[leaf]) {
            a[next] += a[root];
            a[root] = next as u32;
            root += 1;
        } else {
            a[next] += a[leaf];
            leaf += 1;
        }
    }
    // Convert parent indices to internal node depths.
    a[n - 2] = 0;
    for next in (0..n - 2).rev() {
        a[next] = a[a[next] as usize] + 1;
    }
    // Assign leaf depths from the shallowest level down.
    let (mut available, mut depth) = (1usize, 0u32);
    let (mut root, mut next) = (n as isize - 2, n as isize - 1);
    while available > 0 {
        let mut used = 0;
        while root >= 0 && a[root as usize] == depth {
            used += 1;
            root -= 1;
        }
        while available > used {
            a[next as usize] = depth;
            next -= 1;
            available -= 1;
        }
        available = 2 * used;
        depth += 1;
    }
}

/// Computes optimal length-limited code lengths for ascending leaves (package-merge).
fn package_merge(leaves: &[(u32, u16)], max_bits: usize, lens: &mut [u8]) {
    // Nodes are (weight, left, right); a leaf has `left == NONE` and its symbol in `right`.
    let mut nodes: Vec<(u64, usize, usize)> = leaves
        .iter()
        .map(|&(weight, symbol)| (u64::from(weight), NONE, symbol as usize))
        .collect();
    let leaf_count = leaves.len();
    let mut list: Vec<usize> = (0..leaf_count).collect();
    let mut merged = Vec::with_capacity(leaf_count * 2);
    for _ in 1..max_bits {
        merged.clear();
        let packages = nodes.len();
        for pair in list.as_chunks::<2>().0 {
            nodes.push((nodes[pair[0]].0 + nodes[pair[1]].0, pair[0], pair[1]));
        }
        let (mut leaf, mut package) = (0, packages);
        while leaf < leaf_count || package < nodes.len() {
            if package == nodes.len() || (leaf < leaf_count && nodes[leaf].0 <= nodes[package].0) {
                merged.push(leaf);
                leaf += 1;
            } else {
                merged.push(package);
                package += 1;
            }
        }
        core::mem::swap(&mut list, &mut merged);
    }
    let mut stack = Vec::new();
    for &node in &list[..2 * leaf_count - 2] {
        stack.push(node);
        while let Some(node) = stack.pop() {
            let (_, left, right) = nodes[node];
            if left == NONE {
                lens[right] += 1;
            } else {
                stack.extend([left, right]);
            }
        }
    }
}

/// Assigns canonical codes, bit-reversed for LSB-first output.
fn canonical_codes(lens: &[u8], codes: &mut [u16]) {
    let mut count = [0u16; 16];
    for &len in lens {
        count[len as usize] += 1;
    }
    let mut next = [0u16; 16];
    for len in 2..16 {
        next[len] = (next[len - 1] + count[len - 1]) << 1;
    }
    for (code, &len) in codes.iter_mut().zip(lens) {
        if len > 0 {
            *code = next[len as usize].reverse_bits() >> (16 - len);
            next[len as usize] += 1;
        }
    }
}

const FIXED_LITLEN: [u8; 288] = {
    let mut lens = [8; 288];
    let mut symbol = 144;
    while symbol < 288 {
        lens[symbol] = if symbol < 256 {
            9
        } else if symbol < 280 {
            7
        } else {
            8
        };
        symbol += 1;
    }
    lens
};
const FIXED_DIST: [u8; 30] = [5; 30];

/// Dynamic Huffman code for one block with its run-length encoded code lengths.
struct DynamicCode {
    litlen: [u8; 286],
    dist: [u8; 30],
    code_lengths: [u8; 19],
    runs: Vec<(u8, u8)>,
    hlit: usize,
    hdist: usize,
    hclen: usize,
    header_bits: u64,
}

impl DynamicCode {
    fn new(histogram: &Histogram) -> Self {
        let mut litlen = [0; 286];
        let mut dist = [0; 30];
        code_lengths(&histogram.litlen, 15, &mut litlen);
        code_lengths(&histogram.dist, 15, &mut dist);
        let hlit = litlen
            .iter()
            .rposition(|&len| len > 0)
            .map_or(0, |p| p + 1)
            .max(257);
        let hdist = dist
            .iter()
            .rposition(|&len| len > 0)
            .map_or(0, |p| p + 1)
            .max(1);
        let mut all = [0; 316];
        all[..hlit].copy_from_slice(&litlen[..hlit]);
        all[hlit..hlit + hdist].copy_from_slice(&dist[..hdist]);
        let runs = runs(&all[..hlit + hdist]);
        let mut freqs = [0u32; 19];
        for &(symbol, _) in &runs {
            freqs[symbol as usize] += 1;
        }
        let mut code_lengths_lens = [0; 19];
        code_lengths(&freqs, 7, &mut code_lengths_lens);
        let hclen = CODE_LENGTH_ORDER
            .iter()
            .rposition(|&symbol| code_lengths_lens[symbol] > 0)
            .map_or(0, |p| p + 1)
            .max(4);
        let header_bits = 14
            + 3 * hclen as u64
            + runs
                .iter()
                .map(|&(symbol, _)| {
                    u64::from(code_lengths_lens[symbol as usize]) + run_extra_bits(symbol) as u64
                })
                .sum::<u64>();
        Self {
            litlen,
            dist,
            code_lengths: code_lengths_lens,
            runs,
            hlit,
            hdist,
            hclen,
            header_bits,
        }
    }
}

const fn run_extra_bits(symbol: u8) -> u32 {
    match symbol {
        16 => 2,
        17 => 3,
        18 => 7,
        _ => 0,
    }
}

/// Run-length encodes code lengths as (symbol, extra) pairs.
fn runs(lens: &[u8]) -> Vec<(u8, u8)> {
    let mut out = Vec::new();
    let mut index = 0;
    while index < lens.len() {
        let value = lens[index];
        let mut run = lens[index..]
            .iter()
            .take_while(|&&len| len == value)
            .count();
        index += run;
        if value == 0 {
            while run >= 11 {
                let n = run.min(138);
                out.push((18, (n - 11) as u8));
                run -= n;
            }
            if run >= 3 {
                out.push((17, (run - 3) as u8));
                run = 0;
            }
        } else {
            out.push((value, 0));
            run -= 1;
            while run >= 3 {
                let n = run.min(6);
                out.push((16, (n - 3) as u8));
                run -= n;
            }
        }
        out.extend(core::iter::repeat_n((value, 0), run));
    }
    out
}

fn data_bits(histogram: &Histogram, litlen: &[u8], dist: &[u8]) -> u64 {
    let litlen = histogram.litlen.iter().zip(litlen);
    let dist = histogram.dist.iter().zip(dist);
    litlen
        .chain(dist)
        .map(|(&count, &len)| u64::from(count) * u64::from(len))
        .sum::<u64>()
        + histogram.extra_bits()
}

/// Sizes in bits of a block with its dynamic code and with the fixed code, excluding the
/// block header.
fn huffman_bits(histogram: &Histogram, code: &DynamicCode) -> (u64, u64) {
    (
        code.header_bits + data_bits(histogram, &code.litlen, &code.dist),
        data_bits(histogram, &FIXED_LITLEN, &FIXED_DIST),
    )
}

/// Size in bits of the smaller Huffman encoding of a block.
fn block_bits(histogram: &Histogram) -> u64 {
    let (dynamic, fixed) = huffman_bits(histogram, &DynamicCode::new(histogram));
    3 + dynamic.min(fixed)
}

/// Approximate size in bits of a block from symbol entropy, much cheaper than building
/// its Huffman codes.
fn estimated_bits(histogram: &Histogram) -> u64 {
    let litlen = entropy(&histogram.litlen).into_iter().zip(histogram.litlen);
    let dist = entropy(&histogram.dist).into_iter().zip(histogram.dist);
    let (bits, used) = litlen
        .chain(dist)
        .filter(|&(_, freq)| freq > 0)
        .fold((0.0, 0), |(bits, used), (cost, freq)| {
            (bits + cost * freq as f32, used + 1)
        });
    // Each used symbol costs about four header bits for its code length.
    bits as u64 + histogram.extra_bits() + 4 * used + 32
}

// MARK: Bit writer
struct BitWriter<'a> {
    out: &'a mut Vec<u8>,
    bits: u64,
    count: u32,
}

impl BitWriter<'_> {
    fn put(&mut self, value: u32, count: u32) {
        self.bits |= u64::from(value) << self.count;
        self.count += count;
        if self.count >= 32 {
            self.out
                .extend_from_slice(&(self.bits as u32).to_le_bytes());
            self.bits >>= 32;
            self.count -= 32;
        }
    }

    fn align(&mut self) {
        let bytes = self.count.div_ceil(8) as usize;
        self.out
            .extend_from_slice(&self.bits.to_le_bytes()[..bytes]);
        self.bits = 0;
        self.count = 0;
    }
}

fn write_block(writer: &mut BitWriter, symbols: &[Symbol], raw: &[u8], last: bool) {
    let histogram = Histogram::new(symbols);
    let code = DynamicCode::new(&histogram);
    let (dynamic, fixed) = huffman_bits(&histogram, &code);
    let stored = (raw.len().div_ceil(0xffff) * 40 + raw.len() * 8) as u64;
    if stored < dynamic.min(fixed) {
        let chunks = raw.chunks(0xffff).count();
        for (index, chunk) in raw.chunks(0xffff).enumerate() {
            writer.put(u32::from(last && index + 1 == chunks), 3);
            writer.align();
            let len = chunk.len() as u16;
            writer.out.extend_from_slice(&len.to_le_bytes());
            writer.out.extend_from_slice(&(!len).to_le_bytes());
            writer.out.extend_from_slice(chunk);
        }
        return;
    }

    let (litlen, dist_lens) = if fixed <= dynamic {
        writer.put(u32::from(last) | 2, 3);
        (&FIXED_LITLEN[..], &FIXED_DIST[..])
    } else {
        writer.put(u32::from(last) | 4, 3);
        writer.put((code.hlit - 257) as u32, 5);
        writer.put((code.hdist - 1) as u32, 5);
        writer.put((code.hclen - 4) as u32, 4);
        for &symbol in &CODE_LENGTH_ORDER[..code.hclen] {
            writer.put(u32::from(code.code_lengths[symbol]), 3);
        }
        let mut codes = [0; 19];
        canonical_codes(&code.code_lengths, &mut codes);
        for &(symbol, extra) in &code.runs {
            let symbol = symbol as usize;
            writer.put(
                u32::from(codes[symbol]),
                u32::from(code.code_lengths[symbol]),
            );
            writer.put(u32::from(extra), run_extra_bits(symbol as u8));
        }
        (&code.litlen[..], &code.dist[..])
    };
    let mut litlen_codes = [0; 288];
    let mut dist_codes = [0; 30];
    canonical_codes(litlen, &mut litlen_codes);
    canonical_codes(dist_lens, &mut dist_codes);
    for &symbol in symbols {
        if symbol.len == 0 {
            let byte = symbol.dist as usize;
            writer.put(u32::from(litlen_codes[byte]), u32::from(litlen[byte]));
        } else {
            let len = LEN_SYMBOL[symbol.len as usize] as usize;
            writer.put(
                u32::from(litlen_codes[257 + len]),
                u32::from(litlen[257 + len]),
            );
            writer.put(
                u32::from(symbol.len - LEN_BASE[len]),
                u32::from(LEN_EXTRA[len]),
            );
            let dist = dist_symbol(symbol.dist as usize);
            writer.put(u32::from(dist_codes[dist]), u32::from(dist_lens[dist]));
            writer.put(
                u32::from(symbol.dist - DIST_BASE[dist]),
                u32::from(DIST_EXTRA[dist]),
            );
        }
    }
    writer.put(u32::from(litlen_codes[256]), u32::from(litlen[256]));
}

// MARK: Match finders
const fn hash_value(value: u32) -> usize {
    (value.wrapping_mul(0x9e37_79b1) >> (32 - HASH_BITS)) as usize
}

fn hash4(data: &[u8], pos: usize) -> usize {
    hash_value(u32::from_le_bytes(
        *data[pos..].first_chunk().expect("four bytes"),
    ))
}

fn hash(data: &[u8], pos: usize) -> usize {
    hash_value(
        u32::from(data[pos]) | u32::from(data[pos + 1]) << 8 | u32::from(data[pos + 2]) << 16,
    )
}

/// Length of the common prefix of `a` and `b`, up to `max`.
fn common_prefix(a: &[u8], b: &[u8], max: usize) -> usize {
    let (a, b) = (&a[..max], &b[..max]);
    let mut len = 0;
    for (x, y) in a.as_chunks::<8>().0.iter().zip(b.as_chunks::<8>().0) {
        let diff = u64::from_le_bytes(*x) ^ u64::from_le_bytes(*y);
        if diff != 0 {
            return len + diff.trailing_zeros() as usize / 8;
        }
        len += 8;
    }
    len + a[len..]
        .iter()
        .zip(&b[len..])
        .take_while(|(x, y)| x == y)
        .count()
}

/// Hash chains over a sliding window, for greedy and lazy parsing.
///
/// Chains link positions sharing four bytes, and a separate table keeps the latest
/// position of each three bytes, the only length 3 candidate usually worth its distance.
/// Positions are stored truncated to 32 bits; every candidate is verified against the
/// data, so a wrapped position only costs a wasted comparison.
struct HashChains {
    head: Box<[u32; 1 << HASH_BITS]>,
    prev: Box<[u32; WINDOW]>,
    head3: Box<[u32; 1 << HASH_BITS]>,
    inserted: usize,
}

impl HashChains {
    fn new() -> Self {
        Self {
            head: Box::new([u32::MAX; 1 << HASH_BITS]),
            prev: Box::new([u32::MAX; WINDOW]),
            head3: Box::new([u32::MAX; 1 << HASH_BITS]),
            inserted: 0,
        }
    }

    /// Returns the longest match at `pos` longer than `shorter`, as (length, distance), after
    /// indexing earlier positions.
    fn find(
        &mut self,
        data: &[u8],
        pos: usize,
        shorter: usize,
        depth: usize,
        nice: usize,
    ) -> (usize, usize) {
        while self.inserted < pos {
            let at = self.inserted;
            if at + MIN_MATCH <= data.len() {
                self.head3[hash(data, at)] = at as u32;
            }
            if at + 4 <= data.len() {
                let hash = hash4(data, at);
                self.prev[at & (WINDOW - 1)] = self.head[hash];
                self.head[hash] = at as u32;
            }
            self.inserted += 1;
        }
        let max = (data.len() - pos).min(MAX_MATCH);
        let mut best = shorter.max(MIN_MATCH - 1);
        if best >= max {
            return (0, 0);
        }
        let current = &data[pos..pos + max];
        let mut best_dist = 0;
        // The two bytes ending at `best` must match for a candidate to be longer.
        let mut tail = u16::from_le_bytes([current[best - 1], current[best]]);
        // Distances run from 1 up to the window size or the start of the data.
        let in_window = |dist: usize| dist.wrapping_sub(1) < WINDOW.min(pos);
        // Tries the match `dist` back, returning whether it ends the search.
        let mut try_match = |dist: usize| {
            let start = pos - dist;
            let ends = &data[start + best - 1..start + best + 1];
            if u16::from_le_bytes([ends[0], ends[1]]) != tail {
                return false;
            }
            let len = common_prefix(&data[start..], current, max);
            if len <= best {
                return false;
            }
            (best, best_dist) = (len, dist);
            if len >= nice || len == max {
                return true;
            }
            tail = u16::from_le_bytes([current[best - 1], current[best]]);
            false
        };
        if shorter < MIN_MATCH {
            let dist = (pos as u32).wrapping_sub(self.head3[hash(data, pos)]) as usize;
            if in_window(dist) && try_match(dist) {
                return (best, best_dist);
            }
        }
        let mut candidate = if max >= 4 {
            self.head[hash4(data, pos)]
        } else {
            u32::MAX
        };
        for _ in 0..depth {
            let dist = (pos as u32).wrapping_sub(candidate) as usize;
            if !in_window(dist) || try_match(dist) {
                break;
            }
            candidate = self.prev[(pos - dist) & (WINDOW - 1)];
        }
        // Short matches far away usually cost more than their literals.
        if best_dist == 0 || (best == MIN_MATCH && best_dist > 8192) {
            (0, 0)
        } else {
            (best, best_dist)
        }
    }
}

/// Binary trees over a sliding window that yield every useful match length, for optimal parsing.
struct BinaryTrees {
    head: Vec<usize>,
    child: Vec<usize>,
}

impl BinaryTrees {
    fn new() -> Self {
        Self {
            head: vec![NONE; 1 << HASH_BITS],
            child: vec![NONE; 2 * WINDOW],
        }
    }

    /// Inserts `pos`, which must follow the previous position, and appends each longer
    /// match found to `out`. Returns the longest length.
    fn advance(&mut self, data: &[u8], pos: usize, p: &Params, out: &mut Vec<Symbol>) -> usize {
        if pos + MIN_MATCH > data.len() {
            return 0;
        }
        let max = (data.len() - pos).min(MAX_MATCH);
        let nice = p.nice.min(max);
        let hash = hash(data, pos);
        let mut node = self.head[hash];
        self.head[hash] = pos;
        // Slots still waiting for the subtrees of nodes smaller and greater than `pos`.
        let (mut less, mut greater) = (2 * (pos % WINDOW), 2 * (pos % WINDOW) + 1);
        let (mut less_len, mut greater_len, mut len, mut best) = (0, 0, 0, MIN_MATCH - 1);
        for _ in 0..p.depth {
            // A node a full window back shares the slots of `pos`, which are being rewritten.
            if node == NONE || pos - node >= WINDOW {
                break;
            }
            if data[node + len] == data[pos + len] {
                len += 1 + common_prefix(
                    &data[node + len + 1..],
                    &data[pos + len + 1..],
                    max - len - 1,
                );
                if len > best {
                    best = len;
                    out.push(Symbol {
                        len: len as u16,
                        dist: (pos - node) as u16,
                    });
                }
                if len >= nice {
                    let slot = 2 * (node % WINDOW);
                    self.child[less] = self.child[slot];
                    self.child[greater] = self.child[slot + 1];
                    return best;
                }
            }
            let slot = 2 * (node % WINDOW);
            if data[node + len] < data[pos + len] {
                self.child[less] = node;
                less = slot + 1;
                less_len = len;
                len = len.min(greater_len);
                node = self.child[less];
            } else {
                self.child[greater] = node;
                greater = slot;
                greater_len = len;
                len = len.min(less_len);
                node = self.child[greater];
            }
        }
        self.child[less] = NONE;
        self.child[greater] = NONE;
        if best >= MIN_MATCH { best } else { 0 }
    }
}

// MARK: Parsing
fn parse_lazy(
    chains: &mut HashChains,
    data: &[u8],
    start: usize,
    end: usize,
    p: &Params,
) -> Vec<Symbol> {
    let mut symbols = Vec::with_capacity((end - start) / 2);
    let mut pending = None;
    let mut pos = start;
    while pos < end {
        let (len, dist) = match pending.take() {
            Some(found) => found,
            None => chains.find(data, pos, 0, p.depth, p.nice),
        };
        let len = len.min(end - pos);
        if len < MIN_MATCH {
            symbols.push(Symbol::literal(data[pos]));
            pos += 1;
            continue;
        }
        if len < p.lazy && pos + 1 < end {
            let next = chains.find(data, pos + 1, len, p.depth, p.nice);
            if next.0.min(end - pos - 1) > len {
                symbols.push(Symbol::literal(data[pos]));
                pending = Some(next);
                pos += 1;
                continue;
            }
        }
        symbols.push(Symbol {
            len: len as u16,
            dist: dist as u16,
        });
        pos += len;
    }
    symbols
}

/// Matches for every position of a segment, sorted by increasing length.
struct Matches {
    starts: Vec<u32>,
    list: Vec<Symbol>,
}

impl Matches {
    fn at(&self, index: usize) -> &[Symbol] {
        &self.list[self.starts[index] as usize..self.starts[index + 1] as usize]
    }
}

fn find_matches(
    trees: &mut BinaryTrees,
    data: &[u8],
    start: usize,
    end: usize,
    p: &Params,
) -> Matches {
    let mut starts = Vec::with_capacity(end - start + 1);
    let mut list = Vec::with_capacity(end - start);
    let mut skip_until = start;
    for pos in start..end {
        starts.push(list.len() as u32);
        let first = list.len();
        let longest = trees.advance(data, pos, p, &mut list);
        if pos < skip_until {
            // Inside a long match keep only the longest continuation.
            let longest = (list.len() > first).then(|| list[list.len() - 1]);
            list.truncate(first);
            list.extend(longest);
            continue;
        }
        // A shorter length can use the nearest distance of any longer match.
        let mut nearest = u16::MAX;
        for found in list[first..].iter_mut().rev() {
            nearest = nearest.min(found.dist);
            found.dist = nearest;
        }
        if longest >= p.nice {
            skip_until = pos + longest;
        }
    }
    starts.push(list.len() as u32);
    Matches { starts, list }
}

fn log2(x: f32) -> f32 {
    // Exponent plus a quadratic fit of the mantissa, accurate to about 0.005.
    let bits = x.to_bits();
    let exponent = ((bits >> 23) & 0xff) as i32 - 128;
    let mantissa = f32::from_bits(bits & 0x007f_ffff | 0x3f80_0000);
    exponent as f32 + (-0.344_848_43 * mantissa + 2.024_665_8) * mantissa - 0.674_877_6
}

/// Estimated bits per symbol, from the entropy of a previous parse.
struct Costs {
    literal: [f32; 256],
    len: [f32; MAX_MATCH + 1],
    dist: [f32; 30],
}

/// Bits per symbol under the entropy of `freqs`.
fn entropy<const N: usize>(freqs: &[u32; N]) -> [f32; N] {
    let total = log2(freqs.iter().sum::<u32>().max(1) as f32);
    // Unused symbols cost a bit more than a symbol seen once.
    freqs.map(|freq| {
        if freq == 0 {
            total + 1.0
        } else {
            total - log2(freq as f32)
        }
    })
}

impl Costs {
    fn new(histogram: &Histogram) -> Self {
        let litlen = entropy(&histogram.litlen);
        let dist = entropy(&histogram.dist);
        let mut costs = Self {
            literal: [0.0; 256],
            len: [0.0; MAX_MATCH + 1],
            dist: [0.0; 30],
        };
        costs.literal.copy_from_slice(&litlen[..256]);
        for (cost, &symbol) in costs.len.iter_mut().zip(&LEN_SYMBOL).skip(MIN_MATCH) {
            *cost = litlen[257 + symbol as usize] + f32::from(LEN_EXTRA[symbol as usize]);
        }
        for (cost, (&bits, extra)) in costs.dist.iter_mut().zip(dist.iter().zip(DIST_EXTRA)) {
            *cost = bits + f32::from(extra);
        }
        costs
    }
}

/// Finds the cheapest parse of `data[start..end]` under `costs` by dynamic programming.
fn parse_optimal(
    data: &[u8],
    matches: &Matches,
    offset: usize,
    start: usize,
    end: usize,
    costs: &Costs,
    span: usize,
) -> Vec<Symbol> {
    let n = end - start;
    let mut cost = vec![0f32; n + 1];
    let mut choice = vec![Symbol::literal(0); n];
    for i in (0..n).rev() {
        let mut best = costs.literal[data[start + i] as usize] + cost[i + 1];
        let mut chosen = Symbol::literal(data[start + i]);
        let mut len = MIN_MATCH;
        let cap = n - i;
        for found in matches.at(start - offset + i) {
            let longest = (found.len as usize).min(cap);
            if len > longest {
                continue;
            }
            // Lengths up to this match share its distance, so only their sum varies. Long
            // ranges try only their shortest and longest lengths, which bounds the time spent
            // in long runs with little loss.
            let (head, tail) = if longest - len > 2 * span {
                (len + span, longest + 1 - span)
            } else {
                (longest + 1, longest + 1)
            };
            let (mut shortest, mut at) = (f32::INFINITY, 0);
            for l in (len..head).chain(tail..=longest) {
                if costs.len[l] + cost[i + l] < shortest {
                    (shortest, at) = (costs.len[l] + cost[i + l], l);
                }
            }
            let total = shortest + costs.dist[dist_symbol(found.dist as usize)];
            if total < best {
                best = total;
                chosen = Symbol {
                    len: at as u16,
                    dist: found.dist,
                };
            }
            len = longest + 1;
        }
        cost[i] = best;
        choice[i] = chosen;
    }
    let mut symbols = Vec::new();
    let mut i = 0;
    while i < n {
        symbols.push(choice[i]);
        i += choice[i].bytes();
    }
    symbols
}

/// Refines the parse of one range, keeping the smallest result including `initial`.
fn optimize(
    data: &[u8],
    matches: &Matches,
    offset: usize,
    start: usize,
    end: usize,
    initial: Vec<Symbol>,
    p: &Params,
) -> Vec<Symbol> {
    let mut best_bits = block_bits(&Histogram::new(&initial));
    let mut current = initial.clone();
    let mut best = initial;
    for _ in 0..p.passes {
        current = parse_optimal(
            data,
            matches,
            offset,
            start,
            end,
            &Costs::new(&Histogram::new(&current)),
            p.span,
        );
        let bits = block_bits(&Histogram::new(&current));
        if bits < best_bits {
            best_bits = bits;
            best.clone_from(&current);
        }
    }
    best
}

// MARK: Block splitting
const SPLIT_GRAIN: usize = 512;

/// Finds a local minimum of `cost` over split points strictly between `low` and `high`.
fn find_minimum(
    cost: &mut impl FnMut(usize) -> u64,
    mut low: usize,
    mut high: usize,
) -> (usize, u64) {
    let mut best = (low, u64::MAX);
    loop {
        if high - low <= 10 {
            for point in low + 1..high {
                let value = cost(point);
                if value < best.1 {
                    best = (point, value);
                }
            }
            return best;
        }
        let step = (high - low) / 10;
        let (mut index, mut value) = (0, u64::MAX);
        for k in 1..10 {
            let v = cost(low + k * step);
            if v < value {
                (index, value) = (k, v);
            }
        }
        if value < best.1 {
            best = (low + index * step, value);
        }
        (low, high) = (
            low + (index - 1) * step,
            (low + (index + 1) * step).min(high),
        );
    }
}

/// Splits symbols into blocks where separate Huffman codes save more than their headers cost.
/// Returns block boundaries as symbol indices, starting at 0 and ending at `symbols.len()`.
fn split_blocks(symbols: &[Symbol], max_blocks: usize, exact: bool) -> Vec<usize> {
    let grains = symbols.len().div_ceil(SPLIT_GRAIN);
    if max_blocks < 2 || grains < 2 {
        return vec![0, symbols.len()];
    }
    // Cumulative histograms per grain make any range histogram a subtraction.
    let mut prefix = vec![Histogram::EMPTY; grains + 1];
    for (grain, chunk) in symbols.chunks(SPLIT_GRAIN).enumerate() {
        let mut counts = prefix[grain].clone();
        for &symbol in chunk {
            counts.add(symbol);
        }
        prefix[grain + 1] = counts;
    }
    let cost = |a: usize, b: usize| {
        let (from, to) = (&prefix[a], &prefix[b]);
        let mut histogram = Histogram {
            litlen: core::array::from_fn(|i| to.litlen[i] - from.litlen[i]),
            dist: core::array::from_fn(|i| to.dist[i] - from.dist[i]),
        };
        histogram.litlen[256] = 1;
        if exact {
            block_bits(&histogram)
        } else {
            estimated_bits(&histogram)
        }
    };
    let mut points = vec![0, grains];
    let mut done = vec![false];
    while points.len() <= max_blocks {
        let mut best: Option<(usize, usize, u64)> = None;
        for block in 0..points.len() - 1 {
            let (a, b) = (points[block], points[block + 1]);
            if done[block] || b - a < 2 {
                done[block] = true;
                continue;
            }
            let whole = cost(a, b);
            let (point, split) = find_minimum(&mut |k| cost(a, k) + cost(k, b), a, b);
            if split < whole {
                if best.is_none_or(|(_, _, gain)| whole - split > gain) {
                    best = Some((block, point, whole - split));
                }
            } else {
                done[block] = true;
            }
        }
        let Some((block, point, _)) = best else {
            break;
        };
        points.insert(block + 1, point);
        done.insert(block + 1, false);
    }
    points
        .iter()
        .map(|&grain| (grain * SPLIT_GRAIN).min(symbols.len()))
        .collect()
}

// MARK: Compression
/// Number of input bytes covered by `symbols`.
fn symbols_len(symbols: &[Symbol]) -> usize {
    symbols.iter().map(|symbol| symbol.bytes()).sum()
}

fn emit(
    writer: &mut BitWriter,
    data: &[u8],
    start: usize,
    symbols: &[Symbol],
    points: &[usize],
    last: bool,
) {
    let mut pos = start;
    for (index, block) in points.windows(2).enumerate() {
        let block = &symbols[block[0]..block[1]];
        let len = symbols_len(block);
        write_block(
            writer,
            block,
            &data[pos..pos + len],
            last && index + 2 == points.len(),
        );
        pos += len;
    }
}

fn compress_optimal(writer: &mut BitWriter, data: &[u8], p: &Params) {
    let mut trees = BinaryTrees::new();
    let mut start = 0;
    while start < data.len() {
        let end = (start + p.segment).min(data.len());
        let matches = find_matches(&mut trees, data, start, end, p);
        // Start from the longest match at each position, then let costs refine the parse.
        let mut initial = Vec::new();
        let mut pos = start;
        while pos < end {
            let symbol = matches
                .at(pos - start)
                .last()
                .map(|&found| Symbol {
                    len: (found.len as usize).min(end - pos) as u16,
                    dist: found.dist,
                })
                .filter(|symbol| symbol.len as usize >= MIN_MATCH)
                .unwrap_or(Symbol::literal(data[pos]));
            initial.push(symbol);
            pos += symbol.bytes();
        }
        let mut symbols = optimize(data, &matches, start, start, end, initial, p);
        let mut points = vec![0, symbols.len()];
        for _ in 0..p.rounds {
            let split = split_blocks(&symbols, p.max_blocks, true);
            let mut next = Vec::with_capacity(symbols.len());
            points = vec![0];
            let mut pos = start;
            for block in split.windows(2) {
                let block = &symbols[block[0]..block[1]];
                let len = symbols_len(block);
                next.extend(optimize(
                    data,
                    &matches,
                    start,
                    pos,
                    pos + len,
                    block.to_vec(),
                    p,
                ));
                points.push(next.len());
                pos += len;
            }
            symbols = next;
        }
        emit(writer, data, start, &symbols, &points, end == data.len());
        start = end;
    }
}

fn compress_lazy(writer: &mut BitWriter, data: &[u8], p: &Params) {
    let mut chains = HashChains::new();
    let mut start = 0;
    while start < data.len() {
        let end = (start + p.segment).min(data.len());
        let symbols = parse_lazy(&mut chains, data, start, end, p);
        let points = split_blocks(&symbols, p.max_blocks, false);
        emit(writer, data, start, &symbols, &points, end == data.len());
        start = end;
    }
}

/// Appends the raw DEFLATE stream for `data` to `out`.
pub(crate) fn compress(data: &[u8], level: Level, out: &mut Vec<u8>) {
    let p = params(level);
    let mut writer = BitWriter {
        out,
        bits: 0,
        count: 0,
    };
    if data.is_empty() {
        // A final fixed Huffman block holding only the end code.
        writer.put(3, 3);
        writer.put(0, 7);
    } else if p.passes > 0 {
        compress_optimal(&mut writer, data, &p);
    } else {
        compress_lazy(&mut writer, data, &p);
    }
    writer.align();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cost(freqs: &[u32], lens: &[u8]) -> u64 {
        freqs
            .iter()
            .zip(lens)
            .map(|(&f, &l)| u64::from(f) * u64::from(l))
            .sum()
    }

    #[test]
    fn huffman_matches_package_merge() {
        let mut state = 7u32;
        for round in 0..500 {
            let size = 2 + round % 300;
            let freqs: Vec<u32> = (0..size)
                .map(|_| {
                    state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
                    // Skewed weights with some unused symbols.
                    ((state >> 16) % 1000) >> (state % 11)
                })
                .collect();
            let mut fast = vec![0; size];
            let mut reference = vec![0; size];
            code_lengths(&freqs, 15, &mut fast);
            let mut leaves: Vec<(u32, u16)> = freqs
                .iter()
                .enumerate()
                .filter(|&(_, &f)| f > 0)
                .map(|(s, &f)| (f, s as u16))
                .collect();
            leaves.sort_unstable();
            if leaves.len() >= 2 {
                package_merge(&leaves, 15, &mut reference);
                assert_eq!(cost(&freqs, &fast), cost(&freqs, &reference));
            }
            let kraft: f64 = fast
                .iter()
                .filter(|&&l| l > 0)
                .map(|&l| 0.5f64.powi(l.into()))
                .sum();
            assert!(fast.iter().all(|&l| l <= 15) && (kraft - 1.0).abs() < 1e-9);
        }
    }

    #[test]
    fn limits_code_lengths() {
        // Fibonacci weights need codes far longer than 7 bits without a limit.
        let mut freqs = [1u32; 19];
        for i in 2..19 {
            freqs[i] = freqs[i - 1] + freqs[i - 2];
        }
        let mut lens = [0; 19];
        code_lengths(&freqs, 7, &mut lens);
        let kraft: f64 = lens.iter().map(|&l| 0.5f64.powi(l.into())).sum();
        assert!(lens.iter().all(|&l| (1..=7).contains(&l)) && (kraft - 1.0).abs() < 1e-9);
    }
}
