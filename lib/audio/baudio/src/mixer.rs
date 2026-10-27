/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The mixer that runs in the audio callback, it only receives commands and never blocks.

use std::sync::mpsc::Receiver;

use crate::{Play, Sound};

/// The maximum number of voices, the oldest voice stops when another one starts
const MAX_VOICES: usize = 32;

/// The duration of volume ramps in seconds, they avoid clicks
const RAMP_SECONDS: f32 = 0.005;

/// Samples above this level are compressed by the limiter
const LIMITER_KNEE: f32 = 0.8;

pub(crate) enum Command {
    Play(VoiceState),
    Stop(u64),
    Volume(u64, f32),
    Pan(u64, f32),
    Pitch(u64, f32),
    MasterVolume(f32),
}

pub(crate) struct VoiceState {
    id: u64,
    sound: Sound,
    play: Play,
    position: f64,
    gain: f32,
    stopping: bool,
    finished: bool,
}

impl VoiceState {
    pub(crate) const fn new(id: u64, sound: Sound, play: Play) -> Self {
        Self {
            id,
            sound,
            play,
            position: 0.0,
            gain: play.volume,
            stopping: false,
            finished: false,
        }
    }

    /// The sound at the current position, linearly interpolated between samples
    fn sample(&self) -> f32 {
        let samples = &self.sound.samples;
        let index = self.position as usize;
        let fraction = (self.position - index as f64) as f32;
        let next = match samples.get(index + 1) {
            Some(next) => *next,
            None if self.play.looped => samples[0],
            None => 0.0,
        };
        samples[index] + (next - samples[index]) * fraction
    }

    /// Adds the voice to the interleaved output
    fn render(&mut self, output: &mut [f32], channels: usize, sample_rate: f64, smoothing: f32) {
        let length = self.sound.samples.len() as f64;
        let step = f64::from(self.sound.sample_rate) / sample_rate * f64::from(self.play.pitch);
        let pan = self.play.pan.clamp(-1.0, 1.0);
        let (left, right) = ((1.0 - pan).min(1.0), (1.0 + pan).min(1.0));
        let target = if self.stopping { 0.0 } else { self.play.volume };
        for frame in output.chunks_exact_mut(channels) {
            if self.position >= length {
                if self.play.looped && length > 0.0 {
                    self.position %= length;
                } else {
                    self.finished = true;
                    return;
                }
            }
            self.gain += (target - self.gain) * smoothing;
            let value = self.sample() * self.gain;
            if let [first, second, ..] = frame {
                *first += value * left;
                *second += value * right;
            } else {
                frame[0] += value * (left + right) / 2.0;
            }
            self.position += step;
            if self.stopping && self.gain < 0.0001 {
                self.finished = true;
                return;
            }
        }
    }
}

pub(crate) struct Mixer {
    commands: Receiver<Command>,
    voices: Vec<VoiceState>,
    sample_rate: f64,
    channels: usize,
    master: f32,
    master_target: f32,
    /// The ramp coefficient per frame
    smoothing: f32,
}

impl Mixer {
    pub(crate) fn new(commands: Receiver<Command>, sample_rate: u32, channels: u16) -> Self {
        Self {
            commands,
            voices: Vec::with_capacity(MAX_VOICES),
            sample_rate: f64::from(sample_rate),
            channels: usize::from(channels),
            master: 1.0,
            master_target: 1.0,
            smoothing: 1.0 - (-1.0 / (RAMP_SECONDS * sample_rate as f32)).exp(),
        }
    }

    fn voice(&mut self, id: u64) -> Option<&mut VoiceState> {
        self.voices.iter_mut().find(|voice| voice.id == id)
    }

    fn apply(&mut self, command: Command) {
        match command {
            Command::Play(voice) => {
                if self.voices.len() == MAX_VOICES {
                    self.voices.remove(0);
                }
                self.voices.push(voice);
            }
            Command::Stop(id) => {
                if let Some(voice) = self.voice(id) {
                    voice.stopping = true;
                }
            }
            Command::Volume(id, volume) => {
                if let Some(voice) = self.voice(id) {
                    voice.play.volume = volume;
                }
            }
            Command::Pan(id, pan) => {
                if let Some(voice) = self.voice(id) {
                    voice.play.pan = pan;
                }
            }
            Command::Pitch(id, pitch) => {
                if let Some(voice) = self.voice(id) {
                    voice.play.pitch = pitch;
                }
            }
            Command::MasterVolume(volume) => self.master_target = volume,
        }
    }

    /// Mixes all voices into the interleaved output
    pub(crate) fn render(&mut self, output: &mut [f32]) {
        while let Ok(command) = self.commands.try_recv() {
            self.apply(command);
        }
        output.fill(0.0);
        for voice in &mut self.voices {
            voice.render(output, self.channels, self.sample_rate, self.smoothing);
        }
        self.voices.retain(|voice| !voice.finished);
        for frame in output.chunks_exact_mut(self.channels) {
            self.master += (self.master_target - self.master) * self.smoothing;
            for sample in frame {
                *sample = limit(*sample * self.master);
            }
        }
    }
}

/// A soft limiter, many loud voices together saturate smoothly instead of clipping
fn limit(sample: f32) -> f32 {
    let level = sample.abs();
    if level <= LIMITER_KNEE {
        sample
    } else {
        let over = (level - LIMITER_KNEE) / (1.0 - LIMITER_KNEE);
        (LIMITER_KNEE + (1.0 - LIMITER_KNEE) * over.tanh()).copysign(sample)
    }
}

// MARK: Tests
#[cfg(test)]
mod test {
    use std::sync::mpsc;

    use super::*;

    fn mixer(rate: u32, channels: u16) -> (mpsc::Sender<Command>, Mixer) {
        let (sender, receiver) = mpsc::channel();
        (sender, Mixer::new(receiver, rate, channels))
    }

    #[test]
    fn voices_mix_with_pan_and_finish() {
        let (sender, mut mixer) = mixer(4, 2);
        let sound = Sound::new(4, vec![0.5; 2]);
        let left = Play {
            pan: -1.0,
            ..Play::default()
        };
        sender
            .send(Command::Play(VoiceState::new(1, sound.clone(), left)))
            .unwrap();
        sender
            .send(Command::Play(VoiceState::new(2, sound, Play::default())))
            .unwrap();
        let mut output = [0.0; 6];
        mixer.render(&mut output);
        assert_eq!(&output[..2], &[limit(1.0), 0.5]);
        assert_eq!(&output[4..], &[0.0, 0.0]);
        assert!(mixer.voices.is_empty());
    }

    #[test]
    fn pitch_resamples_and_loops_wrap() {
        let (sender, mut mixer) = mixer(2, 1);
        let sound = Sound::new(1, vec![0.0, 0.4]);
        let play = Play {
            looped: true,
            ..Play::default()
        };
        sender
            .send(Command::Play(VoiceState::new(1, sound, play)))
            .unwrap();
        let mut output = [0.0; 5];
        mixer.render(&mut output);
        // The sound is played at half speed, so every other sample is interpolated
        for (value, expected) in output.iter().zip([0.0, 0.2, 0.4, 0.2, 0.0]) {
            assert!((value - expected).abs() < 0.0001);
        }
        assert_eq!(mixer.voices.len(), 1);
    }

    #[test]
    fn stopped_voices_fade_out() {
        let (sender, mut mixer) = mixer(48_000, 1);
        let sound = Sound::new(48_000, vec![0.5; 48_000]);
        sender
            .send(Command::Play(VoiceState::new(1, sound, Play::default())))
            .unwrap();
        sender.send(Command::Stop(1)).unwrap();
        let mut output = vec![0.0; 4_800];
        mixer.render(&mut output);
        assert!(output[0] > 0.4 && output.windows(2).all(|pair| pair[1] <= pair[0]));
        assert!(mixer.voices.is_empty());
    }

    #[test]
    fn the_oldest_voice_makes_room() {
        let (sender, mut mixer) = mixer(48_000, 1);
        let sound = Sound::new(48_000, vec![0.0; 48_000]);
        for id in 0..=MAX_VOICES as u64 {
            sender
                .send(Command::Play(VoiceState::new(
                    id,
                    sound.clone(),
                    Play::default(),
                )))
                .unwrap();
        }
        mixer.render(&mut [0.0; 16]);
        assert_eq!(mixer.voices.len(), MAX_VOICES);
        assert_eq!(mixer.voices[0].id, 1);
    }

    #[test]
    fn limiter_is_smooth_and_bounded() {
        assert_eq!(limit(0.5), 0.5);
        assert!(limit(4.0) <= 1.0 && limit(0.9) < 0.9 && limit(0.9) > 0.85);
        assert_eq!(limit(-2.0), -limit(2.0));
    }
}
