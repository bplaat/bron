/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Times the chunk pipeline of the stream worker on the Wii CPU, built with the `bench`
//! feature and run by `bench.sh`. The results go to stdout, which is Dolphin's debug UART.

use alloc::format;
use alloc::vec::Vec;

use libogc_sys::{gettime, puts};
use plaatcraft_engine::world::{self, ChunkData, Coord, QUAD_BYTES};

use crate::TICKS_PER_SECOND;
use crate::stream::Store;
use crate::sync::Mutex;

// A grid of chunks at spawn and one in the hills, to cover several biomes
const GRIDS: [(i64, i64); 2] = [(-2, -2), (-66, -2)];
const GRID_SIZE: i64 = 4;

fn print(line: &str) {
    let line = format!("{line}\0");
    unsafe { puts(line.as_ptr().cast()) };
}

fn milliseconds(ticks: u64) -> f64 {
    ticks as f64 * 1000.0 / TICKS_PER_SECOND
}

pub fn run() -> ! {
    let coords: Vec<Coord> = GRIDS
        .iter()
        .flat_map(|&(x, z)| {
            (0..GRID_SIZE * GRID_SIZE).map(move |i| (x + i % GRID_SIZE, z + i / GRID_SIZE))
        })
        .collect();
    let time = || unsafe { gettime() };
    // The stream worker reuses the neighbors of the chunks it builds after this one, here
    // without a world, so nothing is stored
    let mut store = Store::new(world::WORLD_SEED);
    let sqlite = Mutex::new(None);
    let start = time();
    for (index, &coord) in coords.iter().enumerate() {
        store.build(&sqlite, coord, &coords[index + 1..]);
    }
    let streamed = time() - start;
    let mut totals = [0u64; 7];
    let mut quads = 0;
    for &coord in &coords {
        let start = time();
        let chunk = ChunkData::generate(world::WORLD_SEED, coord);
        let generated = time();
        let packed = chunk.mesh();
        let meshed = time();
        let mesh = crate::mesh::build(&packed);
        let listed = time();
        let blocks = chunk.encode();
        let encoded = time();
        let decoded = ChunkData::decode(&blocks).map(|chunk| chunk.voxel([0, 0, 0]));
        let unpacked = time();
        let stored = chunk.encode_blocks();
        let saved = time();
        let loaded = ChunkData::decode_blocks(&stored);
        let end = time();
        assert!(decoded.is_ok() && loaded.is_ok() && !mesh.list.is_empty());
        let phases = [
            start, generated, meshed, listed, encoded, unpacked, saved, end,
        ];
        for (total, pair) in totals.iter_mut().zip(phases.windows(2)) {
            *total += pair[1] - pair[0];
        }
        quads += packed.len() / QUAD_BYTES;
    }
    let chunks = coords.len();
    let average = |ticks| milliseconds(ticks) / chunks as f64;
    print(&format!(
        "BENCH {chunks} chunks, {} quads per chunk, ms per chunk: generate {:.1}, streamed {:.1}, mesh {:.1}, display list {:.1}, encode {:.1}, decode {:.1}, store {:.1}, load {:.1}, total {:.1}",
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
    ));
    // Returning reloads the system menu, the script stops Dolphin after the results
    loop {
        core::hint::spin_loop();
    }
}
