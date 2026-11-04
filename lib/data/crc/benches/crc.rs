/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(missing_docs)]

use std::hint::black_box;

use crc::{CRC_32_ISO_HDLC, Crc};
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

static CRC32: Crc<u32> = Crc::<u32>::new(&CRC_32_ISO_HDLC);

fn checksum(c: &mut Criterion) {
    let mut group = c.benchmark_group("crc32_checksum");
    for size in [
        0,
        1,
        7,
        8,
        15,
        16,
        31,
        32,
        63,
        64,
        65,
        1024,
        8 * 1024,
        10 * 1024,
        12 * 1024,
        16 * 1024,
        64 * 1024,
        1024 * 1024,
    ] {
        let input = (0..size).map(|i| i as u8).collect::<Vec<_>>();
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::new("bytes", size), &input, |b, input| {
            b.iter(|| black_box(CRC32.checksum(black_box(input))));
        });
    }
    group.finish();
}

criterion_group!(benches, checksum);
criterion_main!(benches);
