/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Mipmap filtering in linear light that keeps the alpha test coverage of cutouts.

/// The fraction of pixels that pass an alpha test at half opacity.
pub(crate) fn coverage(pixels: &[u8]) -> f32 {
    let pixels = pixels.as_chunks::<4>().0;
    pixels.iter().filter(|pixel| pixel[3] >= 128).count() as f32 / pixels.len() as f32
}

/// Scales the alpha of a mipmap so as many pixels pass the alpha test as in the full image,
/// otherwise cutouts like leaves fade away in the distance.
pub(crate) fn preserve_coverage(pixels: &mut [u8], coverage: f32) {
    if coverage == 0.0 || coverage == 1.0 {
        return;
    }
    let mut alphas: Vec<_> = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| pixel[3])
        .collect();
    alphas.sort_unstable_by(|a, b| b.cmp(a));
    let target = ((coverage * alphas.len() as f32).round() as usize).clamp(1, alphas.len());
    let threshold = f32::from(alphas[target - 1]);
    if threshold == 0.0 {
        return;
    }
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel[3] = (f32::from(pixel[3]) * 128.0 / threshold).round().min(255.0) as u8;
    }
}

/// Halves an RGBA image by averaging 2x2 pixels in linear light, like GPUs filter sRGB
/// textures. Colors are weighted by alpha, so transparent pixels don't darken the edges of
/// cutouts.
pub(crate) fn downsample(pixels: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut output = Vec::with_capacity(width * height);
    for y in (0..height).step_by(2) {
        for x in (0..width).step_by(2) {
            let mut color = [0.0f32; 3];
            let mut alpha = 0.0;
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let pixel = &pixels[((y + dy) * width + x + dx) * 4..];
                let weight = f32::from(pixel[3]) / 255.0;
                for channel in 0..3 {
                    color[channel] += image::srgb_to_linear(pixel[channel]) * weight;
                }
                alpha += weight;
            }
            for channel in color {
                output.push(if alpha > 0.0 {
                    image::linear_to_srgb(channel / alpha)
                } else {
                    0
                });
            }
            output.push((alpha / 4.0 * 255.0).round() as u8);
        }
    }
    output
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_in_linear_light_and_keeps_coverage() {
        // Transparent pixels don't darken their neighbors
        let pixels = [[200, 100, 50, 255], [0; 4], [0; 4], [0; 4]].concat();
        assert_eq!(downsample(&pixels, 2, 2), [200, 100, 50, 64]);
        // Black and white average in linear light, like the GPU filters sRGB textures
        let pixels = [[0, 0, 0, 255], [255; 4], [0, 0, 0, 255], [255; 4]].concat();
        assert_eq!(downsample(&pixels, 2, 2), [188, 188, 188, 255]);
        // Stripes average to just under half opacity, which would fail the alpha test
        let mut pixels = downsample(&[[0, 0, 0, 250], [0; 4]].repeat(8).concat(), 4, 4);
        assert_eq!(coverage(&pixels), 0.0);
        preserve_coverage(&mut pixels, 0.5);
        assert_eq!(coverage(&pixels), 1.0);
    }
}
