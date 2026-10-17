/*
 * Copyright (c) 2024-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [sha2](https://crates.io/crates/sha2) crate
//!
//! Disable default features for `no_std` support. CPU acceleration then uses compile-time
//! target features instead of runtime detection.

#![cfg_attr(not(any(test, feature = "std")), no_std)]

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// A SHA-256 hasher
#[derive(Clone)]
pub struct Sha256 {
    state: [u32; 8],
    buffer: [u8; 64],
    length: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            buffer: [0; 64],
            length: 0,
        }
    }
}

impl Sha256 {
    /// Create a new SHA-256 hasher
    pub fn new() -> Self {
        Self::default()
    }

    /// Compute the SHA-256 digest of the given data
    pub fn digest(data: impl AsRef<[u8]>) -> [u8; 32] {
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
    pub fn finalize(mut self) -> [u8; 32] {
        self.finalize_inner()
    }

    /// Finalize the hash, reset the hasher, and return the digest
    pub fn finalize_reset(&mut self) -> [u8; 32] {
        let result = self.finalize_inner();
        self.reset();
        result
    }

    fn finalize_inner(&mut self) -> [u8; 32] {
        let length_bits = self.length * 8;
        let used = self.length as usize % 64;
        self.buffer[used] = 0x80;
        self.buffer[used + 1..].fill(0);
        if used >= 56 {
            let block = self.buffer;
            self.process_blocks(core::slice::from_ref(&block));
            self.buffer.fill(0);
        }
        self.buffer[56..].copy_from_slice(&length_bits.to_be_bytes());
        let block = self.buffer;
        self.process_blocks(core::slice::from_ref(&block));
        let mut result = [0u8; 32];
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
            panic!("SHA-256 SIMD acceleration is unavailable");
        }
        for block in blocks {
            self.process_block_software(block);
        }
    }

    // MARK: Software
    fn process_block_software(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for (i, chunk) in block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut a = self.state[0];
        let mut b = self.state[1];
        let mut c = self.state[2];
        let mut d = self.state[3];
        let mut e = self.state[4];
        let mut f = self.state[5];
        let mut g = self.state[6];
        let mut h = self.state[7];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
        self.state[5] = self.state[5].wrapping_add(f);
        self.state[6] = self.state[6].wrapping_add(g);
        self.state[7] = self.state[7].wrapping_add(h);
    }

    // MARK: x86_64 SHA-NI
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "sha,sse2,ssse3,sse4.1")]
    #[allow(unsafe_code)]
    unsafe fn process_block_x86_sha(&mut self, block: &[u8; 64]) {
        // SAFETY: The caller verified sha/sse2/ssse3/sse4.1 feature availability; self.state and block are properly initialized.
        unsafe {
            use core::arch::x86_64::*;

            // Per-word byte-swap mask (big-endian to little-endian within each u32)
            let mask = _mm_set_epi64x(
                0x0c_0d_0e_0f_08_09_0a_0b_u64 as i64,
                0x04_05_06_07_00_01_02_03_u64 as i64,
            );

            // Load and shuffle state into ABEF / CDGH layout expected by sha256rnds2
            let tmp = _mm_loadu_si128(self.state.as_ptr().cast());
            let mut state1 = _mm_loadu_si128(self.state.as_ptr().add(4).cast());
            let tmp = _mm_shuffle_epi32(tmp, 0xB1); // CDAB
            state1 = _mm_shuffle_epi32(state1, 0x1B); // EFGH
            let mut state0 = _mm_alignr_epi8::<8>(tmp, state1); // ABEF
            state1 = _mm_blend_epi16::<0xF0>(state1, tmp); // CDGH

            let state0_save = state0;
            let state1_save = state1;

            let mut msg0 = _mm_shuffle_epi8(_mm_loadu_si128(block.as_ptr().cast()), mask);
            let mut msg1 = _mm_shuffle_epi8(_mm_loadu_si128(block.as_ptr().add(16).cast()), mask);
            let mut msg2 = _mm_shuffle_epi8(_mm_loadu_si128(block.as_ptr().add(32).cast()), mask);
            let mut msg3 = _mm_shuffle_epi8(_mm_loadu_si128(block.as_ptr().add(48).cast()), mask);

            let mut tmp;

            // Rounds 0-3
            tmp = _mm_add_epi32(msg0, _mm_loadu_si128(K.as_ptr().cast()));
            state1 = _mm_sha256rnds2_epu32(state1, state0, tmp);
            tmp = _mm_shuffle_epi32(tmp, 0x0E);
            state0 = _mm_sha256rnds2_epu32(state0, state1, tmp);

            // Rounds 4-7
            tmp = _mm_add_epi32(msg1, _mm_loadu_si128(K.as_ptr().add(4).cast()));
            state1 = _mm_sha256rnds2_epu32(state1, state0, tmp);
            tmp = _mm_shuffle_epi32(tmp, 0x0E);
            state0 = _mm_sha256rnds2_epu32(state0, state1, tmp);
            msg0 = _mm_sha256msg1_epu32(msg0, msg1);

            // Rounds 8-11
            tmp = _mm_add_epi32(msg2, _mm_loadu_si128(K.as_ptr().add(8).cast()));
            state1 = _mm_sha256rnds2_epu32(state1, state0, tmp);
            tmp = _mm_shuffle_epi32(tmp, 0x0E);
            state0 = _mm_sha256rnds2_epu32(state0, state1, tmp);
            msg1 = _mm_sha256msg1_epu32(msg1, msg2);

            // Rounds 12-15
            tmp = _mm_add_epi32(msg3, _mm_loadu_si128(K.as_ptr().add(12).cast()));
            state1 = _mm_sha256rnds2_epu32(state1, state0, tmp);
            let t = _mm_alignr_epi8::<4>(msg3, msg2);
            msg0 = _mm_add_epi32(msg0, t);
            msg0 = _mm_sha256msg2_epu32(msg0, msg3);
            tmp = _mm_shuffle_epi32(tmp, 0x0E);
            state0 = _mm_sha256rnds2_epu32(state0, state1, tmp);
            msg2 = _mm_sha256msg1_epu32(msg2, msg3);

            // Rounds 16-19
            tmp = _mm_add_epi32(msg0, _mm_loadu_si128(K.as_ptr().add(16).cast()));
            state1 = _mm_sha256rnds2_epu32(state1, state0, tmp);
            let t = _mm_alignr_epi8::<4>(msg0, msg3);
            msg1 = _mm_add_epi32(msg1, t);
            msg1 = _mm_sha256msg2_epu32(msg1, msg0);
            tmp = _mm_shuffle_epi32(tmp, 0x0E);
            state0 = _mm_sha256rnds2_epu32(state0, state1, tmp);
            msg3 = _mm_sha256msg1_epu32(msg3, msg0);

            // Rounds 20-23
            tmp = _mm_add_epi32(msg1, _mm_loadu_si128(K.as_ptr().add(20).cast()));
            state1 = _mm_sha256rnds2_epu32(state1, state0, tmp);
            let t = _mm_alignr_epi8::<4>(msg1, msg0);
            msg2 = _mm_add_epi32(msg2, t);
            msg2 = _mm_sha256msg2_epu32(msg2, msg1);
            tmp = _mm_shuffle_epi32(tmp, 0x0E);
            state0 = _mm_sha256rnds2_epu32(state0, state1, tmp);
            msg0 = _mm_sha256msg1_epu32(msg0, msg1);

            // Rounds 24-27
            tmp = _mm_add_epi32(msg2, _mm_loadu_si128(K.as_ptr().add(24).cast()));
            state1 = _mm_sha256rnds2_epu32(state1, state0, tmp);
            let t = _mm_alignr_epi8::<4>(msg2, msg1);
            msg3 = _mm_add_epi32(msg3, t);
            msg3 = _mm_sha256msg2_epu32(msg3, msg2);
            tmp = _mm_shuffle_epi32(tmp, 0x0E);
            state0 = _mm_sha256rnds2_epu32(state0, state1, tmp);
            msg1 = _mm_sha256msg1_epu32(msg1, msg2);

            // Rounds 28-31
            tmp = _mm_add_epi32(msg3, _mm_loadu_si128(K.as_ptr().add(28).cast()));
            state1 = _mm_sha256rnds2_epu32(state1, state0, tmp);
            let t = _mm_alignr_epi8::<4>(msg3, msg2);
            msg0 = _mm_add_epi32(msg0, t);
            msg0 = _mm_sha256msg2_epu32(msg0, msg3);
            tmp = _mm_shuffle_epi32(tmp, 0x0E);
            state0 = _mm_sha256rnds2_epu32(state0, state1, tmp);
            msg2 = _mm_sha256msg1_epu32(msg2, msg3);

            // Rounds 32-35
            tmp = _mm_add_epi32(msg0, _mm_loadu_si128(K.as_ptr().add(32).cast()));
            state1 = _mm_sha256rnds2_epu32(state1, state0, tmp);
            let t = _mm_alignr_epi8::<4>(msg0, msg3);
            msg1 = _mm_add_epi32(msg1, t);
            msg1 = _mm_sha256msg2_epu32(msg1, msg0);
            tmp = _mm_shuffle_epi32(tmp, 0x0E);
            state0 = _mm_sha256rnds2_epu32(state0, state1, tmp);
            msg3 = _mm_sha256msg1_epu32(msg3, msg0);

            // Rounds 36-39
            tmp = _mm_add_epi32(msg1, _mm_loadu_si128(K.as_ptr().add(36).cast()));
            state1 = _mm_sha256rnds2_epu32(state1, state0, tmp);
            let t = _mm_alignr_epi8::<4>(msg1, msg0);
            msg2 = _mm_add_epi32(msg2, t);
            msg2 = _mm_sha256msg2_epu32(msg2, msg1);
            tmp = _mm_shuffle_epi32(tmp, 0x0E);
            state0 = _mm_sha256rnds2_epu32(state0, state1, tmp);
            msg0 = _mm_sha256msg1_epu32(msg0, msg1);

            // Rounds 40-43
            tmp = _mm_add_epi32(msg2, _mm_loadu_si128(K.as_ptr().add(40).cast()));
            state1 = _mm_sha256rnds2_epu32(state1, state0, tmp);
            let t = _mm_alignr_epi8::<4>(msg2, msg1);
            msg3 = _mm_add_epi32(msg3, t);
            msg3 = _mm_sha256msg2_epu32(msg3, msg2);
            tmp = _mm_shuffle_epi32(tmp, 0x0E);
            state0 = _mm_sha256rnds2_epu32(state0, state1, tmp);
            msg1 = _mm_sha256msg1_epu32(msg1, msg2);

            // Rounds 44-47
            tmp = _mm_add_epi32(msg3, _mm_loadu_si128(K.as_ptr().add(44).cast()));
            state1 = _mm_sha256rnds2_epu32(state1, state0, tmp);
            let t = _mm_alignr_epi8::<4>(msg3, msg2);
            msg0 = _mm_add_epi32(msg0, t);
            msg0 = _mm_sha256msg2_epu32(msg0, msg3);
            tmp = _mm_shuffle_epi32(tmp, 0x0E);
            state0 = _mm_sha256rnds2_epu32(state0, state1, tmp);
            msg2 = _mm_sha256msg1_epu32(msg2, msg3);

            // Rounds 48-51
            tmp = _mm_add_epi32(msg0, _mm_loadu_si128(K.as_ptr().add(48).cast()));
            state1 = _mm_sha256rnds2_epu32(state1, state0, tmp);
            let t = _mm_alignr_epi8::<4>(msg0, msg3);
            msg1 = _mm_add_epi32(msg1, t);
            msg1 = _mm_sha256msg2_epu32(msg1, msg0);
            tmp = _mm_shuffle_epi32(tmp, 0x0E);
            state0 = _mm_sha256rnds2_epu32(state0, state1, tmp);
            msg3 = _mm_sha256msg1_epu32(msg3, msg0);

            // Rounds 52-55
            tmp = _mm_add_epi32(msg1, _mm_loadu_si128(K.as_ptr().add(52).cast()));
            state1 = _mm_sha256rnds2_epu32(state1, state0, tmp);
            let t = _mm_alignr_epi8::<4>(msg1, msg0);
            msg2 = _mm_add_epi32(msg2, t);
            msg2 = _mm_sha256msg2_epu32(msg2, msg1);
            tmp = _mm_shuffle_epi32(tmp, 0x0E);
            state0 = _mm_sha256rnds2_epu32(state0, state1, tmp);

            // Rounds 56-59
            tmp = _mm_add_epi32(msg2, _mm_loadu_si128(K.as_ptr().add(56).cast()));
            state1 = _mm_sha256rnds2_epu32(state1, state0, tmp);
            let t = _mm_alignr_epi8::<4>(msg2, msg1);
            msg3 = _mm_add_epi32(msg3, t);
            msg3 = _mm_sha256msg2_epu32(msg3, msg2);
            tmp = _mm_shuffle_epi32(tmp, 0x0E);
            state0 = _mm_sha256rnds2_epu32(state0, state1, tmp);

            // Rounds 60-63
            tmp = _mm_add_epi32(msg3, _mm_loadu_si128(K.as_ptr().add(60).cast()));
            state1 = _mm_sha256rnds2_epu32(state1, state0, tmp);
            tmp = _mm_shuffle_epi32(tmp, 0x0E);
            state0 = _mm_sha256rnds2_epu32(state0, state1, tmp);

            // Combine state
            state0 = _mm_add_epi32(state0, state0_save);
            state1 = _mm_add_epi32(state1, state1_save);

            // Unshuffle ABEF/CDGH back to [a,b,c,d] / [e,f,g,h]
            let tmp = _mm_shuffle_epi32(state0, 0x1B); // FEBA
            state1 = _mm_shuffle_epi32(state1, 0xB1); // DCHG
            state0 = _mm_blend_epi16::<0xF0>(tmp, state1); // DCBA
            state1 = _mm_alignr_epi8::<8>(state1, tmp); // ABEF

            _mm_storeu_si128(self.state.as_mut_ptr().cast(), state0);
            _mm_storeu_si128(self.state.as_mut_ptr().add(4).cast(), state1);
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
            let mut state0 = vld1q_u32(self.state.as_ptr()); // [a, b, c, d]
            let mut state1 = vld1q_u32(self.state.as_ptr().add(4)); // [e, f, g, h]
            let state0_save = state0;
            let state1_save = state1;

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
            let mut tmp2;

            // Rounds 0-3
            tmp0 = vaddq_u32(msg0, vld1q_u32(K.as_ptr()));
            tmp2 = state0;
            state0 = vsha256hq_u32(state0, state1, tmp0);
            state1 = vsha256h2q_u32(state1, tmp2, tmp0);
            msg0 = vsha256su0q_u32(msg0, msg1);

            // Rounds 4-7
            tmp0 = vaddq_u32(msg1, vld1q_u32(K.as_ptr().add(4)));
            tmp2 = state0;
            state0 = vsha256hq_u32(state0, state1, tmp0);
            state1 = vsha256h2q_u32(state1, tmp2, tmp0);
            msg0 = vsha256su1q_u32(msg0, msg2, msg3);
            msg1 = vsha256su0q_u32(msg1, msg2);

            // Rounds 8-11
            tmp0 = vaddq_u32(msg2, vld1q_u32(K.as_ptr().add(8)));
            tmp2 = state0;
            state0 = vsha256hq_u32(state0, state1, tmp0);
            state1 = vsha256h2q_u32(state1, tmp2, tmp0);
            msg1 = vsha256su1q_u32(msg1, msg3, msg0);
            msg2 = vsha256su0q_u32(msg2, msg3);

            // Rounds 12-15
            tmp0 = vaddq_u32(msg3, vld1q_u32(K.as_ptr().add(12)));
            tmp2 = state0;
            state0 = vsha256hq_u32(state0, state1, tmp0);
            state1 = vsha256h2q_u32(state1, tmp2, tmp0);
            msg2 = vsha256su1q_u32(msg2, msg0, msg1);
            msg3 = vsha256su0q_u32(msg3, msg0);

            // Rounds 16-19
            tmp0 = vaddq_u32(msg0, vld1q_u32(K.as_ptr().add(16)));
            tmp2 = state0;
            state0 = vsha256hq_u32(state0, state1, tmp0);
            state1 = vsha256h2q_u32(state1, tmp2, tmp0);
            msg3 = vsha256su1q_u32(msg3, msg1, msg2);
            msg0 = vsha256su0q_u32(msg0, msg1);

            // Rounds 20-23
            tmp0 = vaddq_u32(msg1, vld1q_u32(K.as_ptr().add(20)));
            tmp2 = state0;
            state0 = vsha256hq_u32(state0, state1, tmp0);
            state1 = vsha256h2q_u32(state1, tmp2, tmp0);
            msg0 = vsha256su1q_u32(msg0, msg2, msg3);
            msg1 = vsha256su0q_u32(msg1, msg2);

            // Rounds 24-27
            tmp0 = vaddq_u32(msg2, vld1q_u32(K.as_ptr().add(24)));
            tmp2 = state0;
            state0 = vsha256hq_u32(state0, state1, tmp0);
            state1 = vsha256h2q_u32(state1, tmp2, tmp0);
            msg1 = vsha256su1q_u32(msg1, msg3, msg0);
            msg2 = vsha256su0q_u32(msg2, msg3);

            // Rounds 28-31
            tmp0 = vaddq_u32(msg3, vld1q_u32(K.as_ptr().add(28)));
            tmp2 = state0;
            state0 = vsha256hq_u32(state0, state1, tmp0);
            state1 = vsha256h2q_u32(state1, tmp2, tmp0);
            msg2 = vsha256su1q_u32(msg2, msg0, msg1);
            msg3 = vsha256su0q_u32(msg3, msg0);

            // Rounds 32-35
            tmp0 = vaddq_u32(msg0, vld1q_u32(K.as_ptr().add(32)));
            tmp2 = state0;
            state0 = vsha256hq_u32(state0, state1, tmp0);
            state1 = vsha256h2q_u32(state1, tmp2, tmp0);
            msg3 = vsha256su1q_u32(msg3, msg1, msg2);
            msg0 = vsha256su0q_u32(msg0, msg1);

            // Rounds 36-39
            tmp0 = vaddq_u32(msg1, vld1q_u32(K.as_ptr().add(36)));
            tmp2 = state0;
            state0 = vsha256hq_u32(state0, state1, tmp0);
            state1 = vsha256h2q_u32(state1, tmp2, tmp0);
            msg0 = vsha256su1q_u32(msg0, msg2, msg3);
            msg1 = vsha256su0q_u32(msg1, msg2);

            // Rounds 40-43
            tmp0 = vaddq_u32(msg2, vld1q_u32(K.as_ptr().add(40)));
            tmp2 = state0;
            state0 = vsha256hq_u32(state0, state1, tmp0);
            state1 = vsha256h2q_u32(state1, tmp2, tmp0);
            msg1 = vsha256su1q_u32(msg1, msg3, msg0);
            msg2 = vsha256su0q_u32(msg2, msg3);

            // Rounds 44-47
            tmp0 = vaddq_u32(msg3, vld1q_u32(K.as_ptr().add(44)));
            tmp2 = state0;
            state0 = vsha256hq_u32(state0, state1, tmp0);
            state1 = vsha256h2q_u32(state1, tmp2, tmp0);
            msg2 = vsha256su1q_u32(msg2, msg0, msg1);
            msg3 = vsha256su0q_u32(msg3, msg0);

            // Rounds 48-51
            tmp0 = vaddq_u32(msg0, vld1q_u32(K.as_ptr().add(48)));
            tmp2 = state0;
            state0 = vsha256hq_u32(state0, state1, tmp0);
            state1 = vsha256h2q_u32(state1, tmp2, tmp0);
            msg3 = vsha256su1q_u32(msg3, msg1, msg2);

            // Rounds 52-55
            tmp0 = vaddq_u32(msg1, vld1q_u32(K.as_ptr().add(52)));
            tmp2 = state0;
            state0 = vsha256hq_u32(state0, state1, tmp0);
            state1 = vsha256h2q_u32(state1, tmp2, tmp0);

            // Rounds 56-59
            tmp0 = vaddq_u32(msg2, vld1q_u32(K.as_ptr().add(56)));
            tmp2 = state0;
            state0 = vsha256hq_u32(state0, state1, tmp0);
            state1 = vsha256h2q_u32(state1, tmp2, tmp0);

            // Rounds 60-63
            tmp0 = vaddq_u32(msg3, vld1q_u32(K.as_ptr().add(60)));
            tmp2 = state0;
            state0 = vsha256hq_u32(state0, state1, tmp0);
            state1 = vsha256h2q_u32(state1, tmp2, tmp0);

            // Add saved state
            state0 = vaddq_u32(state0, state0_save);
            state1 = vaddq_u32(state1, state1_save);

            // Store state
            vst1q_u32(self.state.as_mut_ptr(), state0);
            vst1q_u32(self.state.as_mut_ptr().add(4), state1);
        }
    }
}

// MARK: Digest impl
impl digest::Digest for Sha256 {
    const BLOCK_SIZE: usize = 64;
    type Output = [u8; 32];

    fn update(&mut self, data: &[u8]) {
        self.update(data);
    }

    fn finalize_reset(&mut self) -> Self::Output {
        self.finalize_reset()
    }

    fn finalize(self) -> Self::Output {
        Sha256::finalize(self)
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
    fn test_sha256() {
        let test_cases = [
            (
                "",
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ),
            (
                "abc",
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            ),
            (
                "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
                "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
            ),
        ];

        for (input, expected) in test_cases {
            let hash = Sha256::digest(input.as_bytes());
            assert_eq!(to_hex(&hash), expected, "Failed for input: {input}");
        }
    }

    #[test]
    fn test_sha256_million_a() {
        let mut hasher = Sha256::new();
        for _ in 0..10000 {
            hasher.update([b'a'; 100]);
        }
        assert_eq!(
            to_hex(&hasher.finalize()),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn test_sha256_reset() {
        let mut hasher = Sha256::new();
        hasher.update(b"abc");
        hasher.finalize_reset();
        hasher.update(b"test");
        assert_eq!(
            to_hex(&hasher.finalize_reset()),
            "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
        );
    }

    #[test]
    fn test_sha256_update_boundaries() {
        for len in [0, 1, 55, 56, 63, 64, 65, 127, 128, 129, 1024] {
            let input = (0..len).map(|i| i as u8).collect::<Vec<_>>();
            let expected = Sha256::digest(&input);
            assert_eq!(<Sha256 as digest::Digest>::digest(&input), expected);
            let reference = match len {
                55 => Some("463eb28e72f82e0a96c0a4cc53690c571281131f672aa229e0d45ae59b598b59"),
                56 => Some("da2ae4d6b36748f2a318f23e7ab1dfdf45acdc9d049bd80e59de82a60895f562"),
                63 => Some("29af2686fd53374a36b0846694cc342177e428d1647515f078784d69cdb9e488"),
                64 => Some("fdeab9acf3710362bd2658cdc9a29e8f9c757fcf9811603a8c447cd1d9151108"),
                65 => Some("4bfd2c8b6f1eec7a2afeb48b934ee4b2694182027e6d0fc075074f2fabb31781"),
                127 => Some("92ca0fa6651ee2f97b884b7246a562fa71250fedefe5ebf270d31c546bfea976"),
                128 => Some("471fb943aa23c511f6f72f8d1652d9c880cfa392ad80503120547703e56a2be5"),
                129 => Some("5099c6a56203f9687f7d33f4bfdf576d31dc91f6b695ecea38b2770c87631135"),
                _ => None,
            };
            if let Some(reference) = reference {
                assert_eq!(to_hex(&expected), reference, "len={len}");
            }
            for chunk_size in [1, 7, 63, 64, 65, 128] {
                let mut hasher = Sha256::new();
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
