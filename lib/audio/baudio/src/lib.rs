/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
pub use cpal::{Error, ErrorKind};

use crate::mixer::{Command, Mixer, VoiceState};

mod mixer;

/// The volume of rendered sfxr sounds, louder than the sfxr default of `0.2`
const SFXR_VOLUME: f32 = 0.5;

// MARK: Sound
/// A mono sound in memory, cloning it is cheap because the samples are shared.
#[derive(Clone, Debug)]
pub struct Sound {
    sample_rate: u32,
    samples: Arc<[f32]>,
}

impl Sound {
    /// Creates a sound from mono samples in `-1.0..=1.0` at the given sample rate.
    pub fn new(sample_rate: u32, samples: impl Into<Arc<[f32]>>) -> Self {
        Self {
            sample_rate,
            samples: samples.into(),
        }
    }

    /// Renders a sfxr sample until its envelope ends.
    ///
    /// # Panics
    ///
    /// Panics when a field of the sample is out of its range.
    pub fn from_sfxr(sample: &sfxr::Sample) -> Self {
        let mut generator = sfxr::Generator::new(*sample);
        generator.volume = SFXR_VOLUME;
        let mut samples = Vec::new();
        let mut chunk = [0.0; 1024];
        while !generator.finished() {
            generator.generate(&mut chunk);
            samples.extend_from_slice(&chunk);
        }
        Self::new(sfxr::SAMPLE_RATE, samples)
    }

    /// Returns the sample rate in Hz.
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Returns the mono samples.
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// Returns the duration at normal pitch.
    pub fn duration(&self) -> Duration {
        Duration::from_secs_f64(self.samples.len() as f64 / f64::from(self.sample_rate.max(1)))
    }
}

// MARK: Play
/// How a sound is played.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Play {
    /// Linear volume, `1.0` plays the sound as is.
    pub volume: f32,
    /// Stereo position from `-1.0` left to `1.0` right.
    pub pan: f32,
    /// Playback speed, `2.0` plays an octave higher and twice as fast.
    pub pitch: f32,
    /// Repeats the sound until the voice is stopped.
    pub looped: bool,
}

impl Default for Play {
    fn default() -> Self {
        Self {
            volume: 1.0,
            pan: 0.0,
            pitch: 1.0,
            looped: false,
        }
    }
}

// MARK: Voice
/// A playing sound, it keeps playing when the handle is dropped.
#[derive(Clone, Debug)]
pub struct Voice {
    id: u64,
    commands: Sender<Command>,
}

impl Voice {
    // Sending fails when the audio is closed, then there is nothing to change
    fn send(&self, command: Command) {
        _ = self.commands.send(command);
    }

    /// Stops the voice with a short fade out.
    pub fn stop(&self) {
        self.send(Command::Stop(self.id));
    }

    /// Changes the volume, with a short ramp.
    pub fn set_volume(&self, volume: f32) {
        self.send(Command::Volume(self.id, volume));
    }

    /// Changes the stereo position.
    pub fn set_pan(&self, pan: f32) {
        self.send(Command::Pan(self.id, pan));
    }

    /// Changes the playback speed.
    pub fn set_pitch(&self, pitch: f32) {
        self.send(Command::Pitch(self.id, pitch));
    }
}

// MARK: Audio
/// The audio output, it mixes all playing voices. Closing it stops all sounds.
pub struct Audio {
    commands: Sender<Command>,
    next_id: AtomicU64,
    stream: cpal::Stream,
}

impl Audio {
    /// Opens the default output device and starts playing silence.
    ///
    /// Errors of the running stream are ignored, the sounds stop playing then.
    pub fn new() -> Result<Self, Error> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or(ErrorKind::DeviceNotAvailable)?;
        let config = device.default_output_config()?.config();
        let (commands, receiver) = mpsc::channel();
        let mut mixer = Mixer::new(receiver, config.sample_rate, config.channels);
        let stream = device.build_output_stream(
            config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| mixer.render(data),
            |_| {},
            None,
        )?;
        stream.play()?;
        Ok(Self {
            commands,
            next_id: AtomicU64::new(0),
            stream,
        })
    }

    /// Plays a sound once at full volume.
    pub fn play(&self, sound: &Sound) -> Voice {
        self.play_with(sound, Play::default())
    }

    /// Plays a sound with the given settings.
    pub fn play_with(&self, sound: &Sound, play: Play) -> Voice {
        let voice = Voice {
            id: self.next_id.fetch_add(1, Ordering::Relaxed),
            commands: self.commands.clone(),
        };
        voice.send(Command::Play(VoiceState::new(
            voice.id,
            sound.clone(),
            play,
        )));
        voice
    }

    /// Changes the master volume of all voices, with a short ramp.
    pub fn set_volume(&self, volume: f32) {
        _ = self.commands.send(Command::MasterVolume(volume));
    }

    /// Pauses the output, for example while the app is in the background.
    pub fn pause(&self) -> Result<(), Error> {
        self.stream.pause()
    }

    /// Resumes the output after a pause.
    pub fn resume(&self) -> Result<(), Error> {
        self.stream.play()
    }
}
