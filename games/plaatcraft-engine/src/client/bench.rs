/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Times the chunk pipeline of the stream's worker on the console's CPU, the ports run it
//! instead of the game for their benchmark.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::mesh;
use super::platform::{Gpu, Platform};
use super::storage::Sqlite;
use super::stream::Store;
use super::sync::Mutex;
use crate::world::{self, ChunkData, Coord, QUAD_BYTES};

// A grid of chunks at spawn and one in the hills, to cover several biomes
const GRIDS: [(i64, i64); 2] = [(-2, -2), (-66, -2)];
const GRID_SIZE: i64 = 4;

/// Builds the benchmark's chunks and returns a line with the milliseconds per chunk of each
/// step, starting with `BENCH`.
pub fn bench<P: Platform>() -> String {
    let coords: Vec<Coord> = GRIDS
        .iter()
        .flat_map(|&(x, z)| {
            (0..GRID_SIZE * GRID_SIZE).map(move |i| (x + i % GRID_SIZE, z + i / GRID_SIZE))
        })
        .collect();
    // The stream worker reuses the neighbors of the chunks it builds after this one, here
    // without a world, so nothing is stored
    let mut store = Store::new(world::WORLD_SEED);
    let sqlite: Sqlite<P> = Mutex::new(None);
    let start = P::seconds();
    for (index, &coord) in coords.iter().enumerate() {
        store.build::<P>(&sqlite, coord, &coords[index + 1..]);
    }
    let streamed = P::seconds() - start;
    let mut totals = [0.0; 7];
    let mut quads = 0;
    for &coord in &coords {
        let start = P::seconds();
        let chunk = ChunkData::generate(world::WORLD_SEED, coord);
        let generated = P::seconds();
        let packed = chunk.mesh();
        let meshed = P::seconds();
        let built = P::Gpu::mesh(&mesh::build(&packed, P::Gpu::has_alpha));
        let converted = P::seconds();
        let blocks = chunk.encode();
        let encoded = P::seconds();
        let decoded = ChunkData::decode(&blocks).map(|chunk| chunk.voxel([0, 0, 0]));
        let unpacked = P::seconds();
        let stored = chunk.encode_blocks();
        let saved = P::seconds();
        let loaded = ChunkData::decode_blocks(&stored);
        let end = P::seconds();
        assert!(decoded.is_ok() && loaded.is_ok());
        drop(built);
        let phases = [
            start, generated, meshed, converted, encoded, unpacked, saved, end,
        ];
        for (total, pair) in totals.iter_mut().zip(phases.windows(2)) {
            *total += pair[1] - pair[0];
        }
        quads += packed.len() / QUAD_BYTES;
    }
    let chunks = coords.len();
    let average = |seconds: f64| seconds * 1000.0 / chunks as f64;
    format!(
        "BENCH {chunks} chunks, {} quads per chunk, ms per chunk: generate {:.1}, streamed {:.1}, mesh {:.1}, GPU mesh {:.1}, encode {:.1}, decode {:.1}, store {:.1}, load {:.1}, total {:.1}",
        quads / chunks,
        average(totals[0]),
        average(streamed),
        average(totals[1]),
        average(totals[2]),
        average(totals[3]),
        average(totals[4]),
        average(totals[5]),
        average(totals[6]),
        average(totals.iter().sum()),
    )
}
