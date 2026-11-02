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
