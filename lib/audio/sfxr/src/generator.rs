/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The synthesizer stages of a generator: oscillator, envelope, filters and phaser.

use core::f32::consts::PI;

#[cfg(not(feature = "std"))]
use crate::math::Float;
use crate::{Rng, WaveType};

// MARK: Oscillator
pub(crate) struct Oscillator {
    wave_type: WaveType,
    rng: Rng,
    period: u32,
    phase: u32,
    noise_buffer: [f32; 32],
    square_duty: f32,
    square_slide: f32,
    fperiod: f64,
    fmaxperiod: f64,
    fslide: f64,
    fdslide: f64,
    vib_phase: f64,
    vib_speed: f64,
    vib_amp: f64,
    arp_time: i32,
    arp_limit: i32,
    arp_mod: f64,
}

/// The parameters of an oscillator restart
pub(crate) struct OscillatorParams {
    pub(crate) wave_type: WaveType,
    pub(crate) base_freq: f64,
    pub(crate) freq_limit: f64,
    pub(crate) freq_ramp: f64,
    pub(crate) freq_dramp: f64,
    pub(crate) duty: f32,
    pub(crate) duty_ramp: f32,
    pub(crate) arp_speed: f32,
    pub(crate) arp_mod: f64,
}

impl Oscillator {
    /// The noise is seeded with zero, so a sample always sounds the same
    pub(crate) const fn new(wave_type: WaveType) -> Self {
        Self {
            wave_type,
            rng: Rng::new(0),
            period: 8,
            phase: 0,
            noise_buffer: [0.0; 32],
            square_duty: 0.5,
            square_slide: 0.0,
            fperiod: 0.0,
            fmaxperiod: 0.0,
            fslide: 0.0,
            fdslide: 0.0,
            vib_phase: 0.0,
            vib_speed: 0.0,
            vib_amp: 0.0,
            arp_time: 0,
            arp_limit: 0,
            arp_mod: 0.0,
        }
    }

    pub(crate) fn reset_noise(&mut self) {
        for value in &mut self.noise_buffer {
            *value = self.rng.next_f32() * 2.0 - 1.0;
        }
    }

    pub(crate) const fn reset_phase(&mut self) {
        self.phase = 0;
    }

    pub(crate) fn reset_vibrato(&mut self, vib_speed: f64, vib_strength: f64) {
        self.vib_phase = 0.0;
        self.vib_speed = vib_speed.powi(2) * 0.01;
        self.vib_amp = vib_strength * 0.5;
    }

    pub(crate) fn reset(&mut self, params: &OscillatorParams) {
        self.wave_type = params.wave_type;
        self.fperiod = 100.0 / (params.base_freq.powi(2) + 0.001);
        self.fmaxperiod = 100.0 / (params.freq_limit.powi(2) + 0.001);
        self.fslide = 1.0 - params.freq_ramp.powi(3) * 0.01;
        self.fdslide = -params.freq_dramp.powi(3) * 0.000001;
        self.square_duty = 0.5 - params.duty * 0.5;
        self.square_slide = -params.duty_ramp * 0.00005;
        self.arp_mod = if params.arp_mod >= 0.0 {
            1.0 - params.arp_mod.powi(2) * 0.9
        } else {
            1.0 - params.arp_mod.powi(2) * 10.0
        };
        self.arp_time = 0;
        self.arp_limit = if params.arp_speed == 1.0 {
            0
        } else {
            ((1.0 - params.arp_speed).powi(2) * 20000.0 + 32.0) as i32
        };
    }

    pub(crate) fn advance(&mut self) {
        self.arp_time += 1;
        if self.arp_limit != 0 && self.arp_time >= self.arp_limit {
            self.arp_limit = 0;
            self.fperiod *= self.arp_mod;
        }

        self.fslide += self.fdslide;
        self.fperiod = (self.fperiod * self.fslide).min(self.fmaxperiod);

        self.vib_phase += self.vib_speed;
        let vibrato = 1.0 + self.vib_phase.sin() * self.vib_amp;

        self.period = ((vibrato * self.fperiod) as u32).max(8);
        self.square_duty = (self.square_duty + self.square_slide).clamp(0.0, 0.5);
    }

    pub(crate) fn next_sample(&mut self) -> f32 {
        self.phase += 1;
        if self.phase >= self.period {
            self.phase %= self.period;
            if self.wave_type == WaveType::Noise {
                self.reset_noise();
            }
        }

        let fp = self.phase as f32 / self.period as f32;
        match self.wave_type {
            WaveType::Square => {
                if fp < self.square_duty {
                    0.5
                } else {
                    -0.5
                }
            }
            WaveType::Triangle => 1.0 - fp * 2.0,
            WaveType::Sine => (fp * 2.0 * PI).sin(),
            WaveType::Noise => self.noise_buffer[(fp * 32.0) as usize],
        }
    }
}

// MARK: Envelope
#[derive(Clone, Copy)]
enum Stage {
    Attack,
    Sustain,
    Decay,
    End,
}

pub(crate) struct Envelope {
    stage: Stage,
    stage_left: u32,
    attack: u32,
    sustain: u32,
    decay: u32,
    punch: f32,
}

/// The length of an envelope stage in samples
pub(crate) fn stage_length(value: f32) -> u32 {
    (value.powi(2) * 100_000.0) as u32
}

impl Envelope {
    pub(crate) const fn new() -> Self {
        Self {
            stage: Stage::Attack,
            stage_left: 0,
            attack: 0,
            sustain: 0,
            decay: 0,
            punch: 0.0,
        }
    }

    pub(crate) fn reset(&mut self, attack: f32, sustain: f32, decay: f32, punch: f32) {
        self.attack = stage_length(attack);
        self.sustain = stage_length(sustain);
        self.decay = stage_length(decay);
        self.punch = punch;
        self.stage = Stage::Attack;
        self.stage_left = self.stage_length();
    }

    pub(crate) const fn advance(&mut self) {
        if self.stage_left > 1 {
            self.stage_left -= 1;
        } else {
            self.stage = match self.stage {
                Stage::Attack => Stage::Sustain,
                Stage::Sustain => Stage::Decay,
                Stage::Decay | Stage::End => Stage::End,
            };
            self.stage_left = self.stage_length();
        }
    }

    const fn stage_length(&self) -> u32 {
        match self.stage {
            Stage::Attack => self.attack,
            Stage::Sustain => self.sustain,
            Stage::Decay => self.decay,
            Stage::End => 0,
        }
    }

    pub(crate) fn filter(&self, sample: f32) -> f32 {
        // An empty stage is passed in one sample, its progress is zero instead of NaN
        let length = self.stage_length();
        let dt = if length == 0 {
            0.0
        } else {
            self.stage_left as f32 / length as f32
        };
        sample
            * match self.stage {
                Stage::Attack => 1.0 - dt,
                Stage::Sustain => 1.0 + dt * 2.0 * self.punch,
                Stage::Decay => dt,
                Stage::End => 0.0,
            }
    }

    pub(crate) const fn finished(&self) -> bool {
        matches!(self.stage, Stage::End)
    }
}

// MARK: Filters
pub(crate) struct HighLowPassFilter {
    fltp: f32,
    fltdp: f32,
    fltw: f32,
    fltw_d: f32,
    fltdmp: f32,
    fltphp: f32,
    flthp: f32,
    flthp_d: f32,
}

impl HighLowPassFilter {
    pub(crate) const fn new() -> Self {
        Self {
            fltp: 0.0,
            fltdp: 0.0,
            fltw: 0.0,
            fltw_d: 0.0,
            fltdmp: 0.0,
            fltphp: 0.0,
            flthp: 0.0,
            flthp_d: 0.0,
        }
    }

    pub(crate) fn reset(
        &mut self,
        lpf_resonance: f32,
        lpf_freq: f32,
        lpf_ramp: f32,
        hpf_freq: f32,
        hpf_ramp: f32,
    ) {
        self.fltp = 0.0;
        self.fltdp = 0.0;
        self.fltw = lpf_freq.powi(3) * 0.1;
        self.fltw_d = 1.0 + lpf_ramp * 0.0001;
        self.fltdmp = (5.0 / (1.0 + lpf_resonance.powi(2) * 20.0) * (0.01 + self.fltw)).min(0.8);
        self.fltphp = 0.0;
        self.flthp = hpf_freq.powi(2) * 0.1;
        self.flthp_d = 1.0 + hpf_ramp * 0.0003;
    }

    pub(crate) fn filter(&mut self, sample: f32) -> f32 {
        let previous = self.fltp;

        // Low pass filter
        if self.fltw > 0.0 {
            self.fltw = (self.fltw * self.fltw_d).clamp(0.0, 0.1);
            self.fltdp += (sample - self.fltp) * self.fltw;
            self.fltdp -= self.fltdp * self.fltdmp;
        } else {
            self.fltp = sample;
            self.fltdp = 0.0;
        }
        self.fltp += self.fltdp;

        // High pass filter
        self.flthp = (self.flthp * self.flthp_d).clamp(0.00001, 0.1);
        self.fltphp += self.fltp - previous;
        self.fltphp -= self.fltphp * self.flthp;
        self.fltphp
    }
}

pub(crate) struct Phaser {
    ipp: usize,
    fphase: f32,
    fdphase: f32,
    buffer: [f32; 1024],
}

impl Phaser {
    pub(crate) const fn new() -> Self {
        Self {
            ipp: 0,
            fphase: 0.0,
            fdphase: 0.0,
            buffer: [0.0; 1024],
        }
    }

    pub(crate) fn reset(&mut self, pha_offset: f32, pha_ramp: f32) {
        self.fphase = pha_offset.powi(2) * 1020.0 * pha_offset.signum();
        self.fdphase = pha_ramp.powi(2) * pha_ramp.signum();
    }

    pub(crate) fn advance(&mut self) {
        self.fphase += self.fdphase;
    }

    pub(crate) fn filter(&mut self, sample: f32) -> f32 {
        let length = self.buffer.len();
        self.buffer[self.ipp] = sample;
        let iphase = (self.fphase.abs() as usize).min(length - 1);
        let result = sample + self.buffer[(self.ipp + length - iphase) % length];
        self.ipp = (self.ipp + 1) % length;
        result
    }
}
