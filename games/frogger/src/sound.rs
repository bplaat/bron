// Copyright (c) 2026 Bastiaan van der Plaat
// SPDX-License-Identifier: MIT

//! The sound effects, designed as sfxr samples and mixed by baudio.

use baudio::{Audio, Play, Sound};
use sfxr::{Sample, WaveType};

use crate::game::Cue;

const HOP: Sample = Sample {
    duty: 0.4,
    base_freq: 0.32,
    freq_ramp: 0.22,
    env_attack: 0.0,
    env_sustain: 0.06,
    env_decay: 0.16,
    lpf_freq: 0.75,
    hpf_freq: 0.08,
    ..Sample::new()
};

const LOG: Sample = Sample {
    wave_type: WaveType::Triangle,
    base_freq: 0.2,
    freq_ramp: -0.25,
    env_attack: 0.0,
    env_sustain: 0.02,
    env_decay: 0.14,
    env_punch: 0.4,
    ..Sample::new()
};

const BELL: Sample = Sample {
    wave_type: WaveType::Sine,
    base_freq: 0.62,
    env_attack: 0.0,
    env_sustain: 0.03,
    env_decay: 0.32,
    env_punch: 0.5,
    ..Sample::new()
};

const HORN: Sample = Sample {
    duty: 0.55,
    base_freq: 0.2,
    vib_strength: 0.05,
    vib_speed: 0.4,
    env_attack: 0.08,
    env_sustain: 0.5,
    env_decay: 0.35,
    lpf_resonance: 0.3,
    lpf_freq: 0.45,
    ..Sample::new()
};

const TICK: Sample = Sample {
    duty: 0.5,
    base_freq: 0.55,
    env_attack: 0.0,
    env_sustain: 0.02,
    env_decay: 0.06,
    hpf_freq: 0.2,
    ..Sample::new()
};

const BEST: Sample = Sample {
    duty: 0.25,
    base_freq: 0.42,
    freq_ramp: 0.15,
    env_attack: 0.0,
    env_sustain: 0.22,
    env_decay: 0.42,
    env_punch: 0.45,
    arp_speed: 0.62,
    arp_mod: 0.45,
    ..Sample::new()
};

const SQUASH: Sample = Sample {
    wave_type: WaveType::Noise,
    base_freq: 0.16,
    freq_ramp: -0.15,
    env_attack: 0.0,
    env_sustain: 0.08,
    env_decay: 0.34,
    env_punch: 0.6,
    lpf_freq: 0.7,
    ..Sample::new()
};

const CRASH: Sample = Sample {
    wave_type: WaveType::Noise,
    base_freq: 0.08,
    freq_ramp: -0.05,
    env_attack: 0.0,
    env_sustain: 0.3,
    env_decay: 0.55,
    env_punch: 0.7,
    pha_offset: 0.2,
    pha_ramp: -0.1,
    ..Sample::new()
};

const SPLASH: Sample = Sample {
    wave_type: WaveType::Noise,
    base_freq: 0.35,
    freq_ramp: -0.2,
    env_attack: 0.02,
    env_sustain: 0.12,
    env_decay: 0.5,
    lpf_freq: 0.55,
    lpf_ramp: -0.25,
    hpf_freq: 0.05,
    ..Sample::new()
};

const TIMEOUT: Sample = Sample {
    duty: 0.5,
    base_freq: 0.3,
    freq_ramp: -0.25,
    vib_strength: 0.3,
    vib_speed: 0.6,
    env_attack: 0.0,
    env_sustain: 0.25,
    env_decay: 0.3,
    ..Sample::new()
};

const GAME_OVER: Sample = Sample {
    wave_type: WaveType::Triangle,
    base_freq: 0.38,
    freq_ramp: -0.18,
    vib_strength: 0.1,
    vib_speed: 0.5,
    env_attack: 0.0,
    env_sustain: 0.3,
    env_decay: 0.5,
    arp_speed: 0.55,
    arp_mod: -0.3,
    ..Sample::new()
};

const START: Sample = Sample {
    duty: 0.3,
    base_freq: 0.45,
    env_attack: 0.0,
    env_sustain: 0.08,
    env_decay: 0.25,
    env_punch: 0.4,
    arp_speed: 0.6,
    arp_mod: 0.35,
    ..Sample::new()
};

const PAUSE: Sample = Sample {
    wave_type: WaveType::Sine,
    base_freq: 0.4,
    freq_ramp: -0.2,
    env_attack: 0.0,
    env_sustain: 0.04,
    env_decay: 0.12,
    ..Sample::new()
};

/// Small pitch steps, so repeated hops don't sound the same
const HOP_PITCHES: [f32; 4] = [1.0, 1.06, 0.97, 1.03];

/// A major third above, the horn plays as a chord
const THIRD: f32 = 1.26;

pub(super) struct Sounds {
    audio: Option<Audio>,
    hop: Sound,
    log: Sound,
    bell: Sound,
    horn: Sound,
    tick: Sound,
    best: Sound,
    squash: Sound,
    crash: Sound,
    splash: Sound,
    timeout: Sound,
    game_over: Sound,
    start: Sound,
    pause: Sound,
    hops: usize,
}

impl Sounds {
    /// Renders all sounds, the game stays silent without an audio device
    pub(super) fn new() -> Self {
        let audio = Audio::new()
            .inspect_err(|error| eprintln!("Audio unavailable: {error}"))
            .ok();
        Self {
            audio,
            hop: Sound::from_sfxr(&HOP),
            log: Sound::from_sfxr(&LOG),
            bell: Sound::from_sfxr(&BELL),
            horn: Sound::from_sfxr(&HORN),
            tick: Sound::from_sfxr(&TICK),
            best: Sound::from_sfxr(&BEST),
            squash: Sound::from_sfxr(&SQUASH),
            crash: Sound::from_sfxr(&CRASH),
            splash: Sound::from_sfxr(&SPLASH),
            timeout: Sound::from_sfxr(&TIMEOUT),
            game_over: Sound::from_sfxr(&GAME_OVER),
            start: Sound::from_sfxr(&START),
            pause: Sound::from_sfxr(&PAUSE),
            hops: 0,
        }
    }

    pub(super) fn set_muted(&self, muted: bool) {
        if let Some(audio) = &self.audio {
            audio.set_volume(if muted { 0.0 } else { 1.0 });
        }
    }

    pub(super) fn play(&mut self, cue: Cue) {
        let Some(audio) = &self.audio else {
            return;
        };
        let play = |sound: &Sound, volume: f32, pan: f32, pitch: f32| {
            audio.play_with(
                sound,
                Play {
                    volume,
                    pan,
                    pitch,
                    ..Play::default()
                },
            );
        };
        match cue {
            Cue::Start => play(&self.start, 0.7, 0.0, 1.0),
            Cue::Pause => play(&self.pause, 0.6, 0.0, 1.0),
            Cue::Resume => play(&self.pause, 0.6, 0.0, 1.25),
            Cue::Hop => {
                self.hops += 1;
                play(
                    &self.hop,
                    0.55,
                    0.0,
                    HOP_PITCHES[self.hops % HOP_PITCHES.len()],
                );
            }
            Cue::Log => play(&self.log, 0.7, 0.0, 1.0),
            Cue::Bell { pan, volume } => play(&self.bell, 0.35 * volume, pan, 1.0),
            Cue::Horn { pan, volume } => {
                play(&self.horn, 0.5 * volume, pan, 1.0);
                play(&self.horn, 0.35 * volume, pan, THIRD);
            }
            // The last seconds tick higher
            Cue::Tick(left) => play(&self.tick, 0.5, 0.0, 1.0 + (5 - left) as f32 * 0.08),
            Cue::Best => play(&self.best, 0.7, 0.0, 1.0),
            Cue::Squash => play(&self.squash, 0.9, 0.0, 1.0),
            Cue::Crash => {
                play(&self.crash, 1.0, 0.0, 1.0);
                play(&self.squash, 0.6, 0.0, 0.8);
            }
            Cue::Splash => play(&self.splash, 0.9, 0.0, 1.0),
            Cue::Timeout => play(&self.timeout, 0.7, 0.0, 1.0),
            Cue::GameOver => play(&self.game_over, 0.6, 0.0, 1.0),
        }
    }
}
