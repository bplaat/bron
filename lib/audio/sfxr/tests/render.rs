/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Exercises synthesis with both std and libc math.

use sfxr::{Generator, SAMPLE_RATE, Sample, WaveType};

#[test]
fn waveforms_with_vibrato_are_finite_and_finish() {
    for wave_type in [
        WaveType::Square,
        WaveType::Triangle,
        WaveType::Sine,
        WaveType::Noise,
    ] {
        let mut sample = Sample::new();
        sample.wave_type = wave_type;
        sample.base_freq = 0.4;
        sample.vib_speed = 0.6;
        sample.vib_strength = 0.7;
        sample.env_sustain = 0.1;
        sample.env_decay = 0.2;
        let mut generator = Generator::new(sample);
        let mut buffer = vec![0.0; SAMPLE_RATE as usize];
        generator.generate(&mut buffer);
        assert!(buffer.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
        assert!(buffer.iter().any(|v| *v != 0.0));
        assert!(generator.finished());
    }
}

fn render(sample: Sample) -> Vec<f32> {
    let mut generator = Generator::new(sample);
    let mut buffer = vec![0.0; SAMPLE_RATE as usize];
    generator.generate(&mut buffer);
    buffer
}

#[test]
fn seeded_presets_are_deterministic_and_valid() {
    for preset in [
        Sample::pickup,
        Sample::laser,
        Sample::explosion,
        Sample::powerup,
        Sample::hit,
        Sample::jump,
        Sample::blip,
    ] {
        for seed in 0..8 {
            assert_eq!(preset(Some(seed)), preset(Some(seed)));
            let mut mutated = preset(Some(seed));
            mutated.mutate(Some(seed));
            for sample in [preset(Some(seed)), mutated] {
                let buffer = render(sample);
                assert!(
                    buffer
                        .iter()
                        .all(|value| value.is_finite() && value.abs() <= 1.0)
                );
                assert!(buffer.iter().any(|value| *value != 0.0));
            }
        }
    }
}
