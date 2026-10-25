/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// Seeded two-dimensional Perlin noise, independent of world generation.
pub(crate) struct Perlin {
    seed: u64,
}

impl Perlin {
    pub(crate) const fn new(seed: u64) -> Self {
        Self { seed }
    }

    fn gradient(&self, x: i64, z: i64, dx: f64, dz: f64) -> f64 {
        let mut hash =
            (x as u64).wrapping_mul(0x8da6_b343) ^ (z as u64).wrapping_mul(0xd816_3841) ^ self.seed;
        hash ^= hash >> 13;
        hash = hash.wrapping_mul(0x85eb_ca6b);
        hash ^= hash >> 16;
        let diagonal = std::f64::consts::FRAC_1_SQRT_2;
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
        let ix = x.floor() as i64;
        let iz = z.floor() as i64;
        let dx = x - x.floor();
        let dz = z - z.floor();
        lerp(
            lerp(
                self.gradient(ix, iz, dx, dz),
                self.gradient(ix + 1, iz, dx - 1.0, dz),
                fade(dx),
            ),
            lerp(
                self.gradient(ix, iz + 1, dx, dz - 1.0),
                self.gradient(ix + 1, iz + 1, dx - 1.0, dz - 1.0),
                fade(dx),
            ),
            fade(dz),
        )
    }
}
