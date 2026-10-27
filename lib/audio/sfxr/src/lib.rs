/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]

use crate::generator::{Envelope, HighLowPassFilter, Oscillator, OscillatorParams, Phaser};

mod generator;

/// The sample rate of the generated sound in Hz.
pub const SAMPLE_RATE: u32 = 44_100;

// MARK: Random
/// A small SplitMix64 generator, seeds without a value come from the operating system
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) const fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn from_seed(seed: Option<u64>) -> Self {
        Self::new(seed.unwrap_or_else(|| {
            let mut bytes = [0; 8];
            // Without operating system randomness the sound is still valid, only predictable
            _ = getrandom::fill(&mut bytes);
            u64::from_ne_bytes(bytes)
        }))
    }

    const fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }

    /// A value in `0.0..1.0`
    pub(crate) fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    fn range_f32(&mut self, from: f32, until: f32) -> f32 {
        from + (until - from) * self.next_f32()
    }

    fn range_f64(&mut self, from: f64, until: f64) -> f64 {
        from + (until - from) * ((self.next_u64() >> 11) as f64 / (1u64 << 53) as f64)
    }

    /// True with `chance_true` to `chance_false` odds
    const fn chance(&mut self, chance_true: u64, chance_false: u64) -> bool {
        self.next_u64() % (chance_true + chance_false) < chance_true
    }

    const fn element<T: Copy>(&mut self, slice: &[T]) -> T {
        slice[(self.next_u64() % slice.len() as u64) as usize]
    }
}

// MARK: Sample
/// The wave shape of the oscillator.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum WaveType {
    /// A square wave with a duty cycle.
    Square,
    /// A triangle wave.
    Triangle,
    /// A sine wave.
    Sine,
    /// White noise.
    Noise,
}

/// Defines a sound effect configuration for a [`Generator`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    /// Oscillator wave type.
    pub wave_type: WaveType,
    /// Oscillator base frequency, between `0.0` and `1.0`.
    pub base_freq: f64,
    /// Oscillator frequency limit, between `0.0` and `1.0`.
    pub freq_limit: f64,
    /// Oscillator frequency change over time, between `-1.0` and `1.0`.
    pub freq_ramp: f64,
    /// `freq_ramp` change over time, between `-1.0` and `1.0`.
    pub freq_dramp: f64,
    /// Oscillator square wave duty cycle, between `0.0` and `1.0`.
    pub duty: f32,
    /// Oscillator square wave duty cycle change over time, between `-1.0` and `1.0`.
    pub duty_ramp: f32,

    /// Vibrato strength, between `0.0` and `1.0`.
    pub vib_strength: f64,
    /// Vibrato speed, between `0.0` and `1.0`.
    pub vib_speed: f64,
    /// Vibrato delay, between `0.0` and `1.0`.
    pub vib_delay: f32,

    /// Duration of the attack envelope, between `0.0` and `1.0`.
    pub env_attack: f32,
    /// Duration of the sustain envelope, between `0.0` and `1.0`.
    pub env_sustain: f32,
    /// Duration of the decay envelope, between `0.0` and `1.0`.
    pub env_decay: f32,
    /// Amount of "punch" in the sustain envelope, between `-1.0` and `1.0`.
    pub env_punch: f32,

    /// Low pass filter resonance, between `0.0` and `1.0`.
    pub lpf_resonance: f32,
    /// Low pass filter cutoff frequency, between `0.0` and `1.0`.
    pub lpf_freq: f32,
    /// Low pass filter cutoff frequency change over time, between `-1.0` and `1.0`.
    pub lpf_ramp: f32,
    /// High pass filter cutoff frequency, between `0.0` and `1.0`.
    pub hpf_freq: f32,
    /// High pass filter cutoff frequency change over time, between `-1.0` and `1.0`.
    pub hpf_ramp: f32,

    /// Phaser temporal offset, between `-1.0` and `1.0`.
    pub pha_offset: f32,
    /// Phaser temporal offset change over time, between `-1.0` and `1.0`.
    pub pha_ramp: f32,

    /// Sample repeat speed, between `0.0` and `1.0`.
    pub repeat_speed: f32,

    /// Arpeggio interval, between `0.0` and `1.0`.
    pub arp_speed: f32,
    /// Arpeggio step in frequency, between `-1.0` and `1.0`.
    pub arp_mod: f64,
}

impl Default for Sample {
    fn default() -> Self {
        Self::new()
    }
}

impl Sample {
    /// Constructs a new sample with default settings.
    pub const fn new() -> Self {
        Self {
            wave_type: WaveType::Square,
            base_freq: 0.3,
            freq_limit: 0.0,
            freq_ramp: 0.0,
            freq_dramp: 0.0,
            duty: 0.0,
            duty_ramp: 0.0,
            vib_strength: 0.0,
            vib_speed: 0.0,
            vib_delay: 0.0,
            env_attack: 0.4,
            env_sustain: 0.1,
            env_decay: 0.5,
            env_punch: 0.0,
            lpf_resonance: 0.0,
            lpf_freq: 1.0,
            lpf_ramp: 0.0,
            hpf_freq: 0.0,
            hpf_ramp: 0.0,
            pha_offset: 0.0,
            pha_ramp: 0.0,
            repeat_speed: 0.0,
            arp_speed: 0.0,
            arp_mod: 0.0,
        }
    }

    /// Asserts that all fields are within their ranges
    fn assert_valid(&self) {
        let unit = 0.0..=1.0;
        let signed = -1.0..=1.0;
        for (name, value, range) in [
            ("base_freq", self.base_freq, &unit),
            ("freq_limit", self.freq_limit, &unit),
            ("freq_ramp", self.freq_ramp, &signed),
            ("freq_dramp", self.freq_dramp, &signed),
            ("duty", f64::from(self.duty), &unit),
            ("duty_ramp", f64::from(self.duty_ramp), &signed),
            ("vib_strength", self.vib_strength, &unit),
            ("vib_speed", self.vib_speed, &unit),
            ("vib_delay", f64::from(self.vib_delay), &unit),
            ("env_attack", f64::from(self.env_attack), &unit),
            ("env_sustain", f64::from(self.env_sustain), &unit),
            ("env_decay", f64::from(self.env_decay), &unit),
            ("env_punch", f64::from(self.env_punch), &signed),
            ("lpf_resonance", f64::from(self.lpf_resonance), &unit),
            ("lpf_freq", f64::from(self.lpf_freq), &unit),
            ("lpf_ramp", f64::from(self.lpf_ramp), &signed),
            ("hpf_freq", f64::from(self.hpf_freq), &unit),
            ("hpf_ramp", f64::from(self.hpf_ramp), &signed),
            ("pha_offset", f64::from(self.pha_offset), &signed),
            ("pha_ramp", f64::from(self.pha_ramp), &signed),
            ("repeat_speed", f64::from(self.repeat_speed), &unit),
            ("arp_speed", f64::from(self.arp_speed), &unit),
            ("arp_mod", self.arp_mod, &signed),
        ] {
            assert!(
                range.contains(&value),
                "{name} must be between {} and {}",
                range.start(),
                range.end()
            );
        }
    }

    /// Changes the fields randomly by a little, `None` uses a random seed.
    pub fn mutate(&mut self, seed: Option<u64>) {
        let rng = &mut Rng::from_seed(seed);
        let mut mutate = |value: &mut f64, min: f64, max: f64| {
            if rng.chance(1, 1) {
                *value = (*value + rng.range_f64(-0.05, 0.05)).clamp(min, max);
            }
        };
        let mut mutate_f32 = |value: &mut f32, min: f32, max: f32| {
            let mut wide = f64::from(*value);
            mutate(&mut wide, f64::from(min), f64::from(max));
            *value = wide as f32;
        };
        // The frequency limit isn't mutated, like in the original sfxr
        mutate_f32(&mut self.duty, 0.0, 1.0);
        mutate_f32(&mut self.duty_ramp, -1.0, 1.0);
        mutate_f32(&mut self.vib_delay, 0.0, 1.0);
        mutate_f32(&mut self.env_attack, 0.0, 1.0);
        mutate_f32(&mut self.env_sustain, 0.0, 1.0);
        mutate_f32(&mut self.env_decay, 0.0, 1.0);
        mutate_f32(&mut self.env_punch, -1.0, 1.0);
        mutate_f32(&mut self.lpf_resonance, 0.0, 1.0);
        mutate_f32(&mut self.lpf_freq, 0.0, 1.0);
        mutate_f32(&mut self.lpf_ramp, -1.0, 1.0);
        mutate_f32(&mut self.hpf_freq, 0.0, 1.0);
        mutate_f32(&mut self.hpf_ramp, -1.0, 1.0);
        mutate_f32(&mut self.pha_offset, -1.0, 1.0);
        mutate_f32(&mut self.pha_ramp, -1.0, 1.0);
        mutate_f32(&mut self.repeat_speed, 0.0, 1.0);
        mutate_f32(&mut self.arp_speed, 0.0, 1.0);
        mutate(&mut self.base_freq, 0.0, 1.0);
        mutate(&mut self.freq_ramp, -1.0, 1.0);
        mutate(&mut self.freq_dramp, -1.0, 1.0);
        mutate(&mut self.vib_strength, 0.0, 1.0);
        mutate(&mut self.vib_speed, 0.0, 1.0);
        mutate(&mut self.arp_mod, -1.0, 1.0);
    }

    /// Constructs a random "coin" or "item pickup" style sample, `None` uses a random seed.
    pub fn pickup(seed: Option<u64>) -> Self {
        let rng = &mut Rng::from_seed(seed);
        let mut s = Self::new();
        s.base_freq = rng.range_f64(0.4, 0.9);
        s.env_attack = 0.0;
        s.env_sustain = rng.range_f32(0.0, 0.1);
        s.env_decay = rng.range_f32(0.1, 0.5);
        s.env_punch = rng.range_f32(0.3, 0.6);
        if rng.chance(1, 1) {
            s.arp_speed = rng.range_f32(0.5, 0.7);
            s.arp_mod = rng.range_f64(0.2, 0.6);
        }
        s
    }

    /// Constructs a random "shoot" or "laser" style sample, `None` uses a random seed.
    pub fn laser(seed: Option<u64>) -> Self {
        use WaveType::{Sine, Square, Triangle};

        let rng = &mut Rng::from_seed(seed);
        let mut s = Self::new();
        s.wave_type = rng.element(&[Square, Square, Sine, Sine, Triangle]);
        if rng.chance(1, 2) {
            s.base_freq = rng.range_f64(0.3, 0.9);
            s.freq_limit = rng.range_f64(0.0, 0.1);
            s.freq_ramp = rng.range_f64(-0.35, -0.65);
        } else {
            s.base_freq = rng.range_f64(0.5, 1.0);
            s.freq_limit = (s.base_freq - rng.range_f64(0.2, 0.8)).max(0.2);
            s.freq_ramp = rng.range_f64(-0.15, -0.35);
        }
        if rng.chance(1, 1) {
            s.duty = rng.range_f32(0.0, 0.5);
            s.duty_ramp = rng.range_f32(0.0, 0.2);
        } else {
            s.duty = rng.range_f32(0.4, 0.9);
            s.duty_ramp = rng.range_f32(0.0, -0.7);
        }
        s.env_attack = 0.0;
        s.env_sustain = rng.range_f32(0.1, 0.3);
        s.env_decay = rng.range_f32(0.0, 0.4);
        if rng.chance(1, 1) {
            s.env_punch = rng.range_f32(0.0, 0.3);
        }
        if rng.chance(1, 2) {
            s.pha_offset = rng.range_f32(0.0, 0.2);
            s.pha_ramp = -rng.range_f32(0.0, 0.2);
        }
        if rng.chance(1, 1) {
            s.hpf_freq = rng.range_f32(0.0, 0.3);
        }
        s
    }

    /// Constructs a random "explosion" style sample, `None` uses a random seed.
    pub fn explosion(seed: Option<u64>) -> Self {
        let rng = &mut Rng::from_seed(seed);
        let mut s = Self::new();
        s.wave_type = WaveType::Noise;
        if rng.chance(1, 1) {
            s.base_freq = rng.range_f64(0.1, 0.5);
            s.freq_ramp = rng.range_f64(-0.1, 0.3);
        } else {
            s.base_freq = rng.range_f64(0.2, 0.9);
            s.freq_ramp = rng.range_f64(-0.2, -0.4);
        }
        s.base_freq = s.base_freq.powi(2);
        if rng.chance(1, 4) {
            s.freq_ramp = 0.0;
        }
        if rng.chance(1, 2) {
            s.repeat_speed = rng.range_f32(0.3, 0.8);
        }
        s.env_attack = 0.0;
        s.env_sustain = rng.range_f32(0.1, 0.4);
        s.env_decay = rng.range_f32(0.0, 0.5);
        if rng.chance(1, 1) {
            s.pha_offset = rng.range_f32(-0.3, 0.6);
            s.pha_ramp = rng.range_f32(-0.3, 0.0);
        }
        s.env_punch = rng.range_f32(0.2, 0.8);
        if rng.chance(1, 1) {
            s.vib_strength = rng.range_f64(0.0, 0.7);
            s.vib_speed = rng.range_f64(0.0, 0.6);
        }
        if rng.chance(1, 2) {
            s.arp_speed = rng.range_f32(0.6, 0.9);
            s.arp_mod = rng.range_f64(-0.8, 0.8);
        }
        s
    }

    /// Constructs a random "powerup" style sample, `None` uses a random seed.
    pub fn powerup(seed: Option<u64>) -> Self {
        let rng = &mut Rng::from_seed(seed);
        let mut s = Self::new();
        if rng.chance(1, 1) {
            s.wave_type = WaveType::Sine;
        } else {
            s.duty = rng.range_f32(0.0, 0.6);
        }
        s.base_freq = rng.range_f64(0.2, 0.5);
        if rng.chance(1, 1) {
            s.freq_ramp = rng.range_f64(0.1, 0.5);
            s.repeat_speed = rng.range_f32(0.4, 0.8);
        } else {
            s.freq_ramp = rng.range_f64(0.05, 0.25);
            if rng.chance(1, 1) {
                s.vib_strength = rng.range_f64(0.0, 0.7);
                s.vib_speed = rng.range_f64(0.0, 0.6);
            }
        }
        s.env_attack = 0.0;
        s.env_sustain = rng.range_f32(0.0, 0.4);
        s.env_decay = rng.range_f32(0.1, 0.5);
        s
    }

    /// Constructs a random "hit" or "damage" style sample, `None` uses a random seed.
    pub fn hit(seed: Option<u64>) -> Self {
        let rng = &mut Rng::from_seed(seed);
        let mut s = Self::new();
        s.wave_type = rng.element(&[WaveType::Square, WaveType::Sine, WaveType::Noise]);
        if s.wave_type == WaveType::Square {
            s.duty = rng.range_f32(0.0, 0.6);
        }
        s.base_freq = rng.range_f64(0.2, 0.8);
        s.freq_ramp = rng.range_f64(-0.3, -0.7);
        s.env_attack = 0.0;
        s.env_sustain = rng.range_f32(0.0, 0.1);
        s.env_decay = rng.range_f32(0.1, 0.3);
        if rng.chance(1, 1) {
            s.hpf_freq = rng.range_f32(0.0, 0.3);
        }
        s
    }

    /// Constructs a random "jump" style sample, `None` uses a random seed.
    pub fn jump(seed: Option<u64>) -> Self {
        let rng = &mut Rng::from_seed(seed);
        let mut s = Self::new();
        s.wave_type = WaveType::Square;
        s.duty = rng.range_f32(0.0, 0.6);
        s.base_freq = rng.range_f64(0.3, 0.6);
        s.freq_ramp = rng.range_f64(0.1, 0.3);
        s.env_attack = 0.0;
        s.env_sustain = rng.range_f32(0.1, 0.4);
        s.env_decay = rng.range_f32(0.1, 0.3);
        if rng.chance(1, 1) {
            s.hpf_freq = rng.range_f32(0.0, 0.3);
        }
        if rng.chance(1, 1) {
            s.lpf_freq = rng.range_f32(0.4, 1.0);
        }
        s
    }

    /// Constructs a random "blip" or "menu navigation" style sample, `None` uses a random seed.
    pub fn blip(seed: Option<u64>) -> Self {
        let rng = &mut Rng::from_seed(seed);
        let mut s = Self::new();
        s.wave_type = rng.element(&[WaveType::Square, WaveType::Sine]);
        if s.wave_type == WaveType::Square {
            s.duty = rng.range_f32(0.0, 0.6);
        }
        s.base_freq = rng.range_f64(0.2, 0.6);
        s.env_attack = 0.0;
        s.env_sustain = rng.range_f32(0.1, 0.2);
        s.env_decay = rng.range_f32(0.0, 0.2);
        s.hpf_freq = 0.1;
        s
    }
}

// MARK: Generator
/// Sound effect generator.
///
/// Generates mono sound at [`SAMPLE_RATE`] according to a [`Sample`] into a buffer. The data can
/// be generated in multiple chunks, as the generator keeps its state between calls.
pub struct Generator {
    /// Generator settings.
    pub sample: Sample,
    /// Sound effect volume, the default is `0.2`.
    pub volume: f32,
    oscillator: Oscillator,
    filter: HighLowPassFilter,
    envelope: Envelope,
    phaser: Phaser,
    rep_time: i32,
    rep_limit: i32,
}

impl Generator {
    /// Constructs a new generator for the sample.
    ///
    /// # Panics
    ///
    /// Panics when a field of the sample is out of its range.
    pub fn new(sample: Sample) -> Self {
        sample.assert_valid();
        let mut generator = Self {
            sample,
            volume: 0.2,
            oscillator: Oscillator::new(sample.wave_type),
            filter: HighLowPassFilter::new(),
            envelope: Envelope::new(),
            phaser: Phaser::new(),
            rep_time: 0,
            rep_limit: 0,
        };
        generator.reset();
        generator
    }

    /// Fills `buffer` with sound, subsequent calls continue where the last one left off. Call
    /// [`Generator::reset`] first to start from the beginning.
    pub fn generate(&mut self, buffer: &mut [f32]) {
        for value in buffer {
            self.rep_time += 1;
            if self.rep_limit != 0 && self.rep_time >= self.rep_limit {
                self.rep_time = 0;
                self.restart();
            }

            self.oscillator.advance();
            self.envelope.advance();
            self.phaser.advance();

            // Eight times supersampling
            let mut sample = 0.0;
            for _ in 0..8 {
                let oscillator = self.oscillator.next_sample();
                let shaped = self.filter.filter(self.envelope.filter(oscillator));
                sample += self.phaser.filter(shaped);
            }
            *value = (sample / 8.0 * self.volume).clamp(-1.0, 1.0);
        }
    }

    /// Returns true when the envelope has ended, after that only silence is generated.
    pub const fn finished(&self) -> bool {
        self.envelope.finished()
    }

    /// Resets the generator to the beginning of the sound effect.
    pub fn reset(&mut self) {
        let s = self.sample;
        self.restart();
        self.envelope
            .reset(s.env_attack, s.env_sustain, s.env_decay, s.env_punch);
        self.phaser.reset(s.pha_offset, s.pha_ramp);
        self.oscillator.reset_phase();
        self.oscillator.reset_vibrato(s.vib_speed, s.vib_strength);
        self.oscillator.reset_noise();
        self.rep_time = 0;
        self.rep_limit = if s.repeat_speed == 0.0 {
            0
        } else {
            ((1.0 - s.repeat_speed).powi(2) * 20_000.0 * 32.0) as i32
        };
    }

    /// Restarts the oscillator and filters, for repeats
    fn restart(&mut self) {
        let s = self.sample;
        self.filter.reset(
            s.lpf_resonance,
            s.lpf_freq,
            s.lpf_ramp,
            s.hpf_freq,
            s.hpf_ramp,
        );
        self.oscillator.reset(&OscillatorParams {
            wave_type: s.wave_type,
            base_freq: s.base_freq,
            freq_limit: s.freq_limit,
            freq_ramp: s.freq_ramp,
            freq_dramp: s.freq_dramp,
            duty: s.duty,
            duty_ramp: s.duty_ramp,
            arp_speed: s.arp_speed,
            arp_mod: s.arp_mod,
        });
    }
}

// MARK: Tests
#[cfg(test)]
mod test {
    use super::*;

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

    #[test]
    fn generator_finishes_after_the_envelope() {
        let mut generator = Generator::new(Sample::blip(Some(1)));
        let mut buffer = vec![0.0; SAMPLE_RATE as usize];
        generator.generate(&mut buffer);
        assert!(generator.finished());
        generator.generate(&mut buffer);
        assert!(buffer.iter().all(|value| value.abs() < 0.001));
        generator.reset();
        assert!(!generator.finished());
    }

    #[test]
    #[should_panic(expected = "base_freq must be between 0 and 1")]
    fn invalid_samples_panic() {
        let mut sample = Sample::new();
        sample.base_freq = 2.0;
        _ = Generator::new(sample);
    }
}
