/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(missing_docs)]

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use deflate::Level;

/// Returns the filtered scanlines of a PNG, a typical image compression input.
fn png_scanlines(png: &[u8]) -> Vec<u8> {
    let mut zlib = Vec::new();
    let mut pos = 8;
    while pos + 8 <= png.len() {
        let len = u32::from_be_bytes(png[pos..pos + 4].try_into().expect("chunk length")) as usize;
        if &png[pos + 4..pos + 8] == b"IDAT" {
            zlib.extend_from_slice(&png[pos + 8..pos + 8 + len]);
        }
        pos += 12 + len;
    }
    deflate::zlib_decompress(&zlib, usize::MAX).expect("valid PNG data")
}

fn inputs() -> [(&'static str, Vec<u8>); 2] {
    [
        (
            "image",
            png_scanlines(include_bytes!(
                "../../../graphics/image/benches/images/dice.png"
            )),
        ),
        ("text", include_bytes!("../src/encode.rs").repeat(4)),
    ]
}

fn compress(c: &mut Criterion) {
    let mut group = c.benchmark_group("deflate_compress");
    for (name, data) in inputs() {
        group.throughput(Throughput::Bytes(data.len() as u64));
        for level in [Level::Fast, Level::Default, Level::Best, Level::Crunch] {
            let size = deflate::compress(&data, level).len();
            println!("{name} {level:?}: {} -> {size} bytes", data.len());
            group.bench_with_input(
                BenchmarkId::new(format!("{level:?}").to_lowercase(), name),
                &data,
                |b, data| b.iter(|| black_box(deflate::compress(black_box(data), level))),
            );
        }
    }
    group.finish();
}

fn decompress(c: &mut Criterion) {
    let mut group = c.benchmark_group("deflate_decompress");
    for (name, data) in inputs() {
        let compressed = deflate::zlib_compress(&data, Level::Best);
        group.throughput(Throughput::Bytes(data.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("zlib", name),
            &compressed,
            |b, compressed| {
                b.iter(|| black_box(deflate::zlib_decompress(black_box(compressed), usize::MAX)));
            },
        );
    }
    group.finish();
}

criterion_group!(benches, compress, decompress);
criterion_main!(benches);
