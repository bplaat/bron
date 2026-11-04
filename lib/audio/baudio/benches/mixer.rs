/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(missing_docs)]

use std::hint::black_box;
use std::sync::mpsc;

use baudio::{Play, Sound};
use criterion::{Criterion, Throughput, criterion_group, criterion_main};

// Exercise the production callback without opening an audio device.
#[allow(dead_code, unused_imports)]
#[path = "../src/mixer.rs"]
mod mixer;

fn mixing(c: &mut Criterion) {
    let mut group = c.benchmark_group("audio_mixer");
    group.throughput(Throughput::Elements(512));
    let samples: Vec<f32> = (0..4096)
        .map(|index| (index as f32 * 0.01).sin() * 0.01)
        .collect();
    let sound = Sound::new(48_000, samples);
    for channels in [1, 2] {
        for voices in [1, 32] {
            for pitch in [1.0, 1.25] {
                let (sender, receiver) = mpsc::channel();
                let mut mixer = mixer::Mixer::new(receiver, 48_000, channels);
                for id in 0..voices {
                    let play = Play {
                        pitch,
                        looped: true,
                        ..Play::default()
                    };
                    sender
                        .send(mixer::Command::Play(mixer::VoiceState::new(
                            id,
                            sound.clone(),
                            play,
                        )))
                        .expect("play voice");
                }
                let mut output = vec![0.0; 512 * usize::from(channels)];
                mixer.render(&mut output);
                group.bench_function(&format!("{channels}ch/{voices}voices/pitch{pitch}"), |b| {
                    b.iter(|| {
                        mixer.render(black_box(&mut output));
                        black_box(&output);
                    });
                });
            }
        }
    }
    group.finish();
}

criterion_group!(benches, mixing);
criterion_main!(benches);
