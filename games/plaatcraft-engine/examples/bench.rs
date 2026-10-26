/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Times chunk generation, meshing and storage on the host, the same chunks as the Wii
//! port's benchmark. Run with `cargo run --release -p plaatcraft-engine --example bench`.

use std::time::{Duration, Instant};

use plaatcraft_engine::world::{self, ChunkData, QUAD_BYTES};

// A grid of chunks at spawn and one in the hills, to cover several biomes
const GRIDS: [(i64, i64); 2] = [(-2, -2), (-66, -2)];
const GRID_SIZE: i64 = 4;

fn main() {
    world::set_seed(world::WORLD_SEED);
    let mut totals = [Duration::ZERO; 4];
    let mut chunks = 0;
    let mut quads = 0;
    for (x, z) in GRIDS {
        for dz in 0..GRID_SIZE {
            for dx in 0..GRID_SIZE {
                let start = Instant::now();
                let chunk = ChunkData::generate((x + dx, z + dz));
                let generated = Instant::now();
                let packed = chunk.mesh();
                let meshed = Instant::now();
                let blocks = chunk.encode();
                let encoded = Instant::now();
                assert!(ChunkData::decode(&blocks).is_ok());
                let phases = [start, generated, meshed, encoded, Instant::now()];
                for (total, pair) in totals.iter_mut().zip(phases.windows(2)) {
                    *total += pair[1] - pair[0];
                }
                chunks += 1;
                quads += packed.len() / QUAD_BYTES;
            }
        }
    }
    let average = |total: Duration| total.as_secs_f64() * 1000.0 / f64::from(chunks);
    println!(
        "{chunks} chunks, {} quads per chunk, ms per chunk: generate {:.2}, mesh {:.2}, encode {:.2}, decode {:.2}, total {:.2}",
        quads / chunks as usize,
        average(totals[0]),
        average(totals[1]),
        average(totals[2]),
        average(totals[3]),
        average(totals.iter().sum()),
    );
}
