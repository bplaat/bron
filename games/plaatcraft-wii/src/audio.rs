/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The sound effects rendered at startup, the DSP mixes them with AESND and changes their
//! pitch, volume and pan in hardware.

use alloc::vec::Vec;
use core::ptr;

use libogc_sys::*;
use plaatcraft_engine::client::Audio;
use plaatcraft_engine::client::sound::samples;
use plaatcraft_engine::sound::Effect;

use crate::memory;

/// The voices played in turn, a new sound cuts the oldest one off
const VOICES: usize = 16;
/// AESND's full voice volume
const FULL_VOLUME: f32 = 256.0;

pub struct Aesnd {
    // 16-bit mono samples, big-endian like the DSP expects
    sounds: [Vec<i16>; Effect::ALL.len()],
    voices: [*mut AESNDPB; VOICES],
    next: usize,
}

impl Aesnd {
    pub fn new() -> Self {
        unsafe {
            AESND_Init();
            AESND_Pause(false);
        }
        Self {
            sounds: Effect::ALL.map(|effect| {
                let samples = samples(effect);
                memory::flush(&samples);
                samples
            }),
            voices: core::array::from_fn(|_| unsafe {
                AESND_AllocateVoiceWithArg(None, ptr::null_mut())
            }),
            next: 0,
        }
    }
}

impl Audio for Aesnd {
    fn play(&mut self, effect: Effect, [left, right]: [f32; 2], pitch: f32) {
        let voice = self.voices[self.next];
        self.next = (self.next + 1) % VOICES;
        if voice.is_null() {
            return;
        }
        let samples = &self.sounds[effect as usize];
        let volume = |side: f32| (side * FULL_VOLUME) as u16;
        unsafe {
            AESND_SetVoiceVolume(voice, volume(left), volume(right));
            AESND_PlayVoice(
                voice,
                VOICE_MONO16,
                samples.as_ptr().cast(),
                size_of_val(samples.as_slice()) as u32,
                sfxr::SAMPLE_RATE as f32 * pitch,
                0,
                false,
            );
        }
    }
}

// Stops the DSP and the audio DMA, which would otherwise keep repeating the last buffer in the
// loader
impl Drop for Aesnd {
    fn drop(&mut self) {
        unsafe { AESND_Reset() };
    }
}
