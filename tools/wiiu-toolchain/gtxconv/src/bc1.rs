/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A BC1 (DXT1) compressor. It starts with the base colors of the libtxc_dxtn compressor, then
//! searches nearby 565 colors for the lowest error with the palette the GPU decodes.

pub(crate) type Rgb = [i32; 3];

/// The error weights per channel, roughly the squared weights of the luminance conversion.
const WEIGHTS: Rgb = [4, 16, 1];

/// The bit offset and maximum value of the red, green and blue channels of a 565 color.
const CHANNELS_565: [(u16, u16); 3] = [(11, 31), (5, 63), (0, 31)];

const fn color_error(a: &Rgb, b: &Rgb) -> i32 {
    let [red, green, blue] = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    red * red * WEIGHTS[0] + green * green * WEIGHTS[1] + blue * blue * WEIGHTS[2]
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

/// The red, green and blue samples of a block.
type Planes = [[i32; 16]; 3];

fn split_planes(colors: &[Rgb; 16]) -> Planes {
    std::array::from_fn(|channel| std::array::from_fn(|pixel| colors[pixel][channel]))
}

const fn to_rgb565(color: &Rgb) -> u16 {
    ((color[0] as u16 & 0xf8) << 8) | ((color[1] as u16 & 0xfc) << 3) | (color[2] as u16 >> 3)
}

fn from_rgb565(color: u16) -> Rgb {
    let [r, g, b] = CHANNELS_565.map(|(shift, max)| i32::from((color >> shift) & max));
    [r << 3 | r >> 2, g << 2 | g >> 4, b << 3 | b >> 2]
}

/// Returns the colors the GPU decodes: four colors blended at 1/3 and 2/3 when color0 > color1,
/// otherwise three colors (the fourth is transparent).
fn hardware_palette(color0: u16, color1: u16) -> ([Rgb; 4], usize) {
    let (a, b) = (from_rgb565(color0), from_rgb565(color1));
    if color0 > color1 {
        let blend = |x: &Rgb, y: &Rgb| std::array::from_fn(|c| (x[c] * 2 + y[c]) / 3);
        ([a, b, blend(&a, &b), blend(&b, &a)], 4)
    } else {
        (
            [a, b, std::array::from_fn(|c| (a[c] + b[c]) / 2), [0; 3]],
            3,
        )
    }
}

/// Encodes a BC1 block: little endian colors and the first pixel of a row in the low bits of
/// its index byte. Returns the block and its error.
fn encode_block(planes: &Planes, color0: u16, color1: u16) -> ([u8; 8], i32) {
    let (palette, size) = hardware_palette(color0, color1);
    let mut block = [0; 8];
    block[0..2].copy_from_slice(&color0.to_le_bytes());
    block[2..4].copy_from_slice(&color1.to_le_bytes());
    // Separate channels and loops of one element width keep the pixels independent for SIMD.
    let mut errors = [i32::MAX; 16];
    let mut indices = [0i32; 16];
    for (index, entry) in palette[..size].iter().enumerate() {
        let mut candidates = [0; 16];
        for ((plane, value), weight) in planes.iter().zip(entry).zip(WEIGHTS) {
            for (candidate, sample) in candidates.iter_mut().zip(plane) {
                *candidate += (sample - value) * (sample - value) * weight;
            }
        }
        for ((error, nearest), candidate) in errors.iter_mut().zip(&mut indices).zip(candidates) {
            *nearest = if candidate < *error {
                index as i32
            } else {
                *nearest
            };
            *error = candidate.min(*error);
        }
    }
    for (i, index) in indices.into_iter().enumerate() {
        block[4 + i / 4] |= (index as u8) << (2 * (i % 4));
    }
    (block, errors.iter().sum())
}

/// Encodes the endpoints in the four color or three color mode, whichever has less error.
fn encode_best_mode(planes: &Planes, a: u16, b: u16) -> ([u8; 8], i32) {
    let four_colors = encode_block(planes, a.max(b), a.min(b));
    let three_colors = encode_block(planes, a.min(b), a.max(b));
    if three_colors.1 < four_colors.1 {
        three_colors
    } else {
        four_colors
    }
}

/// Compresses a 4x4 block of colors, row by row, to a BC1 block.
pub(crate) fn compress_block(colors: &[Rgb; 16]) -> [u8; 8] {
    let mut endpoints = base_colors(colors).map(|color| to_rgb565(&color));
    let planes = split_planes(colors);
    let (mut block, mut error) = encode_best_mode(&planes, endpoints[0], endpoints[1]);

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
                        encode_best_mode(&planes, candidate[0], candidate[1]);
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

    /// Returns the error of a block as the GPU decodes it.
    fn decoded_error(colors: &[Rgb; 16], block: &[u8; 8]) -> i32 {
        let color0 = u16::from_le_bytes([block[0], block[1]]);
        let color1 = u16::from_le_bytes([block[2], block[3]]);
        let (palette, _) = hardware_palette(color0, color1);
        colors
            .iter()
            .enumerate()
            .map(|(i, color)| {
                let index = block[4 + i / 4] >> (2 * (i % 4)) & 3;
                color_error(color, &palette[index as usize])
            })
            .sum()
    }

    #[test]
    fn block_search_matches_nearest_palette_colors() {
        let mut state = 0x2545_f491u32;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state
        };
        for _ in 0..1000 {
            let colors: [Rgb; 16] = std::array::from_fn(|_| {
                let value = next();
                [value & 255, value >> 8 & 255, value >> 16 & 255].map(|sample| sample as i32)
            });
            let (color0, color1) = (next() as u16, next() as u16);
            let (palette, size) = hardware_palette(color0, color1);
            let (block, error) = encode_block(&split_planes(&colors), color0, color1);
            let mut expected = block;
            expected[4..].fill(0);
            let mut expected_error = 0;
            for (i, color) in colors.iter().enumerate() {
                let (index, pixel_error) = nearest(color, &palette[..size]);
                expected[4 + i / 4] |= (index as u8) << (2 * (i % 4));
                expected_error += pixel_error;
            }
            assert_eq!((block, error), (expected, expected_error));
        }
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
        let (_, base_error) = encode_best_mode(&split_planes(&colors), base[0], base[1]);
        assert!(decoded_error(&colors, &compress_block(&colors)) < base_error);
    }

    #[test]
    fn writes_little_endian_blocks_with_the_first_pixel_in_the_low_bits() {
        let (a, b) = (from_rgb565(0xf800), from_rgb565(0x001f));
        let mut colors = [b; 16];
        colors[0] = a;
        let block = compress_block(&colors);
        assert_eq!(block[..4], [0x00, 0xf8, 0x1f, 0x00]);
        assert_eq!(block[4..], [0b01_01_01_00, 0x55, 0x55, 0x55]);
    }
}
