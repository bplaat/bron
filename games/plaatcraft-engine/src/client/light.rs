/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The light of the terrain and liquid shaders of the desktop game as a palette, the colors
//! of the fog and the sky. GX uses gamma encoded colors, GX2 uses linear colors with sRGB
//! textures and a matching render target.

use alloc::vec::Vec;

#[cfg(not(feature = "std"))]
use crate::math::Float;

/// The daylight at night, 1.0 during the day.
pub const NIGHT_DAYLIGHT: f32 = 0.035;

// The palette is indexed by tint, face and the light byte of a quad
pub(super) const TINT_PINE: u16 = 1;
pub(super) const TINT_BEDROCK: u16 = 2;
pub(super) const TINT_LAVA: u16 = 3;
pub(super) const TINT_WATER: u16 = 4;
const TINTS: usize = 5;
const PALETTE_SIZE: usize = TINTS << 13;

pub(super) fn encode(linear: f32) -> u8 {
    let linear = linear.clamp(0.0, 1.0);
    let encoded = if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0 + 0.5) as u8
}

/// Decodes an sRGB color for lighting and blending in linear space.
pub fn linear_rgb(color: [u8; 3]) -> [f32; 3] {
    color.map(|value| {
        let value = f32::from(value) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    })
}

pub(super) fn encode_rgb(color: [f32; 3]) -> [u8; 3] {
    color.map(encode)
}

pub(super) fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    core::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

pub(super) fn fog_color(daylight: f32) -> [f32; 3] {
    mix([0.012, 0.018, 0.04], [0.64, 0.79, 0.90], daylight)
}

fn sky_color(sky: f32) -> [f32; 3] {
    [0.85 * sky, 0.92 * sky, sky]
}

// The linear RGBA color of a palette index of a chunk vertex.
fn color(index: u16, daylight: f32) -> [f32; 4] {
    const TERRAIN_FACES: [f32; 8] = [0.8, 0.8, 1.0, 0.5, 0.65, 0.65, 1.0, 1.0];
    const LIQUID_FACES: [f32; 8] = [0.85, 0.85, 1.0, 0.65, 0.75, 0.75, 1.0, 1.0];
    let curve = |level: u16| (f32::from(level) / 15.0).powf(1.8);
    let tint = index >> 13;
    let occlusion = if tint == TINT_WATER {
        1.0
    } else {
        [0.45, 0.65, 0.82, 1.0][usize::from(index >> 11 & 3)]
    };
    let face = usize::from(index >> 8 & 7);
    let light = index & 255;
    let sky = curve(light & 15) * daylight;
    let block = curve(light >> 4);
    let block = [block, block * 0.62, block * 0.30];
    let (sky, minimum, faces, color, alpha) = match tint {
        TINT_WATER => ([sky; 3], 0.025, LIQUID_FACES, [0.15, 0.58, 0.75], 158u8),
        TINT_PINE => (
            sky_color(sky),
            0.018,
            TERRAIN_FACES,
            [0.48, 0.68, 0.55],
            255,
        ),
        TINT_BEDROCK => (
            sky_color(sky),
            0.018,
            TERRAIN_FACES,
            [0.25, 0.27, 0.30],
            255,
        ),
        _ => (sky_color(sky), 0.018, TERRAIN_FACES, [1.0; 3], 255),
    };
    let lit: [f32; 3] = if tint == TINT_LAVA {
        [1.0; 3]
    } else {
        core::array::from_fn(|i| sky[i].max(block[i]).max(minimum) * faces[face] * color[i])
    };
    let [r, g, b] = lit.map(|value| value * occlusion);
    [r, g, b, f32::from(alpha) / 255.0]
}

/// The colors of all palette indices for the daylight.
pub fn palette(daylight: f32) -> Vec<[u8; 4]> {
    (0..PALETTE_SIZE as u16)
        .map(|index| {
            let [r, g, b, a] = color(index, daylight);
            let [r, g, b] = encode_rgb([r, g, b]);
            [r, g, b, (a * 255.0 + 0.5) as u8]
        })
        .collect()
}

/// Linear colors for a GPU with sRGB textures and output.
pub fn linear_palette(daylight: f32) -> Vec<[f32; 4]> {
    (0..PALETTE_SIZE as u16)
        .map(|index| color(index, daylight))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_round_trip() {
        for value in 0..=255 {
            assert_eq!(encode_rgb(linear_rgb([value; 3])), [value; 3]);
        }
        assert_eq!(encode_rgb([0.5; 3]), [188; 3]);
    }

    #[test]
    fn palettes_match_in_both_color_spaces() {
        for daylight in [1.0, NIGHT_DAYLIGHT] {
            for (encoded, linear) in palette(daylight).into_iter().zip(linear_palette(daylight)) {
                assert_eq!(encoded[..3], encode_rgb([linear[0], linear[1], linear[2]]));
                assert_eq!(encoded[3], (linear[3] * 255.0 + 0.5) as u8);
            }
        }
    }

    #[test]
    fn water_has_no_terrain_occlusion_and_block_light_survives_night() {
        let water = TINT_WATER << 13 | 2 << 8 | 15;
        assert_eq!(color(water, 1.0), color(water | 3 << 11, 1.0));
        let torch = 3 << 11 | 2 << 8 | 15 << 4;
        assert_eq!(color(torch, 1.0), color(torch, NIGHT_DAYLIGHT));
        let [r, g, b, _] = color(torch, NIGHT_DAYLIGHT);
        assert!(r > g && g > b);
    }
}
