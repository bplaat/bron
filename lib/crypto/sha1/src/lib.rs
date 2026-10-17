/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [sha1](https://crates.io/crates/sha1) crate
//!
//! Disable default features for `no_std` support. CPU acceleration then uses compile-time
//! target features instead of runtime detection.

#![cfg_attr(not(any(test, feature = "std")), no_std)]

/// SHA-1 hasher
pub struct Sha1 {
    state: [u32; 5],
    buffer: [u8; 64],
    length: u64,
}

impl Default for Sha1 {
    fn default() -> Self {
        Self {
            state: [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0],
            buffer: [0; 64],
            length: 0,
        }
    }
}

impl Sha1 {
    /// Create a new SHA-1 hasher
    pub fn new() -> Self {
        Self::default()
    }

    /// Compute the SHA-1 digest of the given data
    pub fn digest(data: impl AsRef<[u8]>) -> [u8; 20] {
        let mut h = Self::new();
        h.update(data);
        h.finalize()
    }

    /// Update the hasher with new data
    pub fn update(&mut self, data: impl AsRef<[u8]>) {
        let mut data = data.as_ref();
        let input_len = data.len();
        let buffer_len = self.length as usize % 64;
        if buffer_len != 0 {
            let to_copy = (64 - buffer_len).min(data.len());
            self.buffer[buffer_len..buffer_len + to_copy].copy_from_slice(&data[..to_copy]);
            data = &data[to_copy..];
            if buffer_len + to_copy == 64 {
                let block = self.buffer;
                self.process_blocks(core::slice::from_ref(&block));
            }
        }
        let (blocks, remaining) = data.as_chunks::<64>();
        if !blocks.is_empty() {
            self.process_blocks(blocks);
        }
        data = remaining;
        self.buffer[..data.len()].copy_from_slice(data);
        self.length += input_len as u64;
    }

    /// Finalize the hash and return the digest
    pub fn finalize(mut self) -> [u8; 20] {
        self.finalize_inner()
    }

    /// Finalize the hash, reset the hasher, and return the digest
    pub fn finalize_reset(&mut self) -> [u8; 20] {
        let result = self.finalize_inner();
        self.reset();
        result
    }

    fn finalize_inner(&mut self) -> [u8; 20] {
        let length_bits = self.length * 8;
        let used = self.length as usize % 64;
        let mut block = [0u8; 64];
        if used == 0 {
            block[0] = 0x80;
        } else {
            block[..used].copy_from_slice(&self.buffer[..used]);
            block[used] = 0x80;
            if used >= 56 {
                self.process_blocks(core::slice::from_ref(&block));
                block.fill(0);
            }
        }
        block[56..].copy_from_slice(&length_bits.to_be_bytes());
        self.process_blocks(core::slice::from_ref(&block));
        let mut result = [0u8; 20];
        for (i, chunk) in result.chunks_mut(4).enumerate() {
            chunk.copy_from_slice(&self.state[i].to_be_bytes());
        }
        result
    }

    fn reset(&mut self) {
        *self = Self::default();
    }

    #[allow(unsafe_code)]
    fn process_blocks(&mut self, blocks: &[[u8; 64]]) {
        if blocks.is_empty() {
            return;
        }
        if cfg!(all(
            feature = "disable-simd",
            not(feature = "disable-software")
        )) {
            for block in blocks {
                self.process_block_software(block);
            }
            return;
        }
        cfg_select! {
            target_arch = "x86_64" => {
                if has_sha_acceleration() {
                    for block in blocks {
                        // SAFETY: CPU feature detection confirmed every required CPU feature.
                        unsafe { self.process_block_x86_sha(block) }
                    }
                    return;
                }
            }
            target_arch = "aarch64" => {
                if has_sha_acceleration() {
                    for block in blocks {
                        // SAFETY: CPU feature detection confirmed the sha2 CPU feature.
                        unsafe { self.process_block_aarch64_sha(block) }
                    }
                    return;
                }
            }
            _ => {}
        }
        if cfg!(all(
            feature = "disable-software",
            not(feature = "disable-simd")
        )) {
            panic!("SHA-1 SIMD acceleration is unavailable");
        }
        for block in blocks {
            self.process_block_software(block);
        }
    }

    // MARK: Software
    fn process_block_software(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 80];
        for (i, chunk) in block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let mut a = self.state[0];
        let mut b = self.state[1];
        let mut c = self.state[2];
        let mut d = self.state[3];
        let mut e = self.state[4];

        for (i, w_i) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                60..=79 => (b ^ c ^ d, 0xCA62C1D6),
                _ => unreachable!(),
            };

            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*w_i);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
    }

    // MARK: x86_64 SHA-NI
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "sha,sse2,ssse3,sse4.1")]
    #[allow(unsafe_code)]
    unsafe fn process_block_x86_sha(&mut self, block: &[u8; 64]) {
        // SAFETY: The caller verified sha/sse2/ssse3/sse4.1 feature availability; self.state and block are properly initialized.
        unsafe {
            use core::arch::x86_64::*;

            let mask = _mm_set_epi64x(0x0001_0203_0405_0607, 0x0809_0a0b_0c0d_0e0f_u64 as i64);

            let mut abcd = _mm_loadu_si128(self.state.as_ptr().cast());
            let mut e0 = _mm_set_epi32(self.state[4] as i32, 0, 0, 0);
            abcd = _mm_shuffle_epi32(abcd, 0x1B);
            let abcd_save = abcd;
            let e0_save = e0;

            let mut msg0 = _mm_shuffle_epi8(_mm_loadu_si128(block.as_ptr().cast()), mask);
            let mut msg1 = _mm_shuffle_epi8(_mm_loadu_si128(block.as_ptr().add(16).cast()), mask);
            let mut msg2 = _mm_shuffle_epi8(_mm_loadu_si128(block.as_ptr().add(32).cast()), mask);
            let mut msg3 = _mm_shuffle_epi8(_mm_loadu_si128(block.as_ptr().add(48).cast()), mask);

            let mut e1;

            // Rounds 0-3
            e0 = _mm_add_epi32(e0, msg0);
            e1 = abcd;
            abcd = _mm_sha1rnds4_epu32(abcd, e0, 0);

            // Rounds 4-7
            e1 = _mm_sha1nexte_epu32(e1, msg1);
            e0 = abcd;
            abcd = _mm_sha1rnds4_epu32(abcd, e1, 0);
            msg0 = _mm_sha1msg1_epu32(msg0, msg1);

            // Rounds 8-11
            e0 = _mm_sha1nexte_epu32(e0, msg2);
            e1 = abcd;
            abcd = _mm_sha1rnds4_epu32(abcd, e0, 0);
            msg1 = _mm_sha1msg1_epu32(msg1, msg2);
            msg0 = _mm_xor_si128(msg0, msg2);

            // Rounds 12-15
            e1 = _mm_sha1nexte_epu32(e1, msg3);
            e0 = abcd;
            msg0 = _mm_sha1msg2_epu32(msg0, msg3);
            abcd = _mm_sha1rnds4_epu32(abcd, e1, 0);
            msg2 = _mm_sha1msg1_epu32(msg2, msg3);
            msg1 = _mm_xor_si128(msg1, msg3);

            // Rounds 16-19
            e0 = _mm_sha1nexte_epu32(e0, msg0);
            e1 = abcd;
            msg1 = _mm_sha1msg2_epu32(msg1, msg0);
            abcd = _mm_sha1rnds4_epu32(abcd, e0, 0);
            msg3 = _mm_sha1msg1_epu32(msg3, msg0);
            msg2 = _mm_xor_si128(msg2, msg0);

            // Rounds 20-23
            e1 = _mm_sha1nexte_epu32(e1, msg1);
            e0 = abcd;
            msg2 = _mm_sha1msg2_epu32(msg2, msg1);
            abcd = _mm_sha1rnds4_epu32(abcd, e1, 1);
            msg0 = _mm_sha1msg1_epu32(msg0, msg1);
            msg3 = _mm_xor_si128(msg3, msg1);

            // Rounds 24-27
            e0 = _mm_sha1nexte_epu32(e0, msg2);
            e1 = abcd;
            msg3 = _mm_sha1msg2_epu32(msg3, msg2);
            abcd = _mm_sha1rnds4_epu32(abcd, e0, 1);
            msg1 = _mm_sha1msg1_epu32(msg1, msg2);
            msg0 = _mm_xor_si128(msg0, msg2);

            // Rounds 28-31
            e1 = _mm_sha1nexte_epu32(e1, msg3);
            e0 = abcd;
            msg0 = _mm_sha1msg2_epu32(msg0, msg3);
            abcd = _mm_sha1rnds4_epu32(abcd, e1, 1);
            msg2 = _mm_sha1msg1_epu32(msg2, msg3);
            msg1 = _mm_xor_si128(msg1, msg3);

            // Rounds 32-35
            e0 = _mm_sha1nexte_epu32(e0, msg0);
            e1 = abcd;
            msg1 = _mm_sha1msg2_epu32(msg1, msg0);
            abcd = _mm_sha1rnds4_epu32(abcd, e0, 1);
            msg3 = _mm_sha1msg1_epu32(msg3, msg0);
            msg2 = _mm_xor_si128(msg2, msg0);

            // Rounds 36-39
            e1 = _mm_sha1nexte_epu32(e1, msg1);
            e0 = abcd;
            msg2 = _mm_sha1msg2_epu32(msg2, msg1);
            abcd = _mm_sha1rnds4_epu32(abcd, e1, 1);
            msg0 = _mm_sha1msg1_epu32(msg0, msg1);
            msg3 = _mm_xor_si128(msg3, msg1);

            // Rounds 40-43
            e0 = _mm_sha1nexte_epu32(e0, msg2);
            e1 = abcd;
            msg3 = _mm_sha1msg2_epu32(msg3, msg2);
            abcd = _mm_sha1rnds4_epu32(abcd, e0, 2);
            msg1 = _mm_sha1msg1_epu32(msg1, msg2);
            msg0 = _mm_xor_si128(msg0, msg2);

            // Rounds 44-47
            e1 = _mm_sha1nexte_epu32(e1, msg3);
            e0 = abcd;
            msg0 = _mm_sha1msg2_epu32(msg0, msg3);
            abcd = _mm_sha1rnds4_epu32(abcd, e1, 2);
            msg2 = _mm_sha1msg1_epu32(msg2, msg3);
            msg1 = _mm_xor_si128(msg1, msg3);

            // Rounds 48-51
            e0 = _mm_sha1nexte_epu32(e0, msg0);
            e1 = abcd;
            msg1 = _mm_sha1msg2_epu32(msg1, msg0);
            abcd = _mm_sha1rnds4_epu32(abcd, e0, 2);
            msg3 = _mm_sha1msg1_epu32(msg3, msg0);
            msg2 = _mm_xor_si128(msg2, msg0);

            // Rounds 52-55
            e1 = _mm_sha1nexte_epu32(e1, msg1);
            e0 = abcd;
            msg2 = _mm_sha1msg2_epu32(msg2, msg1);
            abcd = _mm_sha1rnds4_epu32(abcd, e1, 2);
            msg0 = _mm_sha1msg1_epu32(msg0, msg1);
            msg3 = _mm_xor_si128(msg3, msg1);

            // Rounds 56-59
            e0 = _mm_sha1nexte_epu32(e0, msg2);
            e1 = abcd;
            msg3 = _mm_sha1msg2_epu32(msg3, msg2);
            abcd = _mm_sha1rnds4_epu32(abcd, e0, 2);
            msg1 = _mm_sha1msg1_epu32(msg1, msg2);
            msg0 = _mm_xor_si128(msg0, msg2);

            // Rounds 60-63
            e1 = _mm_sha1nexte_epu32(e1, msg3);
            e0 = abcd;
            msg0 = _mm_sha1msg2_epu32(msg0, msg3);
            abcd = _mm_sha1rnds4_epu32(abcd, e1, 3);
            msg2 = _mm_sha1msg1_epu32(msg2, msg3);
            msg1 = _mm_xor_si128(msg1, msg3);

            // Rounds 64-67
            e0 = _mm_sha1nexte_epu32(e0, msg0);
            e1 = abcd;
            msg1 = _mm_sha1msg2_epu32(msg1, msg0);
            abcd = _mm_sha1rnds4_epu32(abcd, e0, 3);
            msg3 = _mm_sha1msg1_epu32(msg3, msg0);
            msg2 = _mm_xor_si128(msg2, msg0);

            // Rounds 68-71
            e1 = _mm_sha1nexte_epu32(e1, msg1);
            e0 = abcd;
            msg2 = _mm_sha1msg2_epu32(msg2, msg1);
            abcd = _mm_sha1rnds4_epu32(abcd, e1, 3);
            msg3 = _mm_xor_si128(msg3, msg1);

            // Rounds 72-75
            e0 = _mm_sha1nexte_epu32(e0, msg2);
            e1 = abcd;
            msg3 = _mm_sha1msg2_epu32(msg3, msg2);
            abcd = _mm_sha1rnds4_epu32(abcd, e0, 3);

            // Rounds 76-79
            e1 = _mm_sha1nexte_epu32(e1, msg3);
            e0 = abcd;
            abcd = _mm_sha1rnds4_epu32(abcd, e1, 3);

            // Combine state
            e0 = _mm_sha1nexte_epu32(e0, e0_save);
            abcd = _mm_add_epi32(abcd, abcd_save);

            // Store state
            abcd = _mm_shuffle_epi32(abcd, 0x1B);
            _mm_storeu_si128(self.state.as_mut_ptr().cast(), abcd);
            self.state[4] = _mm_extract_epi32::<3>(e0) as u32;
        }
    }

    // MARK: aarch64 SHA
    #[cfg(target_arch = "aarch64")]
    #[target_feature(enable = "sha2")]
    #[allow(unsafe_code)]
    unsafe fn process_block_aarch64_sha(&mut self, block: &[u8; 64]) {
        // SAFETY: The caller verified sha2 feature availability; self.state and block are properly initialized.
        unsafe {
            use core::arch::aarch64::*;

            // Load state
            let mut abcd = vld1q_u32(self.state.as_ptr());
            let mut e = self.state[4];
            let abcd_save = abcd;
            let e_save = e;

            // Load and byte-swap message words
            let mut msg0 = vreinterpretq_u32_u8(vrev32q_u8(vreinterpretq_u8_u32(vld1q_u32(
                block.as_ptr().cast(),
            ))));
            let mut msg1 = vreinterpretq_u32_u8(vrev32q_u8(vreinterpretq_u8_u32(vld1q_u32(
                block.as_ptr().add(16).cast(),
            ))));
            let mut msg2 = vreinterpretq_u32_u8(vrev32q_u8(vreinterpretq_u8_u32(vld1q_u32(
                block.as_ptr().add(32).cast(),
            ))));
            let mut msg3 = vreinterpretq_u32_u8(vrev32q_u8(vreinterpretq_u8_u32(vld1q_u32(
                block.as_ptr().add(48).cast(),
            ))));

            let mut tmp0;
            let mut tmp1;

            // Rounds 0-3
            tmp0 = vaddq_u32(msg0, vdupq_n_u32(0x5A827999));
            tmp1 = abcd;
            abcd = vsha1cq_u32(abcd, e, tmp0);
            e = vsha1h_u32(vgetq_lane_u32(tmp1, 0));
            tmp0 = vsha1su0q_u32(msg0, msg1, msg2);
            msg0 = vsha1su1q_u32(tmp0, msg3);

            // Rounds 4-7
            tmp0 = vaddq_u32(msg1, vdupq_n_u32(0x5A827999));
            tmp1 = abcd;
            abcd = vsha1cq_u32(abcd, e, tmp0);
            e = vsha1h_u32(vgetq_lane_u32(tmp1, 0));
            tmp0 = vsha1su0q_u32(msg1, msg2, msg3);
            msg1 = vsha1su1q_u32(tmp0, msg0);

            // Rounds 8-11
            tmp0 = vaddq_u32(msg2, vdupq_n_u32(0x5A827999));
            tmp1 = abcd;
            abcd = vsha1cq_u32(abcd, e, tmp0);
            e = vsha1h_u32(vgetq_lane_u32(tmp1, 0));
            tmp0 = vsha1su0q_u32(msg2, msg3, msg0);
            msg2 = vsha1su1q_u32(tmp0, msg1);

            // Rounds 12-15
            tmp0 = vaddq_u32(msg3, vdupq_n_u32(0x5A827999));
            tmp1 = abcd;
            abcd = vsha1cq_u32(abcd, e, tmp0);
            e = vsha1h_u32(vgetq_lane_u32(tmp1, 0));
            tmp0 = vsha1su0q_u32(msg3, msg0, msg1);
            msg3 = vsha1su1q_u32(tmp0, msg2);

            // Rounds 16-19
            tmp0 = vaddq_u32(msg0, vdupq_n_u32(0x5A827999));
            tmp1 = abcd;
            abcd = vsha1cq_u32(abcd, e, tmp0);
            e = vsha1h_u32(vgetq_lane_u32(tmp1, 0));
            tmp0 = vsha1su0q_u32(msg0, msg1, msg2);
            msg0 = vsha1su1q_u32(tmp0, msg3);

            // Rounds 20-23
            tmp0 = vaddq_u32(msg1, vdupq_n_u32(0x6ED9EBA1));
            tmp1 = abcd;
            abcd = vsha1pq_u32(abcd, e, tmp0);
            e = vsha1h_u32(vgetq_lane_u32(tmp1, 0));
            tmp0 = vsha1su0q_u32(msg1, msg2, msg3);
            msg1 = vsha1su1q_u32(tmp0, msg0);

            // Rounds 24-27
            tmp0 = vaddq_u32(msg2, vdupq_n_u32(0x6ED9EBA1));
            tmp1 = abcd;
            abcd = vsha1pq_u32(abcd, e, tmp0);
            e = vsha1h_u32(vgetq_lane_u32(tmp1, 0));
            tmp0 = vsha1su0q_u32(msg2, msg3, msg0);
            msg2 = vsha1su1q_u32(tmp0, msg1);

            // Rounds 28-31
            tmp0 = vaddq_u32(msg3, vdupq_n_u32(0x6ED9EBA1));
            tmp1 = abcd;
            abcd = vsha1pq_u32(abcd, e, tmp0);
            e = vsha1h_u32(vgetq_lane_u32(tmp1, 0));
            tmp0 = vsha1su0q_u32(msg3, msg0, msg1);
            msg3 = vsha1su1q_u32(tmp0, msg2);

            // Rounds 32-35
            tmp0 = vaddq_u32(msg0, vdupq_n_u32(0x6ED9EBA1));
            tmp1 = abcd;
            abcd = vsha1pq_u32(abcd, e, tmp0);
            e = vsha1h_u32(vgetq_lane_u32(tmp1, 0));
            tmp0 = vsha1su0q_u32(msg0, msg1, msg2);
            msg0 = vsha1su1q_u32(tmp0, msg3);

            // Rounds 36-39
            tmp0 = vaddq_u32(msg1, vdupq_n_u32(0x6ED9EBA1));
            tmp1 = abcd;
            abcd = vsha1pq_u32(abcd, e, tmp0);
            e = vsha1h_u32(vgetq_lane_u32(tmp1, 0));
            tmp0 = vsha1su0q_u32(msg1, msg2, msg3);
            msg1 = vsha1su1q_u32(tmp0, msg0);

            // Rounds 40-43
            tmp0 = vaddq_u32(msg2, vdupq_n_u32(0x8F1BBCDC));
            tmp1 = abcd;
            abcd = vsha1mq_u32(abcd, e, tmp0);
            e = vsha1h_u32(vgetq_lane_u32(tmp1, 0));
            tmp0 = vsha1su0q_u32(msg2, msg3, msg0);
            msg2 = vsha1su1q_u32(tmp0, msg1);

            // Rounds 44-47
            tmp0 = vaddq_u32(msg3, vdupq_n_u32(0x8F1BBCDC));
            tmp1 = abcd;
            abcd = vsha1mq_u32(abcd, e, tmp0);
            e = vsha1h_u32(vgetq_lane_u32(tmp1, 0));
            tmp0 = vsha1su0q_u32(msg3, msg0, msg1);
            msg3 = vsha1su1q_u32(tmp0, msg2);

            // Rounds 48-51
            tmp0 = vaddq_u32(msg0, vdupq_n_u32(0x8F1BBCDC));
            tmp1 = abcd;
            abcd = vsha1mq_u32(abcd, e, tmp0);
            e = vsha1h_u32(vgetq_lane_u32(tmp1, 0));
            tmp0 = vsha1su0q_u32(msg0, msg1, msg2);
            msg0 = vsha1su1q_u32(tmp0, msg3);

            // Rounds 52-55
            tmp0 = vaddq_u32(msg1, vdupq_n_u32(0x8F1BBCDC));
            tmp1 = abcd;
            abcd = vsha1mq_u32(abcd, e, tmp0);
            e = vsha1h_u32(vgetq_lane_u32(tmp1, 0));
            tmp0 = vsha1su0q_u32(msg1, msg2, msg3);
            msg1 = vsha1su1q_u32(tmp0, msg0);

            // Rounds 56-59
            tmp0 = vaddq_u32(msg2, vdupq_n_u32(0x8F1BBCDC));
            tmp1 = abcd;
            abcd = vsha1mq_u32(abcd, e, tmp0);
            e = vsha1h_u32(vgetq_lane_u32(tmp1, 0));
            tmp0 = vsha1su0q_u32(msg2, msg3, msg0);
            msg2 = vsha1su1q_u32(tmp0, msg1);

            // Rounds 60-63
            tmp0 = vaddq_u32(msg3, vdupq_n_u32(0xCA62C1D6));
            tmp1 = abcd;
            abcd = vsha1pq_u32(abcd, e, tmp0);
            e = vsha1h_u32(vgetq_lane_u32(tmp1, 0));
            tmp0 = vsha1su0q_u32(msg3, msg0, msg1);
            msg3 = vsha1su1q_u32(tmp0, msg2);

            // Rounds 64-67
            tmp0 = vaddq_u32(msg0, vdupq_n_u32(0xCA62C1D6));
            tmp1 = abcd;
            abcd = vsha1pq_u32(abcd, e, tmp0);
            e = vsha1h_u32(vgetq_lane_u32(tmp1, 0));
            tmp0 = vsha1su0q_u32(msg0, msg1, msg2);
            msg0 = vsha1su1q_u32(tmp0, msg3);

            // Rounds 68-71
            tmp0 = vaddq_u32(msg1, vdupq_n_u32(0xCA62C1D6));
            tmp1 = abcd;
            abcd = vsha1pq_u32(abcd, e, tmp0);
            e = vsha1h_u32(vgetq_lane_u32(tmp1, 0));
            tmp0 = vsha1su0q_u32(msg1, msg2, msg3);
            let _ = vsha1su1q_u32(tmp0, msg0);

            // Rounds 72-75
            tmp0 = vaddq_u32(msg2, vdupq_n_u32(0xCA62C1D6));
            tmp1 = abcd;
            abcd = vsha1pq_u32(abcd, e, tmp0);
            e = vsha1h_u32(vgetq_lane_u32(tmp1, 0));

            // Rounds 76-79
            tmp0 = vaddq_u32(msg3, vdupq_n_u32(0xCA62C1D6));
            tmp1 = abcd;
            abcd = vsha1pq_u32(abcd, e, tmp0);
            e = vsha1h_u32(vgetq_lane_u32(tmp1, 0));

            // Add saved state
            abcd = vaddq_u32(abcd, abcd_save);
            e = e.wrapping_add(e_save);

            // Store state
            vst1q_u32(self.state.as_mut_ptr(), abcd);
            self.state[4] = e;
        }
    }
}

// MARK: Digest impl
impl digest::Digest for Sha1 {
    const BLOCK_SIZE: usize = 64;
    type Output = [u8; 20];

    fn update(&mut self, data: &[u8]) {
        self.update(data);
    }

    fn finalize_reset(&mut self) -> Self::Output {
        self.finalize_reset()
    }

    fn finalize(self) -> Self::Output {
        Sha1::finalize(self)
    }
}

// MARK: CPU features
#[cfg(all(
    any(target_arch = "x86_64", target_arch = "aarch64"),
    any(test, feature = "std")
))]
fn has_sha_acceleration() -> bool {
    cfg_select! {
        target_arch = "x86_64" => {
            std::arch::is_x86_feature_detected!("sha")
                && std::arch::is_x86_feature_detected!("ssse3")
                && std::arch::is_x86_feature_detected!("sse4.1")
        }
        target_arch = "aarch64" => std::arch::is_aarch64_feature_detected!("sha2"),
    }
}

#[cfg(all(
    any(target_arch = "x86_64", target_arch = "aarch64"),
    not(any(test, feature = "std"))
))]
const fn has_sha_acceleration() -> bool {
    cfg_select! {
        target_arch = "x86_64" => cfg!(all(target_feature = "sha", target_feature = "ssse3", target_feature = "sse4.1")),
        target_arch = "aarch64" => cfg!(target_feature = "sha2"),
    }
}

// MARK: Tests
#[cfg(test)]
mod test {
    use super::*;

    fn to_hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn test_sha1() {
        let test_cases = [
            ("", "da39a3ee5e6b4b0d3255bfef95601890afd80709"),
            ("abc", "a9993e364706816aba3e25717850c26c9cd0d89d"),
            ("hello world", "2aae6c35c94fcfb415dbe95f408b9ce91ee846ed"),
            (
                "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
                "84983e441c3bd26ebaae4aa1f95129e5e54670f1",
            ),
        ];

        for (input, expected) in test_cases {
            let hash = Sha1::digest(input.as_bytes());
            assert_eq!(to_hex(&hash), expected, "Failed for input: {input}");
        }
    }

    #[test]
    fn test_sha1_million_a() {
        let mut hasher = Sha1::new();
        for _ in 0..10000 {
            hasher.update([b'a'; 100]);
        }
        assert_eq!(
            to_hex(&hasher.finalize()),
            "34aa973cd4c4daa4f61eeb2bdbad27316534016f"
        );
    }

    #[test]
    fn test_sha1_reset() {
        let mut hasher = Sha1::new();
        hasher.update(b"abc");
        hasher.finalize_reset();
        hasher.update(b"hello world");
        assert_eq!(
            to_hex(&hasher.finalize_reset()),
            "2aae6c35c94fcfb415dbe95f408b9ce91ee846ed"
        );
    }

    #[test]
    fn test_sha1_update_boundaries() {
        for len in [0, 1, 55, 56, 63, 64, 65, 127, 128, 129, 1024] {
            let input = (0..len).map(|i| i as u8).collect::<Vec<_>>();
            let expected = Sha1::digest(&input);
            assert_eq!(<Sha1 as digest::Digest>::digest(&input), expected);
            let reference = match len {
                55 => Some("8ae2d46729cfe68ff927af5eec9c7d1b66d65ac2"),
                56 => Some("636e2ec698dac903498e648bd2f3af641d3c88cb"),
                63 => Some("6d942da0c4392b123528f2905c713a3ce28364bd"),
                64 => Some("c6138d514ffa2135bfce0ed0b8fac65669917ec7"),
                65 => Some("69bd728ad6e13cd76ff19751fde427b00e395746"),
                127 => Some("89d7312a903f65cd2b3e34a975e55dbea9033353"),
                128 => Some("e6434bc401f98603d7eda504790c98c67385d535"),
                129 => Some("3352e41cc30b40ae80108970492b21014049e625"),
                _ => None,
            };
            if let Some(reference) = reference {
                assert_eq!(to_hex(&expected), reference, "len={len}");
            }
            for chunk_size in [1, 7, 63, 64, 65, 128] {
                let mut hasher = Sha1::new();
                for chunk in input.chunks(chunk_size) {
                    hasher.update(chunk);
                }
                assert_eq!(
                    hasher.finalize(),
                    expected,
                    "len={len}, chunk_size={chunk_size}"
                );
            }
        }
    }
}
