/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [crc](https://crates.io/crates/crc) crate

#![no_std]

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

    fn update(&self, mut crc: u32, bytes: &[u8]) -> u32 {
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
