/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The four round Feistel network with a SHA-1 round function that scrambles installation IDs and
//! derives the curve point from the hardware and product ID.

use sha1::Sha1;

/// The round function: SHA-1 of the half block and key, truncated to the half block length.
/// For a length that isn't a multiple of four the last bytes come from the end of the next word.
fn round_key(half: &[u8], key: &[u8]) -> Vec<u8> {
    let mut hasher = Sha1::new();
    hasher.update(half);
    hasher.update(key);
    let digest = hasher.finalize();
    let (aligned, tail) = (half.len() & !3, half.len() & 3);
    (0..half.len())
        .map(|i| digest[if i < aligned { i } else { i + 4 - tail }])
        .collect()
}

/// Runs four Feistel rounds over the block, forwards or backwards. With an odd length the
/// middle byte stays in place.
pub fn feistel(block: &mut [u8], key: &[u8], forward: bool) {
    let half = block.len() / 2;
    let (left, rest) = block.split_at_mut(half);
    let right = &mut rest[..half];
    for _ in 0..4 {
        // Forwards (L, R) becomes (R, L ^ F(R)), backwards the inverse (R ^ F(L), L)
        let (target, source) = if forward {
            (&mut *left, &*right)
        } else {
            (&mut *right, &*left)
        };
        for (t, k) in target.iter_mut().zip(round_key(source, key)) {
            *t ^= k;
        }
        left.swap_with_slice(right);
    }
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feistel_roundtrip() {
        let original: Vec<u8> = (0..19).collect();
        let mut block = original.clone();
        feistel(&mut block, b"key", true);
        assert_ne!(block, original);
        feistel(&mut block, b"key", false);
        assert_eq!(block, original);
    }
}
