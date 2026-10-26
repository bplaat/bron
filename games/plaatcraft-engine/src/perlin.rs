/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#[cfg(not(feature = "std"))]
use crate::math::Float;

// Seeded two-dimensional Perlin noise, independent of world generation.
pub(crate) struct Perlin {
    seed: u32,
}

// The low 32 bits of a whole number, the Wii converts to 64 bits in software.
fn low_bits(value: f64) -> u32 {
    if value.abs() < 2_147_483_648.0 {
        value as i32 as u32
    } else {
        value as i64 as u32
    }
}

impl Perlin {
    pub(crate) const fn new(seed: u64) -> Self {
        Self { seed: seed as u32 }
    }

    // The gradient only uses the low three bits of the hash, which only depend on the low
    // 32 bits of the coordinates and seed, so 32-bit math gives the 64-bit hash's result.
    fn gradient(&self, x: u32, z: u32, dx: f64, dz: f64) -> f64 {
        let mut hash = x.wrapping_mul(0x8da6_b343) ^ z.wrapping_mul(0xd816_3841) ^ self.seed;
        hash ^= hash >> 13;
        hash = hash.wrapping_mul(0x85eb_ca6b);
        hash ^= hash >> 16;
        let diagonal = core::f64::consts::FRAC_1_SQRT_2;
        match hash & 7 {
            0 => dx,
            1 => -dx,
            2 => dz,
            3 => -dz,
            4 => (dx + dz) * diagonal,
            5 => (dx - dz) * diagonal,
            6 => (-dx + dz) * diagonal,
            _ => (-dx - dz) * diagonal,
        }
    }

    pub(crate) fn sample(&self, x: f64, z: f64) -> f64 {
        fn fade(t: f64) -> f64 {
            t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
        }
        fn lerp(a: f64, b: f64, t: f64) -> f64 {
            a + (b - a) * t
        }
        let (floor_x, floor_z) = (x.floor(), z.floor());
        let (ix, iz) = (low_bits(floor_x), low_bits(floor_z));
        let (dx, dz) = (x - floor_x, z - floor_z);
        lerp(
            lerp(
                self.gradient(ix, iz, dx, dz),
                self.gradient(ix.wrapping_add(1), iz, dx - 1.0, dz),
                fade(dx),
            ),
            lerp(
                self.gradient(ix, iz.wrapping_add(1), dx, dz - 1.0),
                self.gradient(ix.wrapping_add(1), iz.wrapping_add(1), dx - 1.0, dz - 1.0),
                fade(dx),
            ),
            fade(dz),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gradients_match_the_original_64_bit_hash() {
        let original = |x: i64, z: i64, seed: u64| {
            let mut hash =
                (x as u64).wrapping_mul(0x8da6_b343) ^ (z as u64).wrapping_mul(0xd816_3841) ^ seed;
            hash ^= hash >> 13;
            hash = hash.wrapping_mul(0x85eb_ca6b);
            hash ^= hash >> 16;
            hash & 7
        };
        // The gradients of this offset are all distinct, so they identify the hash
        let (dx, dz) = (1.0, 2.0);
        let diagonal = core::f64::consts::FRAC_1_SQRT_2;
        let gradients = [
            dx,
            -dx,
            dz,
            -dz,
            (dx + dz) * diagonal,
            (dx - dz) * diagonal,
            (-dx + dz) * diagonal,
            (-dx - dz) * diagonal,
        ];
        let big = [1 << 40, -(1 << 33), i64::MIN / 2, 123_456_789];
        for seed in [0, 42 ^ 0x1a2b, u64::MAX - 7] {
            let perlin = Perlin::new(seed);
            for (x, z) in (-50..50)
                .flat_map(|x| (-50..50).map(move |z| (x * 7, z * 13)))
                .chain(big.into_iter().zip(big.into_iter().rev()))
            {
                let gradient = perlin.gradient(low_bits(x as f64), low_bits(z as f64), dx, dz);
                assert_eq!(gradient, gradients[original(x, z, seed) as usize]);
            }
        }
    }
}
