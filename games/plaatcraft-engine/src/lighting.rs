/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use alloc::collections::VecDeque;
use alloc::vec;
use alloc::vec::Vec;

pub(crate) const MAX_LIGHT: u8 = 15;

// Low nibble: skylight. High nibble: block light. Only direct skylight travels
// downward without loss through air; all flood-filled light loses at least one level.
// The sizes are constants, so the neighbor math is shifts and multiplies instead of the
// divisions that are slow on the Wii.
pub(crate) fn bake<const WIDTH: usize, const HEIGHT: usize>(
    blocks: &[u8],
    opacity: impl Fn(u8) -> u8,
    emission: impl Fn(u8) -> u8,
) -> Vec<u8> {
    assert_eq!(blocks.len(), WIDTH * WIDTH * HEIGHT);
    let opacity: [u8; 256] = core::array::from_fn(|material| opacity(material as u8));
    let emission: [u8; 256] = core::array::from_fn(|material| emission(material as u8));
    let mut light = vec![0; blocks.len()];
    // Each column is open sky of air, material 0, above its highest block. Below that
    // skylight fades until `dark`, under which only light sources are lit.
    let mut ground = vec![0; WIDTH * WIDTH];
    let mut dark = vec![0; WIDTH * WIDTH];
    let mut sources = Vec::new();
    for (column, (light, blocks)) in light
        .as_chunks_mut::<HEIGHT>()
        .0
        .iter_mut()
        .zip(blocks.as_chunks::<HEIGHT>().0)
        .enumerate()
    {
        let top = blocks
            .iter()
            .rposition(|&block| block != 0)
            .map_or(0, |y| y + 1);
        light[top..].fill(MAX_LIGHT);
        let mut sky = MAX_LIGHT;
        let mut y = top;
        while y > 0 && sky > 0 {
            y -= 1;
            let block = usize::from(blocks[y]);
            sky = sky.saturating_sub(opacity[block]);
            light[y] = sky | emission[block] << 4;
        }
        for (y, &block) in blocks[..y].iter().enumerate() {
            let emitted = emission[usize::from(block)];
            if emitted != 0 {
                light[y] = emitted << 4;
                sources.push(column * HEIGHT + y);
            }
        }
        ground[column] = top;
        dark[column] = y;
    }
    let neighbors = |index: usize| {
        let y = index % HEIGHT;
        let column = index / HEIGHT;
        let x = column % WIDTH;
        let z = column / WIDTH;
        [
            (y > 0).then(|| index - 1),
            (y + 1 < HEIGHT).then_some(index + 1),
            (x > 0).then(|| index - HEIGHT),
            (x + 1 < WIDTH).then_some(index + HEIGHT),
            (z > 0).then(|| index - WIDTH * HEIGHT),
            (z + 1 < WIDTH).then_some(index + WIDTH * HEIGHT),
        ]
    };
    let transmitted = |source: u8, block: u8| {
        let loss = opacity[usize::from(block)].max(1);
        (source & 15).saturating_sub(loss) | (source >> 4).saturating_sub(loss) << 4
    };
    let merge = |a: u8, b: u8| (a & 15).max(b & 15) | (a >> 4).max(b >> 4) << 4;
    let mut queue = VecDeque::new();
    let mut seed = |index: usize| {
        let value = light[index];
        if value != 0
            && neighbors(index)
                .into_iter()
                .flatten()
                .any(|next| merge(light[next], transmitted(value, blocks[next])) != light[next])
        {
            queue.push_back(index as u32);
        }
    };
    // Seed only frontiers: the light sources in the dark and the lit part of each column.
    // Open sky above a column and its four neighbors is full skylight surrounded by full
    // skylight, so it can't spread.
    sources.into_iter().for_each(&mut seed);
    for z in 0..WIDTH {
        for x in 0..WIDTH {
            let column = z * WIDTH + x;
            let mut limit = ground[column] + 1;
            for (next, inside) in [
                (column.wrapping_sub(1), x > 0),
                (column + 1, x + 1 < WIDTH),
                (column.wrapping_sub(WIDTH), z > 0),
                (column + WIDTH, z + 1 < WIDTH),
            ] {
                if inside {
                    limit = limit.max(ground[next]);
                }
            }
            (column * HEIGHT + dark[column]..column * HEIGHT + limit.min(HEIGHT))
                .for_each(&mut seed);
        }
    }
    while let Some(index) = queue.pop_front() {
        let index = index as usize;
        for next in neighbors(index).into_iter().flatten() {
            let value = merge(light[next], transmitted(light[index], blocks[next]));
            if value != light[next] {
                light[next] = value;
                queue.push_back(next as u32);
            }
        }
    }
    light
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solve<const WIDTH: usize, const HEIGHT: usize>(blocks: &[u8]) -> Vec<u8> {
        bake::<WIDTH, HEIGHT>(
            blocks,
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
        assert_eq!(solve::<1, 8>(&[0; 8]), vec![15; 8]);
        let water = solve::<1, 8>(&[0, 0, 2, 2, 2, 0, 0, 0]);
        assert_eq!(water, [9, 9, 9, 11, 13, 15, 15, 15]);
        let covered = solve::<1, 8>(&[0, 0, 0, 1, 0, 0, 0, 0]);
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
        let light = solve::<7, 7>(&blocks);
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
        let light = solve::<7, 7>(&blocks);
        assert_eq!(light[at(3, 3, 3)] >> 4, 15);
        assert_eq!(light[at(2, 3, 3)] >> 4, 14);
        assert_eq!(light[at(1, 2, 3)] >> 4, 12);
        assert_eq!(light[at(6, 3, 3)] >> 4, 0);
    }

    #[test]
    fn matches_relaxing_every_voxel_until_nothing_changes() {
        const WIDTH: usize = 7;
        const HEIGHT: usize = 16;
        // Opaque ground with caves, light sources, water and leaves, also buried in the dark
        let mut random = 0x2545_f491u32;
        let mut next = || {
            random ^= random << 13;
            random ^= random >> 17;
            random ^= random << 5;
            random
        };
        for _ in 0..20 {
            let blocks: Vec<u8> = (0..WIDTH * WIDTH * HEIGHT)
                .map(|index| match (next() % 16, index % HEIGHT < 10) {
                    (0, _) => 3,
                    (1, _) => 2,
                    (2..5, true) => 0,
                    (_, true) => 1,
                    (5, false) => 4,
                    _ => 0,
                })
                .collect();
            let opacity = |block: u8| match block {
                0 => 0,
                2 => 2,
                4 => 1,
                _ => 15,
            };
            let emission = |block: u8| if block == 3 { 15 } else { 0 };
            let mut expected = vec![0; blocks.len()];
            for column in 0..WIDTH * WIDTH {
                let mut sky = MAX_LIGHT;
                for y in (0..HEIGHT).rev() {
                    let block = blocks[column * HEIGHT + y];
                    sky = sky.saturating_sub(opacity(block));
                    expected[column * HEIGHT + y] = sky | emission(block) << 4;
                }
            }
            let mut changed = true;
            while changed {
                changed = false;
                for index in 0..blocks.len() {
                    let (y, column) = (index % HEIGHT, index / HEIGHT);
                    let (x, z) = (column % WIDTH, column / WIDTH);
                    for (inside, next) in [
                        (y > 0, index.wrapping_sub(1)),
                        (y + 1 < HEIGHT, index + 1),
                        (x > 0, index.wrapping_sub(HEIGHT)),
                        (x + 1 < WIDTH, index + HEIGHT),
                        (z > 0, index.wrapping_sub(WIDTH * HEIGHT)),
                        (z + 1 < WIDTH, index + WIDTH * HEIGHT),
                    ] {
                        if !inside {
                            continue;
                        }
                        let loss = opacity(blocks[next]).max(1);
                        let sky = (expected[index] & 15).saturating_sub(loss);
                        let block = (expected[index] >> 4).saturating_sub(loss);
                        let value =
                            sky.max(expected[next] & 15) | block.max(expected[next] >> 4) << 4;
                        changed |= value != expected[next];
                        expected[next] = value;
                    }
                }
            }
            assert_eq!(bake::<WIDTH, HEIGHT>(&blocks, opacity, emission), expected);
        }
    }
}
