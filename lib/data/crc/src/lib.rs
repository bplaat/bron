/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [crc](https://crates.io/crates/crc) crate

#![no_std]

// The bytes of a sparse reduction step, up to the closest of its offsets.
const SPARSE_CHUNK: usize = 1920;
// The distances from a byte to the bytes that take its place, the largest being the reach.
const SPARSE_OFFSETS: [usize; 3] = [1927, 2181, SPARSE_REACH];
const SPARSE_REACH: usize = 4018;
// The bytes pending for later chunks, a whole number of chunks that holds a chunk and the reach.
const SPARSE_RING: usize = 4 * SPARSE_CHUNK;
// The input length where sparse reduction overtakes the tables, which handle its last bytes.
const SPARSE_MINIMUM: usize = 12 * 1024;

// MARK: Algorithm
/// CRC algorithm parameters.
#[derive(Clone, Copy, Debug)]
pub struct Algorithm<W> {
    /// Width in bits.
    pub width: u8,
    /// Generator polynomial.
    pub poly: W,
    /// Initial register value.
    pub init: W,
    /// Whether input bytes are reflected.
    pub refin: bool,
    /// Whether the output is reflected.
    pub refout: bool,
    /// Value XORed into the final register.
    pub xorout: W,
    /// Checksum of `b"123456789"`.
    pub check: W,
    /// Register value after processing a correct codeword.
    pub residue: W,
}

/// CRC-32/ISO-HDLC, used by gzip, PNG and ZIP.
pub const CRC_32_ISO_HDLC: Algorithm<u32> = Algorithm {
    width: 32,
    poly: 0x04c1_1db7,
    init: 0xffff_ffff,
    refin: true,
    refout: true,
    xorout: 0xffff_ffff,
    check: 0xcbf4_3926,
    residue: 0xdebb_20e3,
};

// MARK: Crc
/// CRC calculator with precomputed slicing-by-8 tables.
pub struct Crc<W: 'static> {
    /// Algorithm parameters.
    pub algorithm: &'static Algorithm<W>,
    // `table[k][i]` advances the CRC of byte `i` by `k` zero bytes.
    table: [[W; 256]; 8],
}

impl Crc<u32> {
    /// Create a calculator for a reflected 32-bit algorithm.
    pub const fn new(algorithm: &'static Algorithm<u32>) -> Self {
        assert!(
            algorithm.width == 32 && algorithm.refin && algorithm.refout,
            "only reflected 32-bit algorithms are supported"
        );
        let poly = algorithm.poly.reverse_bits();
        let mut table = [[0; 256]; 8];
        let mut i = 0;
        while i < 256 {
            let mut crc = i as u32;
            let mut bit = 0;
            while bit < 8 {
                crc = (crc >> 1) ^ (poly & (crc & 1).wrapping_neg());
                bit += 1;
            }
            table[0][i] = crc;
            i += 1;
        }
        let mut k = 1;
        while k < 8 {
            let mut i = 0;
            while i < 256 {
                let previous = table[k - 1][i];
                table[k][i] = (previous >> 8) ^ table[0][(previous & 0xff) as usize];
                i += 1;
            }
            k += 1;
        }
        Self { algorithm, table }
    }

    /// Compute the checksum of `bytes`.
    pub fn checksum(&self, bytes: &[u8]) -> u32 {
        let mut digest = self.digest();
        digest.update(bytes);
        digest.finalize()
    }

    /// Start an incremental checksum.
    pub const fn digest(&self) -> Digest<'_, u32> {
        Digest {
            crc: self,
            value: self.algorithm.init.reverse_bits(),
        }
    }

    fn update(&self, crc: u32, bytes: &[u8]) -> u32 {
        if self.algorithm.poly == CRC_32_ISO_HDLC.poly && bytes.len() >= SPARSE_MINIMUM {
            return self.update_sparse(crc, bytes);
        }
        self.update_software(crc, bytes)
    }

    // Shortens the message without changing its remainder: with `y` as x^8, the polynomial
    // divides y^4018 + y^2091 + y^1837 + 1, so a byte equals itself XORed onto the three bytes
    // at `SPARSE_OFFSETS` after it. Those are further away than a chunk, which leaves every
    // byte of a chunk independent, unlike the carried register of a table lookup.
    fn update_sparse(&self, crc: u32, bytes: &[u8]) -> u32 {
        let mut pending = [0u8; SPARSE_RING];
        pending[..4].copy_from_slice(&crc.to_le_bytes());
        let chunks = (bytes.len() - SPARSE_REACH) / SPARSE_CHUNK;
        let (body, rest) = bytes.split_at(chunks * SPARSE_CHUNK);
        for (index, chunk) in body.as_chunks::<SPARSE_CHUNK>().0.iter().enumerate() {
            let start = index * SPARSE_CHUNK % SPARSE_RING;
            let mut value = *chunk;
            xor(&mut value, &pending[start..start + SPARSE_CHUNK]);
            pending[start..start + SPARSE_CHUNK].fill(0);
            for offset in SPARSE_OFFSETS {
                let at = (start + offset) % SPARSE_RING;
                let (head, tail) = value.split_at((SPARSE_RING - at).min(SPARSE_CHUNK));
                xor(&mut pending[at..], head);
                xor(&mut pending, tail);
            }
        }
        let start = chunks * SPARSE_CHUNK % SPARSE_RING;
        let (head, tail) = rest.split_at(SPARSE_REACH);
        let mut remainder = [0; SPARSE_REACH];
        remainder.copy_from_slice(head);
        let (before_wrap, after_wrap) =
            remainder.split_at_mut((SPARSE_RING - start).min(SPARSE_REACH));
        xor(before_wrap, &pending[start..]);
        xor(after_wrap, &pending);
        self.update_software(self.update_software(0, &remainder), tail)
    }

    fn update_software(&self, mut crc: u32, bytes: &[u8]) -> u32 {
        let t = &self.table;
        let (chunks, rest) = bytes.as_chunks::<8>();
        for chunk in chunks {
            let low = crc ^ u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            let high = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
            crc = t[7][(low & 0xff) as usize]
                ^ t[6][(low >> 8 & 0xff) as usize]
                ^ t[5][(low >> 16 & 0xff) as usize]
                ^ t[4][(low >> 24) as usize]
                ^ t[3][(high & 0xff) as usize]
                ^ t[2][(high >> 8 & 0xff) as usize]
                ^ t[1][(high >> 16 & 0xff) as usize]
                ^ t[0][(high >> 24) as usize];
        }
        for &byte in rest {
            crc = t[0][((crc as u8) ^ byte) as usize] ^ (crc >> 8);
        }
        crc
    }
}

// XORs a slice onto the start of another, in a loop that has no dependency between bytes.
fn xor(dest: &mut [u8], source: &[u8]) {
    for (dest, source) in dest.iter_mut().zip(source) {
        *dest ^= source;
    }
}

// MARK: Digest
/// Incremental CRC computation.
pub struct Digest<'a, W: 'static> {
    crc: &'a Crc<W>,
    value: W,
}

impl Digest<'_, u32> {
    /// Feed `bytes` into the checksum.
    pub fn update(&mut self, bytes: &[u8]) {
        self.value = self.crc.update(self.value, bytes);
    }

    /// Finish and return the checksum.
    pub const fn finalize(self) -> u32 {
        self.value ^ self.crc.algorithm.xorout
    }
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_reduction_matches_software_at_its_edges() {
        let mut data = [0u8; 70000];
        let mut state = 0x2545_f491u32;
        for byte in &mut data {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            *byte = state as u8;
        }
        for offset in 0..16 {
            for length in [
                4018, 4019, 5937, 5938, 5939, 7857, 7858, 7859, 9777, 9778, 11697, 13618, 13619,
                15360, 15361, 69000,
            ] {
                let bytes = &data[offset..offset + length];
                for initial in [0, u32::MAX, 0x1234_5678] {
                    assert_eq!(
                        CRC32.update_sparse(initial, bytes),
                        CRC32.update_software(initial, bytes),
                        "offset={offset}, length={length}"
                    );
                }
            }
        }
    }

    #[test]
    fn bulk_update_matches_software_for_alignments_and_registers() {
        let mut data = [0u8; 70000];
        for (index, byte) in data.iter_mut().enumerate() {
            *byte = (index * 73 + 11) as u8;
        }
        const CASTAGNOLI: Algorithm<u32> = Algorithm {
            poly: 0x1edc_6f41,
            ..CRC_32_ISO_HDLC
        };
        let other = Crc::<u32>::new(&CASTAGNOLI);
        for calculator in [&CRC32, &other] {
            for offset in 0..16 {
                for length in [
                    0, 1, 7, 8, 15, 16, 31, 32, 63, 64, 65, 79, 80, 81, 95, 96, 111, 112, 127, 128,
                    129, 255, 256, 1024, 5937, 5938, 5939, 7857, 7858, 7859, 15360, 15361, 69000,
                ] {
                    let bytes = &data[offset..offset + length];
                    for initial in [0, u32::MAX, 0x1234_5678] {
                        assert_eq!(
                            calculator.update(initial, bytes),
                            calculator.update_software(initial, bytes)
                        );
                    }
                }
            }
        }
    }

    const CRC32: Crc<u32> = Crc::<u32>::new(&CRC_32_ISO_HDLC);

    #[test]
    fn test_check() {
        assert_eq!(CRC32.checksum(b"123456789"), 0xcbf4_3926);
        assert_eq!(CRC32.checksum(b"123456789"), CRC_32_ISO_HDLC.check);
        assert_eq!(CRC32.checksum(b""), 0);
    }

    #[test]
    fn test_digest_matches_checksum() {
        let mut data = [0u8; 1000];
        for (i, byte) in data.iter_mut().enumerate() {
            *byte = (i * 37) as u8;
        }
        for split in [0, 1, 7, 8, 9, 500, 999, 1000] {
            let mut digest = CRC32.digest();
            digest.update(&data[..split]);
            digest.update(&data[split..]);
            assert_eq!(digest.finalize(), CRC32.checksum(&data), "split={split}");
        }
    }

    #[test]
    fn test_matches_bitwise() {
        let mut data = [0u8; 64];
        for (i, byte) in data.iter_mut().enumerate() {
            *byte = (i * 73 + 11) as u8;
        }
        for len in 0..data.len() {
            let mut crc = u32::MAX;
            for &byte in &data[..len] {
                crc ^= u32::from(byte);
                for _ in 0..8 {
                    crc = (crc >> 1) ^ (0xedb8_8320 & (crc & 1).wrapping_neg());
                }
            }
            assert_eq!(CRC32.checksum(&data[..len]), !crc, "len={len}");
        }
    }
}
