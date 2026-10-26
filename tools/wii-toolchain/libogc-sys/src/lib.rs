/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Rust bindings for the parts of libogc and picolibc that the Wii programs use, the
//! toolchain's `build.rs` helper builds and links the libraries. The names and values come
//! from libogc's headers, the C macros get the type of the parameter that usually takes them.

#![no_std]
#![allow(non_snake_case, non_camel_case_types)]

use core::ffi::{c_char, c_int, c_void};

// MARK: System and video
pub const SYS_BASE_CACHED: u32 = 0x8000_0000;
pub const SYS_BASE_UNCACHED: u32 = 0xc000_0000;
pub const VI_NON_INTERLACE: u32 = 1;
pub const CONF_ASPECT_16_9: i32 = 1;

#[repr(C)]
pub struct GXRModeObj {
    pub vi_tv_mode: u32,
    pub fb_width: u16,
    pub efb_height: u16,
    pub xfb_height: u16,
    pub vi_x_origin: u16,
    pub vi_y_origin: u16,
    pub vi_width: u16,
    pub vi_height: u16,
    pub xfb_mode: u32,
    pub rgb: u8,
    pub field_rendering: u8,
    pub aa: u8,
    pub sample_pattern: [[u8; 2]; 12],
    pub vfilter: [u8; 7],
}

pub type PowerCallback = Option<extern "C" fn()>;

unsafe extern "C" {
    pub fn VIDEO_Init();
    pub fn VIDEO_SetBlack(black: bool);
    pub fn VIDEO_GetPreferredMode(mode: *mut GXRModeObj) -> *mut GXRModeObj;
    pub fn VIDEO_Configure(mode: *const GXRModeObj);
    pub fn VIDEO_SetNextFramebuffer(framebuffer: *mut c_void);
    pub fn VIDEO_Flush();
    pub fn VIDEO_WaitVSync();
    pub fn SYS_AllocateFramebuffer(mode: *const GXRModeObj) -> *mut c_void;
    pub fn SYS_SetPowerCallback(callback: PowerCallback) -> PowerCallback;
    pub fn CONF_GetAspectRatio() -> c_int;
    pub fn DCFlushRange(pointer: *mut c_void, size: u32);
    pub fn DCInvalidateRange(pointer: *mut c_void, size: u32);
    pub fn gettime() -> u64;
}

// MARK: GX
pub const GX_TRUE: u8 = 1;
pub const GX_FALSE: u8 = 0;
pub const GX_ENABLE: u8 = 1;
pub const GX_DISABLE: u8 = 0;
pub const GX_QUADS: u8 = 0x80;
pub const GX_TRIANGLES: u8 = 0x90;
pub const GX_LINES: u8 = 0xa8;
pub const GX_VTXFMT0: u8 = 0;
pub const GX_VTXFMT1: u8 = 1;
pub const GX_VTXFMT2: u8 = 2;
pub const GX_VA_POS: u8 = 9;
pub const GX_VA_CLR0: u8 = 11;
pub const GX_VA_TEX0: u8 = 13;
pub const GX_NONE: u8 = 0;
pub const GX_DIRECT: u8 = 1;
pub const GX_INDEX8: u8 = 2;
pub const GX_INDEX16: u8 = 3;
pub const GX_POS_XY: u32 = 0;
pub const GX_POS_XYZ: u32 = 1;
pub const GX_U8: u32 = 0;
pub const GX_S8: u32 = 1;
pub const GX_U16: u32 = 2;
pub const GX_S16: u32 = 3;
pub const GX_F32: u32 = 4;
pub const GX_CLR_RGBA: u32 = 1;
pub const GX_RGBA8: u32 = 5;
pub const GX_TEX_ST: u32 = 1;
pub const GX_TF_RGB5A3: u8 = 5;
pub const GX_TF_RGBA8: u8 = 6;
pub const GX_TF_CMPR: u8 = 14;
pub const GX_CLAMP: u8 = 0;
pub const GX_REPEAT: u8 = 1;
pub const GX_NEAR: u8 = 0;
pub const GX_LINEAR: u8 = 1;
pub const GX_LIN_MIP_LIN: u8 = 5;
pub const GX_ANISO_1: u8 = 0;
pub const GX_ANISO_2: u8 = 1;
pub const GX_ANISO_4: u8 = 2;
pub const GX_TEXCOORD0: u16 = 0;
pub const GX_TEXCOORDNULL: u8 = 0xff;
pub const GX_TEXMAP0: u32 = 0;
pub const GX_TEXMAP_NULL: u32 = 0xff;
pub const GX_TG_MTX2X4: u32 = 1;
pub const GX_TG_TEX0: u32 = 4;
pub const GX_IDENTITY: u32 = 60;
pub const GX_TEVSTAGE0: u8 = 0;
pub const GX_COLOR0A0: u8 = 4;
pub const GX_SRC_VTX: u8 = 1;
pub const GX_DF_NONE: u8 = 0;
pub const GX_AF_NONE: u8 = 2;
pub const GX_MODULATE: u8 = 0;
pub const GX_REPLACE: u8 = 3;
pub const GX_PASSCLR: u8 = 4;
pub const GX_PERSPECTIVE: u8 = 0;
pub const GX_ORTHOGRAPHIC: u8 = 1;
pub const GX_PNMTX0: u32 = 0;
pub const GX_CULL_NONE: u8 = 0;
pub const GX_CULL_FRONT: u8 = 1;
pub const GX_CULL_BACK: u8 = 2;
pub const GX_FOG_NONE: u8 = 0;
pub const GX_FOG_LIN: u8 = 2;
pub const GX_NEVER: u8 = 0;
pub const GX_LEQUAL: u8 = 3;
pub const GX_GREATER: u8 = 4;
pub const GX_ALWAYS: u8 = 7;
pub const GX_AOP_AND: u8 = 0;
pub const GX_ZC_LINEAR: u8 = 0;
pub const GX_PF_RGB8_Z24: u8 = 0;
pub const GX_PF_RGB565_Z16: u8 = 2;
pub const GX_GM_1_0: u8 = 0;
pub const GX_BM_NONE: u8 = 0;
pub const GX_BM_BLEND: u8 = 1;
pub const GX_BL_ZERO: u8 = 0;
pub const GX_BL_ONE: u8 = 1;
pub const GX_BL_SRCALPHA: u8 = 4;
pub const GX_BL_INVSRCALPHA: u8 = 5;
pub const GX_LO_CLEAR: u8 = 0;
pub const GX_TO_ZERO: u8 = 0;
pub const GX_MAX_Z24: u32 = 0x00ff_ffff;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct GXColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct GXTexObj {
    pub val: [u32; 8],
}

#[repr(C)]
pub struct GXFifoObj {
    _private: [u8; 0],
}

unsafe extern "C" {
    /// The write-gather pipe, vertex data between `GX_Begin` calls is written to it.
    pub static wgPipe: *mut c_void;

    // Setup and display output
    pub fn GX_Init(fifo: *mut c_void, size: u32) -> *mut GXFifoObj;
    pub fn GX_SetViewport(x: f32, y: f32, width: f32, height: f32, near: f32, far: f32);
    pub fn GX_SetScissor(x: u32, y: u32, width: u32, height: u32);
    pub fn GX_GetYScaleFactor(efb_height: u16, xfb_height: u16) -> f32;
    pub fn GX_SetDispCopyYScale(scale: f32) -> u32;
    pub fn GX_SetDispCopySrc(x: u16, y: u16, width: u16, height: u16);
    pub fn GX_SetDispCopyDst(width: u16, height: u16);
    pub fn GX_SetCopyFilter(aa: u8, sample_pattern: *const [u8; 2], vf: u8, vfilter: *const u8);
    pub fn GX_SetFieldMode(field_mode: u8, half_aspect_ratio: u8);
    pub fn GX_SetPixelFmt(pixel_format: u8, depth_compression: u8);
    pub fn GX_SetDispCopyGamma(gamma: u8);
    pub fn GX_SetCopyClear(color: GXColor, depth: u32);
    pub fn GX_CopyDisp(destination: *mut c_void, clear: u8);
    pub fn GX_InvVtxCache();
    pub fn GX_InvalidateTexAll();
    pub fn GX_Flush();
    pub fn GX_DrawDone();

    // Vertex, pixel and texture environment state
    pub fn GX_SetZMode(enable: u8, function: u8, update_enable: u8);
    pub fn GX_SetZCompLoc(before_texture: u8);
    pub fn GX_SetColorUpdate(enable: u8);
    pub fn GX_SetAlphaCompare(comp0: u8, ref0: u8, op: u8, comp1: u8, ref1: u8);
    pub fn GX_SetBlendMode(kind: u8, src: u8, dst: u8, op: u8);
    pub fn GX_SetCullMode(mode: u8);
    pub fn GX_SetFog(kind: u8, start: f32, end: f32, near: f32, far: f32, color: GXColor);
    pub fn GX_SetLineWidth(width: u8, format: u8);
    pub fn GX_ClearVtxDesc();
    pub fn GX_SetVtxDesc(attribute: u8, mode: u8);
    pub fn GX_SetVtxAttrFmt(
        format: u8,
        attribute: u32,
        component_type: u32,
        component_size: u32,
        fractional_bits: u32,
    );
    pub fn GX_SetArray(attribute: u32, pointer: *mut c_void, stride: u8);
    pub fn GX_SetNumChans(count: u8);
    pub fn GX_SetChanCtrl(
        channel: i32,
        enable: u8,
        ambient_source: u8,
        material_source: u8,
        light_mask: u8,
        diffuse_function: u8,
        attenuation_function: u8,
    );
    pub fn GX_SetNumTexGens(count: u32);
    pub fn GX_SetNumTevStages(count: u8);
    pub fn GX_SetTexCoordGen(coord: u16, kind: u32, source: u32, matrix: u32);
    pub fn GX_SetTevOrder(stage: u8, coord: u8, map: u32, color: u8);
    pub fn GX_SetTevOp(stage: u8, mode: u8);

    // Matrices, textures and drawing
    pub fn GX_LoadProjectionMtx(matrix: *const Mtx44, projection_type: u8);
    pub fn GX_LoadPosMtxImm(matrix: *const Mtx, index: u32);
    pub fn GX_SetCurrentMtx(matrix: u32);
    pub fn GX_InitTexObj(
        obj: *mut GXTexObj,
        data: *mut c_void,
        width: u16,
        height: u16,
        format: u8,
        wrap_s: u8,
        wrap_t: u8,
        mipmap: u8,
    );
    pub fn GX_InitTexObjLOD(
        obj: *mut GXTexObj,
        min_filter: u8,
        mag_filter: u8,
        min_lod: f32,
        max_lod: f32,
        lod_bias: f32,
        bias_clamp: u8,
        edge_lod: u8,
        max_aniso: u8,
    );
    pub fn GX_LoadTexObj(obj: *const GXTexObj, map: u8);
    pub fn GX_Begin(primitive: u8, format: u8, vertices: u16);
    pub fn GX_BeginDispList(list: *mut c_void, size: u32);
    pub fn GX_EndDispList() -> u32;
    pub fn GX_CallDispList(list: *const c_void, size: u32);
}

// MARK: GU matrices
pub type Mtx = [[f32; 4]; 3];
pub type Mtx44 = [[f32; 4]; 4];

#[repr(C)]
pub struct GuVector {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

unsafe extern "C" {
    pub fn guPerspective(matrix: *mut Mtx44, fovy: f32, aspect: f32, near: f32, far: f32);
    pub fn guOrtho(
        matrix: *mut Mtx44,
        top: f32,
        bottom: f32,
        left: f32,
        right: f32,
        near: f32,
        far: f32,
    );
    pub fn guLookAt(
        matrix: *mut Mtx,
        camera: *const GuVector,
        up: *const GuVector,
        target: *const GuVector,
    );
    pub fn ps_guMtxIdentity(matrix: *mut Mtx);
    pub fn ps_guMtxRotRad(matrix: *mut Mtx, axis: c_char, radians: f32);
    pub fn ps_guMtxConcat(a: *const Mtx, b: *const Mtx, destination: *mut Mtx);
    pub fn ps_guMtxTransApply(source: *const Mtx, destination: *mut Mtx, x: f32, y: f32, z: f32);
}

// MARK: Threads
pub type lwp_t = u32;
pub type mutex_t = u32;
pub type cond_t = u32;

pub const LWP_THREAD_NULL: lwp_t = 0xffff_ffff;
pub const LWP_MUTEX_NULL: mutex_t = 0xffff_ffff;
pub const LWP_COND_NULL: cond_t = 0xffff_ffff;

unsafe extern "C" {
    pub fn LWP_CreateThread(
        thread: *mut lwp_t,
        entry: extern "C" fn(*mut c_void) -> *mut c_void,
        argument: *mut c_void,
        stack: *mut c_void,
        stack_size: u32,
        priority: u8,
    ) -> i32;
    pub fn LWP_MutexInit(mutex: *mut mutex_t, recursive: bool) -> i32;
    pub fn LWP_MutexLock(mutex: mutex_t) -> i32;
    pub fn LWP_MutexUnlock(mutex: mutex_t) -> i32;
    pub fn LWP_CondInit(cond: *mut cond_t) -> i32;
    pub fn LWP_CondWait(cond: cond_t, mutex: mutex_t) -> i32;
    pub fn LWP_CondSignal(cond: cond_t) -> i32;
}

// MARK: Wii Remote
pub const WPAD_CHAN_ALL: c_int = -1;
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

pub type WpadPowerCallback = Option<extern "C" fn(c_int)>;

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
    pub fn WPAD_SetDataFormat(channel: c_int, format: c_int) -> c_int;
    pub fn WPAD_SetVRes(channel: c_int, width: u32, height: u32) -> c_int;
    pub fn WPAD_ButtonsDown(channel: c_int) -> u32;
    pub fn WPAD_ButtonsHeld(channel: c_int) -> u32;
    pub fn WPAD_Expansion(channel: c_int, expansion: *mut Expansion);
    pub fn WPAD_IR(channel: c_int, ir: *mut IrData);
    pub fn WPAD_Disconnect(channel: c_int) -> c_int;
    pub fn WPAD_SetPowerButtonCallback(callback: WpadPowerCallback);
}

// MARK: C library
unsafe extern "C" {
    pub fn memalign(alignment: usize, size: usize) -> *mut c_void;
    pub fn free(pointer: *mut c_void);
    pub fn puts(text: *const c_char) -> c_int;
    pub safe fn floor(x: f64) -> f64;
    pub safe fn floorf(x: f32) -> f32;
    pub safe fn powf(x: f32, y: f32) -> f32;
    pub safe fn sinf(x: f32) -> f32;
    pub safe fn cosf(x: f32) -> f32;
}
