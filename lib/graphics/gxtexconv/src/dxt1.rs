/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A DXT1 compressor for GX CMPR textures. It starts with the base colors of the libtxc_dxtn
//! compressor that devkitPro's gxtexconv uses, then searches nearby 565 colors for the lowest
//! error with the palette the GX hardware decodes.

pub(crate) type Rgb = [i32; 3];

/// The error weights per channel, roughly the squared weights of the luminance conversion.
const WEIGHTS: Rgb = [4, 16, 1];

/// The bit offset and maximum value of the red, green and blue channels of a 565 color.
const CHANNELS_565: [(u16, u16); 3] = [(11, 31), (5, 63), (0, 31)];

fn color_error(a: &Rgb, b: &Rgb) -> i32 {
    (0..3).map(|c| (a[c] - b[c]).pow(2) * WEIGHTS[c]).sum()
}

/// Returns the index of the first palette color with the lowest error and that error.
fn nearest(color: &Rgb, palette: &[Rgb]) -> (usize, i32) {
    palette
        .iter()
        .map(|entry| color_error(color, entry))
        .enumerate()
        .min_by_key(|&(_, error)| error)
        .unwrap_or_default()
}

const fn to_rgb565(color: &Rgb) -> u16 {
    ((color[0] as u16 & 0xf8) << 8) | ((color[1] as u16 & 0xfc) << 3) | (color[2] as u16 >> 3)
}

fn from_rgb565(color: u16) -> Rgb {
    let [r, g, b] = CHANNELS_565.map(|(shift, max)| i32::from((color >> shift) & max));
    [r << 3 | r >> 2, g << 2 | g >> 4, b << 3 | b >> 2]
}

/// Returns the colors the GX hardware decodes: four colors blended at 3/8 and 5/8 when
/// color0 > color1, otherwise three colors (the fourth is transparent).
fn hardware_palette(color0: u16, color1: u16) -> ([Rgb; 4], usize) {
    let (a, b) = (from_rgb565(color0), from_rgb565(color1));
    if color0 > color1 {
        let blend = |x: &Rgb, y: &Rgb| std::array::from_fn(|c| (x[c] * 5 + y[c] * 3) >> 3);
        ([a, b, blend(&a, &b), blend(&b, &a)], 4)
    } else {
        (
            [a, b, std::array::from_fn(|c| (a[c] + b[c]) / 2), [0; 3]],
            3,
        )
    }
}

/// Encodes a GX CMPR block: big endian colors and the first pixel of a row in the high bits of
/// its index byte. Returns the block and its error.
fn encode_block(colors: &[Rgb; 16], color0: u16, color1: u16) -> ([u8; 8], i32) {
    let (palette, size) = hardware_palette(color0, color1);
    let mut block = [0; 8];
    block[0..2].copy_from_slice(&color0.to_be_bytes());
    block[2..4].copy_from_slice(&color1.to_be_bytes());
    let mut error = 0;
    for (i, color) in colors.iter().enumerate() {
        let (index, pixel_error) = nearest(color, &palette[..size]);
        block[4 + i / 4] |= (index as u8) << (6 - 2 * (i % 4));
        error += pixel_error;
    }
    (block, error)
}

/// Encodes the endpoints in the four color or three color mode, whichever has less error.
fn encode_best_mode(colors: &[Rgb; 16], a: u16, b: u16) -> ([u8; 8], i32) {
    let four_colors = encode_block(colors, a.max(b), a.min(b));
    let three_colors = encode_block(colors, a.min(b), a.max(b));
    if three_colors.1 < four_colors.1 {
        three_colors
    } else {
        four_colors
    }
}

/// Compresses a 4x4 block of colors to a GX CMPR block.
pub(crate) fn compress_block(colors: &[Rgb; 16]) -> [u8; 8] {
    let mut endpoints = base_colors(colors).map(|color| to_rgb565(&color));
    let (mut block, mut error) = encode_best_mode(colors, endpoints[0], endpoints[1]);

    // Step the channels of both endpoints while that lowers the error
    let mut improved = true;
    while improved && error > 0 {
        improved = false;
        for endpoint in 0..2 {
            for (shift, max) in CHANNELS_565 {
                for step in [-1, 1] {
                    let value = ((endpoints[endpoint] >> shift) & max) as i32 + step;
                    if !(0..=max as i32).contains(&value) {
                        continue;
                    }
                    let mut candidate = endpoints;
                    candidate[endpoint] =
                        (candidate[endpoint] & !(max << shift)) | ((value as u16) << shift);
                    let (candidate_block, candidate_error) =
                        encode_best_mode(colors, candidate[0], candidate[1]);
                    if candidate_error < error {
                        (block, error, endpoints) = (candidate_block, candidate_error, candidate);
                        improved = true;
                    }
                }
            }
        }
    }
    block
}

/// Picks the darkest and brightest color, weighted like luminance, then moves both by the
/// average error of the pixels that use them, like libtxc_dxtn.
fn base_colors(colors: &[Rgb; 16]) -> [Rgb; 2] {
    let luminance = |color: &Rgb| color_error(color, &[0; 3]);
    let mut base = [colors[0]; 2];
    for color in colors {
        if luminance(color) < luminance(&base[0]) {
            base[0] = *color;
        } else if luminance(color) > luminance(&base[1]) {
            base[1] = *color;
        }
    }

    // How much each palette color counts towards both base colors
    const SHARES: [[i32; 2]; 4] = [[3, 0], [0, 3], [2, 1], [1, 2]];
    let palette: [Rgb; 4] = [
        base[0],
        base[1],
        std::array::from_fn(|c| (2 * base[0][c] + base[1][c]) / 3),
        std::array::from_fn(|c| (base[0][c] + 2 * base[1][c]) / 3),
    ];
    let mut errors = [[0; 3]; 2];
    let mut counts = [0; 2];
    for color in colors {
        let (index, _) = nearest(color, &palette);
        for end in 0..2 {
            let share = SHARES[index][end];
            for c in 0..3 {
                errors[end][c] += share * (color[c] - palette[index][c]);
            }
            counts[end] += share;
        }
    }
    std::array::from_fn(|end| {
        std::array::from_fn(|c| (base[end][c] + errors[end][c] / counts[end].max(1)).clamp(0, 255))
    })
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    /// Returns the error of a block as the GX hardware decodes it.
    fn decoded_error(colors: &[Rgb; 16], block: &[u8; 8]) -> i32 {
        let color0 = u16::from_be_bytes([block[0], block[1]]);
        let color1 = u16::from_be_bytes([block[2], block[3]]);
        let (palette, _) = hardware_palette(color0, color1);
        colors
            .iter()
            .enumerate()
            .map(|(i, color)| {
                let index = block[4 + i / 4] >> (6 - 2 * (i % 4)) & 3;
                color_error(color, &palette[index as usize])
            })
            .sum()
    }

    #[test]
    fn compresses_solid_block_losslessly() {
        let colors = [[255, 255, 255]; 16];
        assert_eq!(decoded_error(&colors, &compress_block(&colors)), 0);
    }

    #[test]
    fn compresses_two_565_colors_losslessly() {
        let (a, b) = (from_rgb565(0xf800), from_rgb565(0x07ff));
        let colors = std::array::from_fn(|i| if i % 3 == 0 { a } else { b });
        assert_eq!(decoded_error(&colors, &compress_block(&colors)), 0);
    }

    #[test]
    fn search_improves_on_base_colors() {
        let colors: [Rgb; 16] = std::array::from_fn(|i| {
            let i = i as i32;
            [i * 16, 255 - i * 9, (i * 37) % 256]
        });
        let base = base_colors(&colors).map(|color| to_rgb565(&color));
        let (_, base_error) = encode_best_mode(&colors, base[0], base[1]);
        assert!(decoded_error(&colors, &compress_block(&colors)) < base_error);
    }
}
