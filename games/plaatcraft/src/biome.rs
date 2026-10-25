/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use crate::perlin::Perlin;
use crate::world::{HEIGHT, SEA_LEVEL};

// Landforms first, then climate and surface ecosystems. Inspired by:
// https://learn.microsoft.com/en-us/minecraft/creator/documents/world-generation
// https://www.redblobgames.com/maps/terrain-from-noise/
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Biome {
    Sea,
    Beach,
    #[default]
    Plains,
    Forest,
    Desert,
    Taiga,
    Tundra,
    River,
    Mountains,
}

#[derive(Clone, Copy)]
pub(crate) struct Column {
    pub(crate) height: u8,
    pub(crate) biome: Biome,
    pub(crate) temperature: f64,
    pub(crate) humidity: f64,
    pub(crate) ruggedness: f64,
}

impl Default for Column {
    fn default() -> Self {
        Self {
            height: SEA_LEVEL + 12,
            biome: Biome::Plains,
            temperature: 0.5,
            humidity: 0.5,
            ruggedness: 0.0,
        }
    }
}

pub(crate) struct Terrain {
    continent: Perlin,
    erosion: Perlin,
    ridges: Perlin,
    temperature: Perlin,
    humidity: Perlin,
    detail: Perlin,
    warp: Perlin,
}

pub(crate) fn smooth(low: f64, high: f64, value: f64) -> f64 {
    let t = ((value - low) / (high - low)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn coast_height(continent: f64) -> f64 {
    let sea = f64::from(SEA_LEVEL);
    let knots = [
        (-1.0, sea - 32.0),
        (-0.28, sea - 22.0),
        (-0.12, sea - 6.0),
        (-0.04, sea + 1.0),
        (0.08, sea + 12.0),
        (0.30, sea + 20.0),
        (1.0, sea + 30.0),
    ];
    for pair in knots.windows(2) {
        let [(a, low), (b, high)] = [pair[0], pair[1]];
        if continent <= b {
            return low + (high - low) * smooth(a, b, continent);
        }
    }
    knots[knots.len() - 1].1
}

impl Terrain {
    pub(crate) const fn new(seed: u64) -> Self {
        Self {
            continent: Perlin::new(seed ^ 0x1a2b),
            erosion: Perlin::new(seed ^ 0x3c4d),
            ridges: Perlin::new(seed ^ 0x5e6f),
            temperature: Perlin::new(seed ^ 0x7182),
            humidity: Perlin::new(seed ^ 0x93a4),
            detail: Perlin::new(seed ^ 0xb5c6),
            warp: Perlin::new(seed ^ 0xd7e8),
        }
    }

    pub(crate) fn sample(&self, x: i64, z: i64) -> Column {
        let (x, z) = (x as f64, z as f64);
        // Two independent displacements break up grid-aligned outlines.
        let wx = x + self.warp.sample(x / 360.0 + 7.3, z / 360.0 - 2.1) * 65.0;
        let wz = z + self.warp.sample(x / 360.0 - 51.7, z / 360.0 + 29.4) * 65.0;
        let continent = self.continent.sample(wx / 700.0 + 13.7, wz / 700.0 + 9.3) * 0.75
            + self.continent.sample(wx / 310.0 - 17.1, wz / 310.0 + 25.4) * 0.25;
        let erosion = smooth(
            -0.35,
            0.35,
            self.erosion.sample(wx / 330.0 + 3.1, wz / 330.0 - 7.9),
        );
        let ridge_noise = self.ridges.sample(wx / 220.0 + 24.6, wz / 220.0 - 17.3);
        let ridge = smooth(0.04, 0.32, ridge_noise.abs());
        let inland = smooth(-0.03, 0.18, continent);
        let ruggedness = (1.0 - erosion) * inland;
        let uplift = ruggedness * (12.0 + 46.0 * ridge);
        let detail = self.detail.sample(wx / 64.0, wz / 64.0) * 4.0
            + self.detail.sample(wx / 25.0 + 8.3, wz / 25.0 - 1.7) * 1.4;
        let base = coast_height(continent);
        let mut elevation = base + uplift + detail * (0.35 + ruggedness * 1.8);

        // Rivers follow the same broad valley field as the ridges. In rough highlands
        // the bed rises into a dry saddle instead of cutting every peak to sea level.
        let distance = ridge_noise.abs();
        let bank_width = 0.06 + erosion * 0.025;
        let valley = 1.0 - smooth(0.012, bank_width, distance);
        let bed = f64::from(SEA_LEVEL) - 4.0 + 30.0 * smooth(0.35, 0.85, ruggedness);
        elevation += (bed.min(elevation) - elevation) * valley * smooth(-0.12, 0.02, continent);
        // Ease toward the build ceiling instead of flattening every tall peak at a clamp.
        let shoulder = (HEIGHT - 30) as f64;
        if elevation > shoulder {
            elevation = shoulder + 16.0 * (1.0 - (-(elevation - shoulder) / 16.0).exp());
        }
        let height = elevation.round().clamp(4.0, (HEIGHT - 14) as f64) as u8;
        let temperature = (0.5
            + self
                .temperature
                .sample(wx / 580.0 + 35.2, wz / 580.0 - 11.8)
                * 1.35)
            .clamp(0.0, 1.0);
        let humidity = (0.5 + self.humidity.sample(wx / 470.0 - 23.6, wz / 470.0 + 64.1) * 1.35)
            .clamp(0.0, 1.0);
        let biome = if height < SEA_LEVEL {
            if base < f64::from(SEA_LEVEL) {
                Biome::Sea
            } else {
                Biome::River
            }
        } else if height <= SEA_LEVEL + 2 && continent < 0.04 {
            Biome::Beach
        } else if height >= 76 && ruggedness > 0.40 {
            Biome::Mountains
        } else if temperature < 0.30 {
            if humidity > 0.48 {
                Biome::Taiga
            } else {
                Biome::Tundra
            }
        } else if temperature > 0.58 && humidity < 0.40 {
            Biome::Desert
        } else if humidity > 0.54 {
            Biome::Forest
        } else {
            Biome::Plains
        };
        Column {
            height,
            biome,
            temperature,
            humidity,
            ruggedness,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn climate_selection_does_not_change_landform_height() {
        let original = Terrain::new(42);
        let mut different_climate = Terrain::new(42);
        different_climate.temperature = Perlin::new(731);
        different_climate.humidity = Perlin::new(942);
        let mut changed = 0;
        for z in (-1600..1600).step_by(64) {
            for x in (-1600..1600).step_by(64) {
                let a = original.sample(x, z);
                let b = different_climate.sample(x, z);
                assert_eq!(a.height, b.height);
                changed += usize::from(a.biome != b.biome);
            }
        }
        assert!(changed > 200);
    }

    #[test]
    fn seeds_are_repeatable_and_terrain_is_continuous() {
        let a = Terrain::new(42);
        let b = Terrain::new(43);
        let mut changed = 0;
        let mut tall = 0;
        for z in (-2048..2048).step_by(32) {
            for x in (-2048..2048).step_by(32) {
                let column = a.sample(x, z);
                assert_eq!(column.height, Terrain::new(42).sample(x, z).height);
                assert!(column.height >= 4 && usize::from(column.height) < HEIGHT - 14);
                assert!(column.height.abs_diff(a.sample(x + 1, z).height) <= 6);
                assert!(column.height.abs_diff(a.sample(x, z + 1).height) <= 6);
                assert!((0.0..=1.0).contains(&column.temperature));
                assert!((0.0..=1.0).contains(&column.humidity));
                if column.biome == Biome::Beach {
                    assert!((SEA_LEVEL..=SEA_LEVEL + 2).contains(&column.height));
                }
                changed += usize::from(column.height != b.sample(x, z).height);
                tall += usize::from(column.height >= 100);
            }
        }
        assert!(changed > 8000);
        assert!(tall > 20);
        for (x, z) in [(16_000_000, -16_000_000), (-16_000_000, 16_000_000)] {
            assert_eq!(a.sample(x, z).height, Terrain::new(42).sample(x, z).height);
        }
    }
}
