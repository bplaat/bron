/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The sound effects rendered at startup, the DSP mixes them with AX and changes their pitch
//! and volume, on the TV and the GamePad.

use alloc::vec::Vec;
use core::ptr;

use plaatcraft_engine::client::Audio;
use plaatcraft_engine::client::sound::samples;
use plaatcraft_engine::sound::Effect;
use wut_sys::*;

use crate::memory;

/// The voices played in turn, a new sound cuts the oldest one off
const VOICES: usize = 16;
/// AX's full volume, 1.0 in 1.15 fixed point
const FULL_VOLUME: f32 = 32768.0;
const TV_CHANNELS: usize = 6;
const DRC_CHANNELS: usize = 4;

pub struct Ax {
    // 16-bit mono samples, big-endian like the DSP expects
    sounds: [Vec<i16>; Effect::ALL.len()],
    voices: [*mut AXVoice; VOICES],
    next: usize,
}

impl Ax {
    pub fn new() -> Self {
        let mut params = AXInitParams::default();
        params.renderer = AX_INIT_RENDERER_48KHZ;
        params.pipeline = AX_INIT_PIPELINE_SINGLE;
        unsafe { AXInitWithParams(&mut params) };
        Self {
            sounds: Effect::ALL.map(|effect| {
                let samples = samples(effect);
                memory::flush(&samples);
                samples
            }),
            voices: core::array::from_fn(|_| unsafe { AXAcquireVoice(31, None, ptr::null_mut()) }),
            next: 0,
        }
    }
}

impl Audio for Ax {
    fn play(&mut self, effect: Effect, [left, right]: [f32; 2], pitch: f32) {
        let voice = self.voices[self.next];
        self.next = (self.next + 1) % VOICES;
        let samples = &self.sounds[effect as usize];
        if voice.is_null() || samples.is_empty() {
            return;
        }
        let mut offsets = AXVoiceOffsets {
            data_type: AX_VOICE_FORMAT_LPCM16,
            looping_enabled: AX_VOICE_LOOP_DISABLED,
            loop_offset: 0,
            end_offset: samples.len() as u32 - 1,
            current_offset: 0,
            data: samples.as_ptr().cast(),
        };
        let volume = |side: f32| {
            let mut mix = AXVoiceDeviceMixData::default();
            mix.bus[0].volume = (side.clamp(0.0, 1.0) * (FULL_VOLUME - 1.0)) as u16;
            mix
        };
        let mut tv = [AXVoiceDeviceMixData::default(); TV_CHANNELS];
        let mut drc = [AXVoiceDeviceMixData::default(); DRC_CHANNELS];
        for mix in [&mut tv[..], &mut drc[..]] {
            mix[0] = volume(left);
            mix[1] = volume(right);
        }
        let mut ve = AXVoiceVeData {
            volume: 0x8000,
            delta: 0,
        };
        let ratio = sfxr::SAMPLE_RATE as f32 * pitch / unsafe { AXGetInputSamplesPerSec() } as f32;
        unsafe {
            AXVoiceBegin(voice);
            AXSetVoiceState(voice, AX_VOICE_STATE_STOPPED);
            AXSetVoiceOffsets(voice, &mut offsets);
            AXSetVoiceSrcType(voice, AX_VOICE_SRC_TYPE_LINEAR);
            AXSetVoiceSrcRatio(voice, ratio);
            AXSetVoiceVe(voice, &mut ve);
            AXSetVoiceDeviceMix(voice, AX_DEVICE_TYPE_TV, 0, tv.as_mut_ptr());
            AXSetVoiceDeviceMix(voice, AX_DEVICE_TYPE_DRC, 0, drc.as_mut_ptr());
            AXSetVoiceState(voice, AX_VOICE_STATE_PLAYING);
            AXVoiceEnd(voice);
        }
    }
}

impl Drop for Ax {
    fn drop(&mut self) {
        unsafe {
            for &voice in &self.voices {
                if !voice.is_null() {
                    AXFreeVoice(voice);
                }
            }
            AXQuit();
        }
    }
}
