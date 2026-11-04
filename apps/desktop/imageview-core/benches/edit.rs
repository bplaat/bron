/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(missing_docs)]

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use imageview_core::edit::{Filter, Picture};

fn edit(c: &mut Criterion) {
    let mut group = c.benchmark_group("image_edit");
    for (width, height) in [(127, 93), (1920, 1080)] {
        let pixels = (0..width * height * 4)
            .map(|index: u32| (index.wrapping_mul(7919) % 256) as u8)
            .collect();
        let picture = Picture::new(width, height, pixels);
        group.throughput(Throughput::Elements(u64::from(width) * u64::from(height)));
        let size = format!("{width}x{height}");
        for (name, output_width, output_height) in [
            ("resize_horizontal", width / 2, height),
            ("resize_vertical", width, height / 2),
            ("resize_both", width / 2, height / 2),
        ] {
            group.bench_function(&format!("{name}/{size}"), |b| {
                b.iter(|| black_box(black_box(&picture).resize(output_width, output_height)));
            });
        }
        for filter in [
            Filter::Adjust,
            Filter::AutoEnhance,
            Filter::Invert,
            Filter::Grayscale,
            Filter::Sepia,
            Filter::Solarize,
            Filter::BoxBlur,
            Filter::GaussianBlur,
            Filter::Sharpen,
            Filter::ReduceNoise,
            Filter::Pixelate,
        ] {
            let values = filter.defaults();
            group.bench_function(&format!("{}/{size}", filter.title()), |b| {
                b.iter(|| black_box(filter.apply(black_box(&picture), black_box(&values))));
            });
        }
    }
    group.finish();
}

criterion_group!(benches, edit);
criterion_main!(benches);
