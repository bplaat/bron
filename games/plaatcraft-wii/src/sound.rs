/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The sound effects of plaatcraft-engine, rendered from their sfxr samples at startup. The
//! DSP mixes them with AESND and changes their pitch, volume and pan in hardware.

use alloc::vec::Vec;
use core::ptr;

use libogc_sys::*;
use plaatcraft_engine::camera::Camera;
use plaatcraft_engine::menu::MAX_VOLUME;
use plaatcraft_engine::sound::{Effect, Footsteps, Movement, Play};

use crate::memory;

/// The voices played in turn, a new sound cuts the oldest one off
const VOICES: usize = 16;

/// AESND's full voice volume
const FULL_VOLUME: f32 = 256.0;

/// Renders an effect like baudio does on the desktop, as 16-bit mono samples that are big-endian
/// like the DSP expects
fn render(effect: Effect) -> Vec<i16> {
    let mut generator = sfxr::Generator::new(effect.sample());
    generator.volume = 0.5;
    let mut samples = Vec::new();
    let mut chunk = [0.0; 1024];
    while !generator.finished() {
        generator.generate(&mut chunk);
        samples.extend(chunk.map(|sample| (sample * 32767.0) as i16));
    }
    memory::flush(&samples);
    samples
}

struct Mixer {
    sounds: [Vec<i16>; Effect::ALL.len()],
    voices: [*mut AESNDPB; VOICES],
    next: usize,
    volume: f32,
}

impl Mixer {
    fn play(&mut self, play: Play) {
        let voice = self.voices[self.next];
        self.next = (self.next + 1) % VOICES;
        if voice.is_null() {
            return;
        }
        let samples = &self.sounds[play.effect as usize];
        let pan = play.pan.clamp(-1.0, 1.0);
        let volume = |side: f32| (play.volume * self.volume * side.min(1.0) * FULL_VOLUME) as u16;
        unsafe {
            AESND_SetVoiceVolume(voice, volume(1.0 - pan), volume(1.0 + pan));
            AESND_PlayVoice(
                voice,
                VOICE_MONO16,
                samples.as_ptr().cast(),
                size_of_val(samples.as_slice()) as u32,
                sfxr::SAMPLE_RATE as f32 * play.pitch,
                0,
                false,
            );
        }
    }
}

pub struct Sounds {
    mixer: Mixer,
    footsteps: Footsteps,
}

impl Sounds {
    pub fn new() -> Self {
        unsafe {
            AESND_Init();
            AESND_Pause(false);
        }
        Self {
            mixer: Mixer {
                sounds: Effect::ALL.map(render),
                voices: core::array::from_fn(|_| unsafe {
                    AESND_AllocateVoiceWithArg(None, ptr::null_mut())
                }),
                next: 0,
                volume: 1.0,
            },
            footsteps: Footsteps::default(),
        }
    }

    pub fn set_volume(&mut self, volume: u8) {
        self.mixer.volume = f32::from(volume) / f32::from(MAX_VOLUME);
    }

    /// A menu or hotbar click
    pub fn click(&mut self) {
        self.mixer.play(Play::click());
    }

    /// Taking off or landing from flight
    pub fn fly(&mut self, flying: bool) {
        self.mixer.play(Play::fly(flying));
    }

    /// A broken or placed block
    pub fn block(&mut self, camera: &Camera, position: [i64; 3], material: u8, broken: bool) {
        Play::block(camera, position, material, broken, |play| {
            self.mixer.play(play);
        });
    }

    /// Jumps, footsteps, splashes and landings since `before`
    pub fn movement(&mut self, before: Movement, camera: &Camera, ground: Option<u8>) {
        let mixer = &mut self.mixer;
        self.footsteps
            .update(before, camera, ground, |play| mixer.play(play));
    }
}

// Stops the DSP and the audio DMA, which would otherwise keep repeating the last buffer in the
// loader
impl Drop for Sounds {
    fn drop(&mut self) {
        unsafe { AESND_Reset() };
    }
}
