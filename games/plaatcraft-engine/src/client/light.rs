/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The light of the terrain and liquid shaders of the desktop game as a palette, the colors
//! of the fog and the sky. The consoles' GPUs have no sRGB, so the colors are gamma encoded
//! and light is applied to the gamma encoded textures.

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
    (linear.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0 + 0.5) as u8
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

// The gamma encoded RGBA color of a palette index of a chunk vertex.
fn color(index: u16, daylight: f32) -> [u8; 4] {
    const TERRAIN_FACES: [f32; 8] = [0.8, 0.8, 1.0, 0.5, 0.65, 0.65, 1.0, 1.0];
    const LIQUID_FACES: [f32; 8] = [0.85, 0.85, 1.0, 0.65, 0.75, 0.75, 1.0, 1.0];
    let curve = |level: u16| (f32::from(level) / 15.0).powf(1.8);
    let tint = index >> 13;
    let occlusion = [0.45, 0.65, 0.82, 1.0][usize::from(index >> 11 & 3)];
    let face = usize::from(index >> 8 & 7);
    let light = index & 255;
    let sky = curve(light & 15) * daylight;
    let block = curve(light >> 4);
    let block = [block, block * 0.62, block * 0.30];
    let (sky, minimum, faces, color, alpha) = match tint {
        TINT_WATER => ([sky; 3], 0.025, LIQUID_FACES, [0.15, 0.58, 0.75], 158),
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
    let [r, g, b] = lit.map(|value| encode(value * occlusion));
    [r, g, b, alpha]
}

/// The colors of all palette indices for the daylight.
pub fn palette(daylight: f32) -> Vec<[u8; 4]> {
    (0..PALETTE_SIZE as u16)
        .map(|index| color(index, daylight))
        .collect()
}
