/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(missing_docs)]

use std::hint::black_box;
use std::path::PathBuf;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use image::{Bitmap, Compression, EncodeOptions};

struct Fixture(PathBuf);

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn textures(c: &mut Criterion) {
    let mut group = c.benchmark_group("texture_conversion");
    for size in [64, 256] {
        let pixels = (0..size * size)
            .flat_map(|index| {
                let value = (index * 7919) as u8;
                [value, 255 - value, value / 2, (index * 37) as u8]
            })
            .collect();
        let bitmap = Bitmap::new(size, size, pixels).expect("valid bitmap");
        let bytes = image::encode_with_options(
            &bitmap,
            image::Format::Bmp,
            EncodeOptions {
                compression: Compression::None,
                ..EncodeOptions::default()
            },
        )
        .expect("BMP fixture");
        let path = std::env::temp_dir().join(format!(
            "bron-gx2-texture-bench-{}-{size}.bmp",
            std::process::id()
        ));
        let fixture = Fixture(path);
        std::fs::write(&fixture.0, bytes).expect("write fixture");
        group.throughput(Throughput::Elements(u64::from(size) * u64::from(size)));
        for format in [gtxconv::Format::Rgba8, gtxconv::Format::Bc1] {
            group.bench_function(&format!("gx2_{format:?}_mipmaps/{size}"), |b| {
                b.iter(|| {
                    black_box(
                        gtxconv::Texture::load(black_box(&fixture.0), format, true)
                            .expect("GX2 texture"),
                    )
                });
            });
        }
    }
    group.finish();
}

criterion_group!(benches, textures);
criterion_main!(benches);
