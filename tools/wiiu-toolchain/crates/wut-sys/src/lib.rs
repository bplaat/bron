/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Rust bindings for the parts of wut, libwhb, libgfd, libogc's matrix math and picolibc that the
//! Wii U programs use, the toolchain's `build.rs` helper builds and links the libraries. The names and values come
//! from wut's headers; constants use the parameter's integer type.

#![no_std]
#![allow(non_snake_case, non_camel_case_types, missing_docs)]
#![allow(unsafe_code)]

use core::ffi::{CStr, c_char, c_int, c_void};

mod audio;
mod input;
mod network;
mod thread;
mod usb;

pub use audio::*;
pub use input::*;
pub use network::*;
pub use thread::*;
pub use usb::*;

pub type BOOL = i32;
pub const TRUE: BOOL = 1;
pub const FALSE: BOOL = 0;

// MARK: Time
pub type OSTime = i64;

#[repr(C)]
pub struct OSSystemInfo {
    pub bus_clock_speed: u32,
    pub core_clock_speed: u32,
    pub base_time: i64,
    _unknown: [u8; 0x10],
}

unsafe extern "C" {
    pub fn OSGetTime() -> OSTime;
    pub fn OSGetSystemTime() -> OSTime;
    pub fn OSGetSystemInfo() -> *mut OSSystemInfo;
}

/// The time base ticks at a quarter of the bus clock, like wut's `OSTimerClockSpeed`.
pub fn OSTimerClockSpeed() -> u32 {
    // SAFETY: Cafe OS owns the system information and returns a valid static pointer.
    unsafe { (*OSGetSystemInfo()).bus_clock_speed / 4 }
}

// MARK: GX2 shaders
pub const GX2_ATTRIB_FORMAT_UNORM_8_8: u32 = 0x004;
pub const GX2_ATTRIB_FORMAT_UNORM_8_8_8_8: u32 = 0x00a;
pub const GX2_ATTRIB_FORMAT_UINT_8_8_8_8: u32 = 0x10a;
pub const GX2_ATTRIB_FORMAT_FLOAT_32_32: u32 = 0x80d;
pub const GX2_ATTRIB_FORMAT_FLOAT_32_32_32: u32 = 0x811;
pub const GX2_ATTRIB_FORMAT_FLOAT_32_32_32_32: u32 = 0x813;

#[repr(C)]
pub struct GX2FetchShader {
    pub r#type: u32,
    pub sq_pgm_resources_fs: u32,
    pub size: u32,
    pub program: *mut c_void,
    pub attrib_count: u32,
    pub num_divisors: u32,
    pub divisors: [u32; 2],
}

#[repr(C)]
pub struct GX2AttribStream {
    pub location: u32,
    pub buffer: u32,
    pub offset: u32,
    pub format: u32,
    pub r#type: u32,
    pub alu_divisor: u32,
    pub mask: u32,
    pub endian_swap: u32,
}

#[repr(C)]
pub struct GX2UniformVar {
    pub name: *const c_char,
    pub r#type: u32,
    pub count: u32,
    pub offset: u32,
    pub block: i32,
}

/// The start of a vertex shader, up to its uniform variables
#[repr(C)]
pub struct GX2VertexShader {
    _registers: [u8; 0xe4],
    pub uniform_var_count: u32,
    pub uniform_vars: *const GX2UniformVar,
}

/// The start of a pixel shader, up to its uniform variables
#[repr(C)]
pub struct GX2PixelShader {
    _registers: [u8; 0xb8],
    pub uniform_var_count: u32,
    pub uniform_vars: *const GX2UniformVar,
}

unsafe fn find_uniform_var(
    count: u32,
    vars: *const GX2UniformVar,
    name: &CStr,
) -> Option<&'static GX2UniformVar> {
    // SAFETY: the caller supplies a loaded shader's uniform array and names.
    unsafe {
        (0..count as usize)
            .map(|index| &*vars.add(index))
            .find(|var| CStr::from_ptr(var.name) == name)
    }
}

/// Finds a uniform variable of a vertex shader by name, like wut's static inline function.
///
/// # Safety
/// The shader must point to a loaded vertex shader.
pub unsafe fn GX2GetVertexUniformVar(
    shader: *const GX2VertexShader,
    name: &CStr,
) -> Option<&'static GX2UniformVar> {
    // SAFETY: the caller guarantees shader points to a loaded vertex shader.
    unsafe { find_uniform_var((*shader).uniform_var_count, (*shader).uniform_vars, name) }
}

/// Finds a uniform variable of a pixel shader by name, like wut's static inline function.
///
/// # Safety
/// The shader must point to a loaded pixel shader.
pub unsafe fn GX2GetPixelUniformVar(
    shader: *const GX2PixelShader,
    name: &CStr,
) -> Option<&'static GX2UniformVar> {
    // SAFETY: the caller guarantees shader points to a loaded pixel shader.
    unsafe { find_uniform_var((*shader).uniform_var_count, (*shader).uniform_vars, name) }
}

unsafe extern "C" {
    pub fn GX2SetFetchShader(shader: *const GX2FetchShader);
    pub fn GX2SetVertexShader(shader: *const GX2VertexShader);
    pub fn GX2SetPixelShader(shader: *const GX2PixelShader);
    pub fn GX2SetVertexUniformReg(offset: u32, count: u32, data: *const c_void);
    pub fn GX2SetPixelUniformReg(offset: u32, count: u32, data: *const c_void);
}

// MARK: GX2 drawing and state
pub const GX2_VERTEX_BUFFER_ALIGNMENT: usize = 0x40;
pub const GX2_INDEX_BUFFER_ALIGNMENT: usize = 0x20;
pub const GX2_PRIMITIVE_MODE_LINES: u32 = 2;
pub const GX2_PRIMITIVE_MODE_TRIANGLES: u32 = 4;
pub const GX2_PRIMITIVE_MODE_QUADS: u32 = 19;
pub const GX2_INDEX_TYPE_U16: u32 = 4;
pub const GX2_INDEX_TYPE_U32: u32 = 9;
pub const GX2_INVALIDATE_MODE_CPU_ATTRIBUTE_BUFFER: u32 = 1 << 6 | 1 << 0;
pub const GX2_INVALIDATE_MODE_CPU_TEXTURE: u32 = 1 << 6 | 1 << 1;
pub const GX2_COMPARE_FUNC_LESS: u32 = 1;
pub const GX2_COMPARE_FUNC_LEQUAL: u32 = 3;
pub const GX2_COMPARE_FUNC_GREATER: u32 = 4;
pub const GX2_COMPARE_FUNC_ALWAYS: u32 = 7;
pub const GX2_FRONT_FACE_CCW: u32 = 0;
pub const GX2_FRONT_FACE_CW: u32 = 1;
pub const GX2_BLEND_MODE_ZERO: u32 = 0;
pub const GX2_BLEND_MODE_ONE: u32 = 1;
pub const GX2_BLEND_MODE_SRC_ALPHA: u32 = 4;
pub const GX2_BLEND_MODE_INV_SRC_ALPHA: u32 = 5;
pub const GX2_BLEND_COMBINE_MODE_ADD: u32 = 0;
pub const GX2_RENDER_TARGET_0: u32 = 0;
pub const GX2_LOGIC_OP_COPY: u32 = 0xcc;
pub const GX2_SCAN_TARGET_TV: u32 = 1 << 0;
pub const GX2_SCAN_TARGET_DRC: u32 = 1 << 2;

/// A render target, including its surface and cached registers.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GX2ColorBuffer {
    pub surface: GX2Surface,
    pub view_mip: u32,
    pub view_first_slice: u32,
    pub view_num_slices: u32,
    pub aa_buffer: *mut c_void,
    pub aa_size: u32,
    pub regs: [u32; 5],
}

#[cfg(target_pointer_width = "32")]
const _: () = {
    assert!(size_of::<GX2ColorBuffer>() == 0x9c);
    assert!(core::mem::offset_of!(GX2ColorBuffer, regs) == 0x88);
};

unsafe extern "C" {
    pub fn GX2InitColorBufferRegs(buffer: *mut GX2ColorBuffer);
    pub fn GX2SetColorBuffer(buffer: *const GX2ColorBuffer, target: u32);
    pub fn GX2Invalidate(mode: u32, buffer: *mut c_void, size: u32);
    pub fn GX2SetAttribBuffer(index: u32, size: u32, stride: u32, buffer: *const c_void);
    pub fn GX2DrawEx(mode: u32, count: u32, offset: u32, num_instances: u32);
    pub fn GX2DrawIndexedEx(
        mode: u32,
        count: u32,
        index_type: u32,
        indices: *const c_void,
        offset: u32,
        num_instances: u32,
    );
    pub fn GX2SetDepthOnlyControl(depth_test: BOOL, depth_write: BOOL, depth_compare: u32);
    pub fn GX2SetCullOnlyControl(front_face: u32, cull_front: BOOL, cull_back: BOOL);
    pub fn GX2SetColorControl(
        rop3: u32,
        target_blend_enable: u8,
        multi_write_enable: BOOL,
        color_write_enable: BOOL,
    );
    pub fn GX2SetBlendControl(
        target: u32,
        color_src_blend: u32,
        color_dst_blend: u32,
        color_combine: u32,
        use_alpha_blend: BOOL,
        alpha_src_blend: u32,
        alpha_dst_blend: u32,
        alpha_combine: u32,
    );
    pub fn GX2SetAlphaTest(alpha_test: BOOL, func: u32, reference: f32);
    pub fn GX2SetLineWidth(width: f32);
    pub fn GX2SetViewport(x: f32, y: f32, width: f32, height: f32, near: f32, far: f32);
    pub fn GX2SetScissor(x: u32, y: u32, width: u32, height: u32);
    /// Copies a color buffer to the screen's scan buffer, scaled to the screen's size.
    pub fn GX2CopyColorBufferToScanBuffer(buffer: *const GX2ColorBuffer, scan_target: u32);
}

// MARK: GX2 textures
pub const GX2_SURFACE_FORMAT_UNORM_R8_G8_B8_A8: u32 = 0x1a;
pub const GX2_SURFACE_FORMAT_SRGB_R8_G8_B8_A8: u32 = 0x41a;
pub const GX2_SURFACE_FORMAT_SRGB_BC1: u32 = 0x431;
pub const GX2_TEX_CLAMP_MODE_WRAP: u32 = 0;
pub const GX2_TEX_CLAMP_MODE_CLAMP: u32 = 2;
pub const GX2_TEX_XY_FILTER_MODE_POINT: u32 = 0;
pub const GX2_TEX_XY_FILTER_MODE_LINEAR: u32 = 1;
pub const GX2_TEX_ANISO_RATIO_NONE: u32 = 0;
pub const GX2_TEX_Z_FILTER_MODE_POINT: u32 = 1;
pub const GX2_TEX_MIP_FILTER_MODE_LINEAR: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct GX2Surface {
    pub dim: u32,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub mip_levels: u32,
    pub format: u32,
    pub aa: u32,
    pub r#use: u32,
    pub image_size: u32,
    pub image: *mut c_void,
    pub mipmap_size: u32,
    pub mipmaps: *mut c_void,
    pub tile_mode: u32,
    pub swizzle: u32,
    pub alignment: u32,
    pub pitch: u32,
    pub mip_level_offset: [u32; 13],
}

/// Create with `core::mem::zeroed()` and load with `GFDGetTextureInPlace`
#[repr(C)]
pub struct GX2Texture {
    pub surface: GX2Surface,
    pub view_first_mip: u32,
    pub view_num_mips: u32,
    pub view_first_slice: u32,
    pub view_num_slices: u32,
    pub comp_map: u32,
    pub regs: [u32; 5],
}

#[repr(C)]
#[derive(Default)]
pub struct GX2Sampler {
    pub regs: [u32; 3],
}

unsafe extern "C" {
    pub fn GX2InitSampler(sampler: *mut GX2Sampler, clamp_mode: u32, min_mag_filter_mode: u32);
    pub fn GX2InitTextureRegs(texture: *mut GX2Texture);
    pub fn GX2InitSamplerXYFilter(
        sampler: *mut GX2Sampler,
        mag_filter_mode: u32,
        min_filter_mode: u32,
        max_aniso: u32,
    );
    pub fn GX2InitSamplerZMFilter(
        sampler: *mut GX2Sampler,
        z_filter_mode: u32,
        mip_filter_mode: u32,
    );
    pub fn GX2SetPixelTexture(texture: *const GX2Texture, unit: u32);
    pub fn GX2SetPixelSampler(sampler: *const GX2Sampler, unit: u32);
}

// MARK: libgfd
unsafe extern "C" {
    /// Points a texture at its image and mipmaps in a `.gtx` file, the GPU reads them in place.
    /// The file must stay loaded and be aligned like the texture data in it.
    pub fn GFDGetTextureInPlace(texture: *mut GX2Texture, index: u32, file: *const c_void) -> BOOL;
}

// MARK: libwhb
#[repr(C)]
pub struct WHBGfxShaderGroup {
    pub fetch_shader: GX2FetchShader,
    pub fetch_shader_program: *mut c_void,
    pub pixel_shader: *mut GX2PixelShader,
    pub vertex_shader: *mut GX2VertexShader,
    pub num_attributes: u32,
    pub attributes: [GX2AttribStream; 16],
}

unsafe extern "C" {
    pub fn WHBProcInit();
    pub fn WHBProcShutdown();
    pub fn WHBProcIsRunning() -> BOOL;
    pub fn WHBGfxInit() -> BOOL;
    pub fn WHBGfxShutdown();
    pub fn WHBGfxBeginRender();
    pub fn WHBGfxFinishRender();
    pub fn WHBGfxBeginRenderDRC();
    pub fn WHBGfxGetDRCColourBuffer() -> *mut GX2ColorBuffer;
    pub fn WHBGfxBeginRenderTV();
    pub fn WHBGfxFinishRenderTV();
    pub fn WHBGfxGetTVColourBuffer() -> *mut GX2ColorBuffer;
    pub fn WHBGfxClearColor(r: f32, g: f32, b: f32, a: f32);
    pub fn WHBGfxLoadGFDShaderGroup(
        group: *mut WHBGfxShaderGroup,
        index: u32,
        file: *const c_void,
    ) -> BOOL;
    pub fn WHBGfxInitShaderAttribute(
        group: *mut WHBGfxShaderGroup,
        name: *const c_char,
        buffer: u32,
        offset: u32,
        format: u32,
    ) -> BOOL;
    pub fn WHBGfxInitFetchShader(group: *mut WHBGfxShaderGroup) -> BOOL;
    pub fn WHBGfxFreeShaderGroup(group: *mut WHBGfxShaderGroup) -> BOOL;
}

// MARK: sysapp
unsafe extern "C" {
    /// Asks the system to go back to the Wii U Menu, `WHBProcIsRunning` turns false once the
    /// program may exit.
    pub fn SYSLaunchMenu();
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

// MARK: picolibc
unsafe extern "C" {
    /// Fill at most 256 bytes using the platform entropy source; returns 0 or -1 with errno set.
    pub fn getentropy(buffer: *mut c_void, length: usize) -> c_int;
    pub fn malloc(size: usize) -> *mut c_void;
    pub fn memalign(alignment: usize, size: usize) -> *mut c_void;
    pub fn free(pointer: *mut c_void);
    pub fn printf(format: *const c_char, ...) -> c_int;
    pub safe fn floorf(x: f32) -> f32;
    pub safe fn powf(x: f32, y: f32) -> f32;
    pub safe fn sinf(x: f32) -> f32;
    pub safe fn cosf(x: f32) -> f32;
    pub safe fn atan2f(y: f32, x: f32) -> f32;
    pub safe fn tanf(x: f32) -> f32;
    pub safe fn fabsf(x: f32) -> f32;
    pub safe fn sqrtf(x: f32) -> f32;
}
