/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(missing_docs)]

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use plaatcraft_engine::world::{ChunkData, WORLD_SEED};

fn world(c: &mut Criterion) {
    let mut group = c.benchmark_group("world_chunk");
    group.throughput(Throughput::Elements(1));
    for (name, coord) in [("spawn", (0, 0)), ("hills", (-64, 0))] {
        let chunk = ChunkData::generate(WORLD_SEED, coord);
        let encoded = chunk.encode();
        group.bench_function(&format!("generate/{name}"), |b| {
            b.iter(|| black_box(ChunkData::generate(black_box(WORLD_SEED), black_box(coord))));
        });
        group.bench_function(&format!("mesh/{name}"), |b| {
            b.iter(|| black_box(black_box(&chunk).mesh()));
        });
        group.bench_function(&format!("encode/{name}"), |b| {
            b.iter(|| black_box(black_box(&chunk).encode()));
        });
        group.bench_function(&format!("decode/{name}"), |b| {
            b.iter(|| black_box(ChunkData::decode(black_box(&encoded)).expect("valid chunk")));
        });
    }
    group.finish();
}

criterion_group!(benches, world);
criterion_main!(benches);
