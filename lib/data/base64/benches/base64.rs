/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(missing_docs)]

use std::hint::black_box;

use base64::prelude::*;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

fn codecs(c: &mut Criterion) {
    for (name, engine) in [
        ("standard", &BASE64_STANDARD),
        ("no_pad", &BASE64_STANDARD_NO_PAD),
    ] {
        let mut encode = c.benchmark_group(format!("base64_{name}_encode"));
        for size in [16, 1024, 1024 * 1024] {
            let input = (0..size).map(|i| i as u8).collect::<Vec<_>>();
            encode.throughput(Throughput::Bytes(size as u64));
            encode.bench_with_input(BenchmarkId::new("bytes", size), &input, |b, input| {
                b.iter(|| black_box(engine.encode(black_box(input))));
            });
        }
        encode.finish();

        let mut decode = c.benchmark_group(format!("base64_{name}_decode"));
        for size in [16, 1024, 1024 * 1024] {
            let input = (0..size).map(|i| i as u8).collect::<Vec<_>>();
            let encoded = engine.encode(&input);
            decode.throughput(Throughput::Bytes(encoded.len() as u64));
            decode.bench_with_input(BenchmarkId::new("bytes", size), &encoded, |b, encoded| {
                b.iter(|| black_box(engine.decode(black_box(encoded)).expect("valid base64")));
            });
        }
        decode.finish();
    }
}

criterion_group!(benches, codecs);
criterion_main!(benches);
