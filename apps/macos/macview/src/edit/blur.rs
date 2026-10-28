/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Gaussian blur approximated by three running-sum box blurs over premultiplied channels.

use super::par_bands;

// The number of box blurs that approximate one gaussian.
const PASSES: usize = 3;

// Returns the pixel distance beyond which a gaussian blur of `sigma` has no visible effect.
pub(super) fn margin(sigma: f64) -> u32 {
    if sigma < 0.5 {
        0
    } else {
        (sigma * 3.0).ceil() as u32 + 1
    }
}

// Blurs premultiplied RGBA channels in place with a gaussian of standard deviation `sigma`
// pixels, extending the edges; a `sigma` below 0.5 is a no-op.
pub(super) fn gaussian(plane: &mut [u16], width: usize, height: usize, sigma: f64) {
    if sigma < 0.5 {
        return;
    }
    let mut scratch = vec![0; plane.len()];
    for radius in box_radii(sigma) {
        if radius > 0 {
            horizontal(plane, &mut scratch, width, radius);
            vertical(&scratch, plane, width, height, radius);
        }
    }
}

// Returns the box radii whose successive blurs best approximate a gaussian of `sigma`.
fn box_radii(sigma: f64) -> [usize; PASSES] {
    let passes = PASSES as f64;
    let variance = 12.0 * sigma * sigma;
    let mut lower = (variance / passes + 1.0).sqrt().floor() as usize;
    if lower.is_multiple_of(2) {
        lower = lower.saturating_sub(1).max(1);
    }
    let l = lower as f64;
    let lower_count = ((variance - passes * l * l - 4.0 * passes * l - 3.0 * passes)
        / (-4.0 * l - 4.0))
        .round()
        .max(0.0) as usize;
    std::array::from_fn(|pass| {
        let size = if pass < lower_count { lower } else { lower + 2 };
        (size - 1) / 2
    })
}

// Box blurs each row of `source` into `target`.
fn horizontal(source: &[u16], target: &mut [u16], width: usize, radius: usize) {
    let stride = width * 4;
    let count = (2 * radius + 1) as u32;
    par_bands(target, stride, |first_row, band| {
        for (row, out) in source[first_row * stride..]
            .chunks_exact(stride)
            .zip(band.chunks_exact_mut(stride))
        {
            for channel in 0..4 {
                let at = |x: usize| u32::from(row[x.min(width - 1) * 4 + channel]);
                let mut sum = at(0) * radius as u32 + (0..=radius).map(at).sum::<u32>();
                for x in 0..width {
                    out[x * 4 + channel] = ((sum + count / 2) / count) as u16;
                    sum += at(x + radius + 1);
                    sum -= at(x.saturating_sub(radius));
                }
            }
        }
    });
}

// Box blurs each column of `source` into `target`, keeping running sums for whole rows.
fn vertical(source: &[u16], target: &mut [u16], width: usize, height: usize, radius: usize) {
    let stride = width * 4;
    let count = (2 * radius + 1) as u32;
    let row = |y: usize| &source[y.min(height - 1) * stride..][..stride];
    par_bands(target, stride, |first_row, band| {
        let mut sums = vec![0u32; stride];
        for offset in 0..=2 * radius {
            let y = (first_row + offset).saturating_sub(radius);
            for (sum, &value) in sums.iter_mut().zip(row(y)) {
                *sum += u32::from(value);
            }
        }
        for (index, out) in band.chunks_exact_mut(stride).enumerate() {
            let y = first_row + index;
            for (value, &sum) in out.iter_mut().zip(&sums) {
                *value = ((sum + count / 2) / count) as u16;
            }
            for ((sum, &add), &remove) in sums
                .iter_mut()
                .zip(row(y + radius + 1))
                .zip(row(y.saturating_sub(radius)))
            {
                *sum = *sum + u32::from(add) - u32::from(remove);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::Canvas;

    #[test]
    fn radii_approximate_sigma() {
        for sigma in [0.75, 1.5, 4.0, 25.0] {
            let variance: f64 = box_radii(sigma)
                .iter()
                .map(|&r| ((2 * r + 1).pow(2) - 1) as f64 / 12.0)
                .sum();
            assert!(
                (variance.sqrt() - sigma).abs() < 0.35,
                "{sigma}: {variance}"
            );
        }
    }

    #[test]
    fn flat_stays_flat() {
        let canvas = Canvas::new(9, 7, [40, 120, 200, 255].repeat(63));
        let mut plane = canvas.premultiplied();
        gaussian(&mut plane, 9, 7, 3.0);
        assert_eq!(Canvas::from_premultiplied(9, 7, &plane), canvas);
    }

    #[test]
    fn spreads_smoothly() {
        let mut plane = vec![0u16; 11 * 11 * 4];
        plane[(5 * 11 + 5) * 4 + 3] = 65025;
        gaussian(&mut plane, 11, 11, 2.0);
        let center = plane[(5 * 11 + 5) * 4 + 3];
        let neighbour = plane[(5 * 11 + 6) * 4 + 3];
        assert!(center < 65025 && neighbour > 0 && neighbour < center);
    }
}
