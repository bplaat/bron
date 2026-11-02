/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use core::ffi::c_int;

pub const WPAD_CHAN_ALL: c_int = -1;
pub const WPAD_FMT_BTNS: c_int = 0;
pub const WPAD_FMT_BTNS_ACC: c_int = 1;
pub const WPAD_FMT_BTNS_ACC_IR: c_int = 2;
pub const WPAD_EXP_NUNCHUK: c_int = 1;
pub const WPAD_BUTTON_2: u32 = 0x0001;
pub const WPAD_BUTTON_1: u32 = 0x0002;
pub const WPAD_BUTTON_B: u32 = 0x0004;
pub const WPAD_BUTTON_A: u32 = 0x0008;
pub const WPAD_BUTTON_MINUS: u32 = 0x0010;
pub const WPAD_BUTTON_HOME: u32 = 0x0080;
pub const WPAD_BUTTON_LEFT: u32 = 0x0100;
pub const WPAD_BUTTON_RIGHT: u32 = 0x0200;
pub const WPAD_BUTTON_DOWN: u32 = 0x0400;
pub const WPAD_BUTTON_UP: u32 = 0x0800;
pub const WPAD_BUTTON_PLUS: u32 = 0x1000;
pub const WPAD_NUNCHUK_BUTTON_Z: u32 = 0x0001 << 16;
pub const WPAD_NUNCHUK_BUTTON_C: u32 = 0x0002 << 16;

#[repr(C)]
pub struct IrDot {
    pub visible: u8,
    pub rx: i16,
    pub ry: i16,
    pub size: u8,
}

/// wiiuse's `ir_t`, the pointer position is `x` and `y` in the `WPAD_SetVRes` resolution.
#[repr(C)]
pub struct IrData {
    pub dots: [IrDot; 4],
    pub num_dots: u8,
    pub state: c_int,
    pub raw_valid: c_int,
    pub sensorbar: [f32; 15],
    pub ax: f32,
    pub ay: f32,
    pub distance: f32,
    pub z: f32,
    pub angle: f32,
    pub smooth_valid: c_int,
    pub sx: f32,
    pub sy: f32,
    pub error_cnt: f32,
    pub glitch_cnt: f32,
    pub valid: c_int,
    pub x: f32,
    pub y: f32,
    pub aspect: c_int,
    pub pos: c_int,
    pub vres: [u32; 2],
    pub offset: [c_int; 2],
}

/// wiiuse's `joystick_t`, the angle is in degrees clockwise from up.
#[repr(C)]
pub struct Joystick {
    pub max: [u8; 2],
    pub min: [u8; 2],
    pub center: [u8; 2],
    pub position: [u8; 2],
    pub ang: f32,
    pub mag: f32,
}

/// wiiuse's `expansion_t` up to the Nunchuk's joystick, which follows its accelerometer
/// calibration. The union of all expansions is smaller than the padding.
#[repr(C)]
pub struct Expansion {
    pub kind: c_int,
    pub accel_calibration: [u16; 6],
    pub accel_smoothing: [f32; 3],
    pub joystick: Joystick,
    _rest: [u32; 128],
}

unsafe extern "C" {
    pub fn WPAD_Init() -> c_int;
    pub fn WPAD_ScanPads() -> c_int;
    pub fn WPAD_Shutdown();
    pub fn WPAD_Probe(channel: c_int, kind: *mut u32) -> i32;
    pub fn WPAD_Disconnect(channel: c_int) -> i32;
    pub fn WPAD_Rumble(channel: c_int, status: c_int) -> i32;
    pub fn WPAD_BatteryLevel(channel: c_int) -> u8;
    pub fn WPAD_ButtonsUp(channel: c_int) -> u32;
    pub fn WPAD_SetDataFormat(channel: c_int, format: c_int) -> c_int;
    pub fn WPAD_SetVRes(channel: c_int, width: u32, height: u32) -> c_int;
    pub fn WPAD_ButtonsDown(channel: c_int) -> u32;
    pub fn WPAD_ButtonsHeld(channel: c_int) -> u32;
    pub fn WPAD_Expansion(channel: c_int, expansion: *mut Expansion);
    pub fn WPAD_IR(channel: c_int, ir: *mut IrData);
}
