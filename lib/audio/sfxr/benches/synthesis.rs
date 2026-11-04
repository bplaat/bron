/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(missing_docs)]

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use sfxr::{Generator, Sample, WaveType};

fn synthesis(c: &mut Criterion) {
    let mut group = c.benchmark_group("sfxr_synthesis");
    group.throughput(Throughput::Elements(4096));
    for (name, wave_type) in [
        ("square", WaveType::Square),
        ("triangle", WaveType::Triangle),
        ("sine", WaveType::Sine),
        ("noise", WaveType::Noise),
    ] {
        let mut sample = Sample::new();
        sample.wave_type = wave_type;
        sample.env_sustain = 0.5;
        sample.env_decay = 0.5;
        sample.vib_speed = 0.6;
        sample.vib_strength = 0.7;
        let mut generator = Generator::new(sample);
        let mut output = [0.0; 4096];
        group.bench_function(name, |b| {
            b.iter(|| {
                generator.reset();
                generator.generate(black_box(&mut output));
                black_box(&output);
            });
        });
    }
    group.finish();
}

criterion_group!(benches, synthesis);
criterion_main!(benches);
