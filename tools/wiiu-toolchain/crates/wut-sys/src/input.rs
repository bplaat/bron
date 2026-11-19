/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use crate::BOOL;

pub const VPAD_BUTTON_A: u32 = 0x8000;
pub const VPAD_BUTTON_B: u32 = 0x4000;
pub const VPAD_BUTTON_X: u32 = 0x2000;
pub const VPAD_BUTTON_Y: u32 = 0x1000;
pub const VPAD_BUTTON_LEFT: u32 = 0x0800;
pub const VPAD_BUTTON_RIGHT: u32 = 0x0400;
pub const VPAD_BUTTON_UP: u32 = 0x0200;
pub const VPAD_BUTTON_DOWN: u32 = 0x0100;
pub const VPAD_BUTTON_ZL: u32 = 0x0080;
pub const VPAD_BUTTON_ZR: u32 = 0x0040;
pub const VPAD_BUTTON_L: u32 = 0x0020;
pub const VPAD_BUTTON_R: u32 = 0x0010;
pub const VPAD_BUTTON_PLUS: u32 = 0x0008;
pub const VPAD_BUTTON_MINUS: u32 = 0x0004;
pub const VPAD_BUTTON_HOME: u32 = 0x0002;
pub const VPAD_BUTTON_SYNC: u32 = 0x0001;
pub const VPAD_BUTTON_STICK_R: u32 = 0x00020000;
pub const VPAD_BUTTON_STICK_L: u32 = 0x00040000;
pub const VPAD_BUTTON_TV: u32 = 0x00010000;
pub const VPAD_STICK_R_EMULATION_LEFT: u32 = 0x04000000;
pub const VPAD_STICK_R_EMULATION_RIGHT: u32 = 0x02000000;
pub const VPAD_STICK_R_EMULATION_UP: u32 = 0x01000000;
pub const VPAD_STICK_R_EMULATION_DOWN: u32 = 0x00800000;
pub const VPAD_STICK_L_EMULATION_LEFT: u32 = 0x40000000;
pub const VPAD_STICK_L_EMULATION_RIGHT: u32 = 0x20000000;
pub const VPAD_STICK_L_EMULATION_UP: u32 = 0x10000000;
pub const VPAD_STICK_L_EMULATION_DOWN: u32 = 0x08000000;
pub const VPAD_VALID: u16 = 0x0;
pub const VPAD_INVALID_X: u16 = 0x1;
pub const VPAD_INVALID_Y: u16 = 0x2;
pub const VPAD_TP_1920X1080: u32 = 0;
pub const VPAD_TP_1280X720: u32 = 1;
pub const VPAD_TP_854X480: u32 = 2;
pub const VPAD_READ_SUCCESS: i32 = 0;
pub const VPAD_READ_NO_SAMPLES: i32 = -1;
pub const VPAD_READ_INVALID_CONTROLLER: i32 = -2;
pub const VPAD_READ_BUSY: i32 = -4;
pub const VPAD_READ_UNINITIALIZED: i32 = -5;
pub const VPAD_CHAN_0: u32 = 0;
pub const VPAD_CHAN_1: u32 = 1;
pub const WPAD_CHAN_0: u32 = 0;
pub const WPAD_CHAN_1: u32 = 1;
pub const WPAD_CHAN_2: u32 = 2;
pub const WPAD_CHAN_3: u32 = 3;
pub const WPAD_CHAN_4: u32 = 4;
pub const WPAD_CHAN_5: u32 = 5;
pub const WPAD_CHAN_6: u32 = 6;
pub const WPAD_FMT_CORE: u8 = 0x00;
pub const WPAD_FMT_CORE_ACC: u8 = 0x01;
pub const WPAD_FMT_CORE_ACC_DPD: u8 = 0x02;
pub const WPAD_FMT_NUNCHUK: u8 = 0x03;
pub const WPAD_FMT_NUNCHUK_ACC: u8 = 0x04;
pub const WPAD_FMT_NUNCHUK_ACC_DPD: u8 = 0x05;
pub const WPAD_FMT_CLASSIC: u8 = 0x06;
pub const WPAD_FMT_CLASSIC_ACC: u8 = 0x07;
pub const WPAD_FMT_CLASSIC_ACC_DPD: u8 = 0x08;
pub const WPAD_FMT_CORE_ACC_DPD_FULL: u8 = 0x09;
pub const WPAD_FMT_TRAIN: u8 = 0x0A;
pub const WPAD_FMT_GUITAR: u8 = 0x0B;
pub const WPAD_FMT_BALANCE_BOARD: u8 = 0x0C;
pub const WPAD_FMT_DRUM: u8 = 0x0F;
pub const WPAD_FMT_MPLUS: u8 = 0x10;
pub const WPAD_FMT_TAIKO: u8 = 0x11;
pub const WPAD_FMT_PRO_CONTROLLER: u8 = 0x16;
pub const WPAD_EXT_CORE: u8 = 0x00;
pub const WPAD_EXT_NUNCHUK: u8 = 0x01;
pub const WPAD_EXT_CLASSIC: u8 = 0x02;
pub const WPAD_EXT_BALANCE_BOARD: u8 = 0x03;
pub const WPAD_EXT_MPLUS: u8 = 0x05;
pub const WPAD_EXT_MPLUS_NUNCHUK: u8 = 0x06;
pub const WPAD_EXT_MPLUS_CLASSIC: u8 = 0x07;
pub const WPAD_EXT_TRAIN: u8 = 0x10;
pub const WPAD_EXT_GUITAR: u8 = 0x11;
pub const WPAD_EXT_DRUM: u8 = 0x12;
pub const WPAD_EXT_TAIKO: u8 = 0x13;
pub const WPAD_EXT_PRO_CONTROLLER: u8 = 0x1f;
pub const WPAD_EXT_DEV_NOT_FOUND: u8 = 0xfd;
pub const WPAD_EXT_UNKNOWN: u8 = 0xff;
pub const WPAD_BUTTON_LEFT: u32 = 0x0001;
pub const WPAD_BUTTON_RIGHT: u32 = 0x0002;
pub const WPAD_BUTTON_DOWN: u32 = 0x0004;
pub const WPAD_BUTTON_UP: u32 = 0x0008;
pub const WPAD_BUTTON_PLUS: u32 = 0x0010;
pub const WPAD_BUTTON_2: u32 = 0x0100;
pub const WPAD_BUTTON_1: u32 = 0x0200;
pub const WPAD_BUTTON_B: u32 = 0x0400;
pub const WPAD_BUTTON_A: u32 = 0x0800;
pub const WPAD_BUTTON_MINUS: u32 = 0x1000;
pub const WPAD_BUTTON_Z: u32 = 0x2000;
pub const WPAD_BUTTON_C: u32 = 0x4000;
pub const WPAD_BUTTON_HOME: u32 = 0x8000;
pub const WPAD_NUNCHUK_BUTTON_Z: u32 = 0x2000;
pub const WPAD_NUNCHUK_BUTTON_C: u32 = 0x4000;
pub const WPAD_PRO_BUTTON_UP: u32 = 0x00000001;
pub const WPAD_PRO_BUTTON_LEFT: u32 = 0x00000002;
pub const WPAD_PRO_BUTTON_ZR: u32 = 0x00000004;
pub const WPAD_PRO_BUTTON_X: u32 = 0x00000008;
pub const WPAD_PRO_BUTTON_A: u32 = 0x00000010;
pub const WPAD_PRO_BUTTON_Y: u32 = 0x00000020;
pub const WPAD_PRO_BUTTON_B: u32 = 0x00000040;
pub const WPAD_PRO_BUTTON_ZL: u32 = 0x00000080;
pub const WPAD_PRO_BUTTON_R: u32 = 0x00000200;
pub const WPAD_PRO_BUTTON_PLUS: u32 = 0x00000400;
pub const WPAD_PRO_BUTTON_HOME: u32 = 0x00000800;
pub const WPAD_PRO_BUTTON_MINUS: u32 = 0x00001000;
pub const WPAD_PRO_BUTTON_L: u32 = 0x00002000;
pub const WPAD_PRO_BUTTON_DOWN: u32 = 0x00004000;
pub const WPAD_PRO_BUTTON_RIGHT: u32 = 0x00008000;
pub const WPAD_PRO_BUTTON_STICK_R: u32 = 0x00010000;
pub const WPAD_PRO_BUTTON_STICK_L: u32 = 0x00020000;
pub const KPAD_ERROR_OK: i32 = 0;
pub const KPAD_ERROR_NO_SAMPLES: i32 = -1;
pub const KPAD_ERROR_INVALID_CONTROLLER: i32 = -2;
pub const KPAD_ERROR_WPAD_UNINIT: i32 = -3;
pub const KPAD_ERROR_BUSY: i32 = -4;
pub const KPAD_ERROR_UNINITIALIZED: i32 = -5;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct VPADVec2D {
    pub x: f32,
    pub y: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct VPADVec3D {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct VPADDirection {
    pub x: VPADVec3D,
    pub y: VPADVec3D,
    pub z: VPADVec3D,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct VPADAccStatus {
    pub acc: VPADVec3D,
    pub magnitude: f32,
    pub variation: f32,
    pub vertical: VPADVec2D,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct VPADTouchData {
    pub x: u16,
    pub y: u16,
    pub touched: u16,
    pub validity: u16,
}

#[repr(C)]
#[derive(Default)]
pub struct VPADStatus {
    pub hold: u32,
    pub trigger: u32,
    pub release: u32,
    pub left_stick: VPADVec2D,
    pub right_stick: VPADVec2D,
    pub accelerometer: VPADAccStatus,
    pub gyro: VPADVec3D,
    pub angle: VPADVec3D,
    pub error: u8,
    _unknown: u8,
    pub tp_normal: VPADTouchData,
    pub tp_filtered1: VPADTouchData,
    pub tp_filtered2: VPADTouchData,
    _padding: [u8; 2],
    pub direction: VPADDirection,
    pub using_headphones: BOOL,
    pub mag: VPADVec3D,
    pub slide_volume: u8,
    pub battery: u8,
    pub mic_status: u8,
    pub slide_volume_ex: u8,
    _rest: [u8; 8],
}

pub type KPADVec2D = VPADVec2D;
pub type KPADVec3D = VPADVec3D;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct KPADNunchukStatus {
    pub stick: KPADVec2D,
    pub acc: KPADVec3D,
    pub acc_magnitude: f32,
    pub acc_variation: f32,
    pub hold: u32,
    pub trigger: u32,
    pub release: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct KPADClassicStatus {
    pub hold: u32,
    pub trigger: u32,
    pub release: u32,
    pub left_stick: KPADVec2D,
    pub right_stick: KPADVec2D,
    pub left_trigger: f32,
    pub right_trigger: f32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct KPADProStatus {
    pub hold: u32,
    pub trigger: u32,
    pub release: u32,
    pub left_stick: KPADVec2D,
    pub right_stick: KPADVec2D,
    pub charging: i32,
    pub wired: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct KPADBalanceStatus {
    pub avg_tgc_weight: f64,
    pub weights: [f64; 4],
    pub avg_weights: [f64; 4],
    pub error: i32,
    pub calibration: i32,
}

/// Read only the member selected by `KPADStatus::extension_type`.
#[repr(C)]
pub union KPADExtensionStatus {
    pub nunchuk: KPADNunchukStatus,
    pub classic: KPADClassicStatus,
    pub pro: KPADProStatus,
    pub balance: KPADBalanceStatus,
}

#[repr(C)]
pub struct KPADMotionPlusStatus {
    pub acc: KPADVec3D,
    pub angles: KPADVec3D,
    pub dir_x: KPADVec3D,
    pub dir_y: KPADVec3D,
    pub dir_z: KPADVec3D,
}

#[repr(C)]
pub struct KPADStatus {
    pub hold: u32,
    pub trigger: u32,
    pub release: u32,
    pub acc: KPADVec3D,
    pub acc_magnitude: f32,
    pub acc_variation: f32,
    pub pos: KPADVec2D,
    pub pos_diff: KPADVec2D,
    pub pos_diff_magnitude: f32,
    pub angle: KPADVec2D,
    pub angle_diff: KPADVec2D,
    pub angle_diff_magnitude: f32,
    pub dist: f32,
    pub dist_diff: f32,
    pub dist_diff_magnitude: f32,
    pub down: KPADVec2D,
    pub extension_type: u8,
    pub error: i8,
    pub pos_valid: i8,
    pub format: u8,
    pub extension: KPADExtensionStatus,
    pub mplus: KPADMotionPlusStatus,
    _padding: [u8; 4],
}

unsafe extern "C" {
    pub fn VPADInit();
    pub fn VPADShutdown();
    /// `count` is the number of samples, not the buffer size in bytes.
    pub fn VPADRead(channel: u32, buffers: *mut VPADStatus, count: u32, error: *mut i32) -> i32;
    pub fn VPADGetTPCalibratedPoint(
        channel: u32,
        calibrated: *mut VPADTouchData,
        raw: *const VPADTouchData,
    );
    pub fn KPADInit();
    pub fn KPADShutdown();
    pub fn KPADRead(channel: u32, buffers: *mut KPADStatus, count: u32) -> u32;
    pub fn KPADReadEx(channel: u32, buffers: *mut KPADStatus, count: u32, error: *mut i32) -> u32;
    pub fn WPADEnableURCC(enable: BOOL);
}
