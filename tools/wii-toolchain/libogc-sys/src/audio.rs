/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use core::ffi::c_void;

pub const VOICE_STATE_STOPPED: u32 = 0;
pub const VOICE_STATE_RUNNING: u32 = 1;
pub const VOICE_STATE_STREAM: u32 = 2;
pub const VOICE_MONO8: u32 = 0;
pub const VOICE_STEREO8: u32 = 1;
pub const VOICE_MONO16: u32 = 2;
pub const VOICE_STEREO16: u32 = 3;
pub const VOICE_MONO8_UNSIGNED: u32 = 4;
pub const VOICE_STEREO8_UNSIGNED: u32 = 5;
pub const VOICE_MONO16_UNSIGNED: u32 = 6;
pub const VOICE_STEREO16_UNSIGNED: u32 = 7;

/// AESND's voice parameter block, the DSP mixes the voices in hardware.
#[repr(C)]
pub struct AESNDPB {
    _private: [u8; 0],
}

pub type AESNDVoiceCallbackArg =
    Option<extern "C" fn(pb: *mut AESNDPB, state: u32, arg: *mut c_void)>;

pub type AESNDAudioCallbackArg =
    Option<extern "C" fn(buffer: *mut c_void, length: u32, arg: *mut c_void)>;

unsafe extern "C" {
    pub fn AESND_Init();
    pub fn AESND_Reset();
    pub fn AESND_Pause(pause: bool);
    pub fn AESND_AllocateVoiceWithArg(
        callback: AESNDVoiceCallbackArg,
        arg: *mut c_void,
    ) -> *mut AESNDPB;
    pub fn AESND_FreeVoice(pb: *mut AESNDPB);
    pub fn AESND_SetVoiceStop(pb: *mut AESNDPB, stop: bool);
    pub fn AESND_SetVoiceMute(pb: *mut AESNDPB, mute: bool);
    pub fn AESND_SetVoiceLoop(pb: *mut AESNDPB, looping: bool);
    pub fn AESND_SetVoiceFormat(pb: *mut AESNDPB, format: u32);
    pub fn AESND_SetVoiceStream(pb: *mut AESNDPB, stream: bool);
    pub fn AESND_SetVoiceFrequency(pb: *mut AESNDPB, frequency: f32);
    pub fn AESND_SetVoiceDelay(pb: *mut AESNDPB, delay: u32);
    pub fn AESND_SetVoiceBuffer(pb: *mut AESNDPB, buffer: *const c_void, length: u32);
    pub fn AESND_RegisterVoiceCallbackWithArg(
        pb: *mut AESNDPB,
        callback: AESNDVoiceCallbackArg,
        arg: *mut c_void,
    ) -> AESNDVoiceCallbackArg;
    pub fn AESND_RegisterAudioCallbackWithArg(
        callback: AESNDAudioCallbackArg,
        arg: *mut c_void,
    ) -> AESNDAudioCallbackArg;
    pub fn AESND_SetVoiceVolume(pb: *mut AESNDPB, volume_l: u16, volume_r: u16);
    pub fn AESND_PlayVoice(
        pb: *mut AESNDPB,
        format: u32,
        buffer: *const c_void,
        len: u32,
        freq: f32,
        delay: u32,
        looped: bool,
    );
}
