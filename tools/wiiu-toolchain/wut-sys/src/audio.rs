/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use core::ffi::c_void;

use crate::BOOL;

pub const AX_INIT_RENDERER_32KHZ: u32 = 0;
pub const AX_INIT_RENDERER_48KHZ: u32 = 1;
pub const AX_INIT_PIPELINE_SINGLE: u32 = 0;
pub const AX_INIT_PIPELINE_FOUR_STAGE: u32 = 1;
pub const AX_VOICE_FORMAT_ADPCM: u16 = 0;
pub const AX_VOICE_FORMAT_LPCM16: u16 = 10;
pub const AX_VOICE_FORMAT_LPCM8: u16 = 25;
pub const AX_VOICE_LOOP_DISABLED: u16 = 0;
pub const AX_VOICE_LOOP_ENABLED: u16 = 1;
pub const AX_VOICE_STATE_STOPPED: u32 = 0;
pub const AX_VOICE_STATE_PLAYING: u32 = 1;
pub const AX_VOICE_SRC_TYPE_NONE: u32 = 0;
pub const AX_VOICE_SRC_TYPE_LINEAR: u32 = 1;
pub const AX_DEVICE_TYPE_TV: u32 = 0;
pub const AX_DEVICE_TYPE_DRC: u32 = 1;
pub const AX_DEVICE_TYPE_CONTROLLER: u32 = 2;
pub const AX_RESULT_SUCCESS: i32 = 0;

pub type AXFrameCallback = Option<unsafe extern "C" fn()>;
pub type AXVoiceCallbackFn = Option<unsafe extern "C" fn(*mut c_void)>;

/// Voice objects are allocated by `AXAcquireVoice` and released by `AXFreeVoice`.
#[repr(C)]
pub struct AXVoice {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Default)]
pub struct AXInitParams {
    pub renderer: u32,
    _unknown: [u8; 4],
    pub pipeline: u32,
}

/// Offsets count samples, with `end_offset` pointing to the last sample.
/// Keep `data` alive and flush its CPU cache before starting playback.
#[repr(C)]
pub struct AXVoiceOffsets {
    pub data_type: u16,
    pub looping_enabled: u16,
    pub loop_offset: u32,
    pub end_offset: u32,
    pub current_offset: u32,
    pub data: *const c_void,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AXVoiceDeviceBusMixData {
    pub volume: u16,
    pub delta: i16,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AXVoiceDeviceMixData {
    pub bus: [AXVoiceDeviceBusMixData; 4],
}

#[repr(C)]
#[derive(Default)]
pub struct AXVoiceVeData {
    pub volume: u16,
    pub delta: i16,
}

unsafe extern "C" {
    pub fn AXInit();
    pub fn AXInitWithParams(params: *mut AXInitParams);
    pub fn AXQuit();
    pub fn AXIsInit() -> BOOL;
    pub fn AXRegisterFrameCallback(callback: AXFrameCallback) -> AXFrameCallback;
    pub fn AXGetInputSamplesPerSec() -> u32;
    pub fn AXGetInputSamplesPerFrame() -> u32;
    pub fn AXAcquireVoice(
        priority: u32,
        callback: AXVoiceCallbackFn,
        context: *mut c_void,
    ) -> *mut AXVoice;
    pub fn AXFreeVoice(voice: *mut AXVoice);
    pub fn AXSetVoiceOffsets(voice: *mut AXVoice, offsets: *mut AXVoiceOffsets);
    pub fn AXGetVoiceOffsets(voice: *mut AXVoice, offsets: *mut AXVoiceOffsets);
    pub fn AXSetVoiceState(voice: *mut AXVoice, state: u32);
    pub fn AXIsVoiceRunning(voice: *mut AXVoice) -> BOOL;
    pub fn AXSetVoiceSrcType(voice: *mut AXVoice, kind: u32);
    pub fn AXSetVoiceSrcRatio(voice: *mut AXVoice, ratio: f32) -> i32;
    pub fn AXSetVoiceVe(voice: *mut AXVoice, volume: *mut AXVoiceVeData);
    /// `mix` points to one mix entry per channel of the selected device.
    pub fn AXSetVoiceDeviceMix(
        voice: *mut AXVoice,
        device: u32,
        id: u32,
        mix: *mut AXVoiceDeviceMixData,
    ) -> i32;
    pub fn AXSetVoiceLoop(voice: *mut AXVoice, looping: u16);
}
