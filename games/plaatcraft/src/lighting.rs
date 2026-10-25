/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::collections::VecDeque;

pub(crate) const MAX_LIGHT: u8 = 15;

// Low nibble: skylight. High nibble: block light. Only direct skylight travels
// downward without loss through air; all flood-filled light loses at least one level.
pub(crate) fn bake(
    blocks: &[u8],
    width: usize,
    height: usize,
    opacity: impl Fn(u8) -> u8,
    emission: impl Fn(u8) -> u8,
) -> Vec<u8> {
    assert_eq!(blocks.len(), width * width * height);
    let mut light = vec![0; blocks.len()];
    for column in 0..width * width {
        let mut sky = MAX_LIGHT;
        for y in (0..height).rev() {
            let index = column * height + y;
            sky = sky.saturating_sub(opacity(blocks[index]));
            light[index] = sky | emission(blocks[index]) << 4;
        }
    }
    let neighbors = |index: usize| {
        let y = index % height;
        let column = index / height;
        let x = column % width;
        let z = column / width;
        [
            (y > 0).then(|| index - 1),
            (y + 1 < height).then_some(index + 1),
            (x > 0).then(|| index - height),
            (x + 1 < width).then_some(index + height),
            (z > 0).then(|| index - width * height),
            (z + 1 < width).then_some(index + width * height),
        ]
    };
    let transmitted = |source: u8, block: u8| {
        let loss = opacity(block).max(1);
        (source & 15).saturating_sub(loss) | (source >> 4).saturating_sub(loss) << 4
    };
    let merge = |a: u8, b: u8| (a & 15).max(b & 15) | (a >> 4).max(b >> 4) << 4;
    let mut queue = VecDeque::new();
    // Seed only frontiers, not every voxel in the open sky.
    for (index, &value) in light.iter().enumerate() {
        if value == 0 {
            continue;
        }
        if neighbors(index)
            .into_iter()
            .flatten()
            .any(|next| merge(light[next], transmitted(value, blocks[next])) != light[next])
        {
            queue.push_back(index);
        }
    }
    while let Some(index) = queue.pop_front() {
        for next in neighbors(index).into_iter().flatten() {
            let value = merge(light[next], transmitted(light[index], blocks[next]));
            if value != light[next] {
                light[next] = value;
                queue.push_back(next);
            }
        }
    }
    light
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solve(blocks: &[u8], width: usize, height: usize) -> Vec<u8> {
        bake(
            blocks,
            width,
            height,
            |b| match b {
                0 => 0,
                2 => 2,
                _ => 15,
            },
            |b| if b == 3 { 15 } else { 0 },
        )
    }

    #[test]
    fn sky_water_and_sealed_rooms() {
        assert_eq!(solve(&[0; 8], 1, 8), vec![15; 8]);
        let water = solve(&[0, 0, 2, 2, 2, 0, 0, 0], 1, 8);
        assert_eq!(water, [9, 9, 9, 11, 13, 15, 15, 15]);
        let covered = solve(&[0, 0, 0, 1, 0, 0, 0, 0], 1, 8);
        assert_eq!(&covered[..4], &[0; 4]);
    }

    #[test]
    fn skylight_spreads_under_overhangs_with_distance_falloff() {
        let mut blocks = vec![0; 7 * 7 * 7];
        let at = |x: usize, y: usize, z: usize| (z * 7 + x) * 7 + y;
        for z in 1..6 {
            for x in 1..6 {
                blocks[at(x, 4, z)] = 1;
            }
        }
        let light = solve(&blocks, 7, 7);
        assert_eq!(light[at(0, 3, 3)], 15);
        assert_eq!(light[at(1, 3, 3)], 14);
        assert_eq!(light[at(3, 3, 3)], 12);
        assert_eq!(light[at(3, 4, 3)], 0);
    }

    #[test]
    fn block_light_falls_off_and_does_not_cross_solid_walls() {
        let mut blocks = vec![0; 7 * 7 * 7];
        let at = |x: usize, y: usize, z: usize| (z * 7 + x) * 7 + y;
        blocks[at(3, 3, 3)] = 3;
        for z in 0..7 {
            for y in 0..7 {
                blocks[at(5, y, z)] = 1;
            }
        }
        let light = solve(&blocks, 7, 7);
        assert_eq!(light[at(3, 3, 3)] >> 4, 15);
        assert_eq!(light[at(2, 3, 3)] >> 4, 14);
        assert_eq!(light[at(1, 2, 3)] >> 4, 12);
        assert_eq!(light[at(6, 3, 3)] >> 4, 0);
    }
}
