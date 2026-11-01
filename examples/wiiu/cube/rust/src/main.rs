/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The rotating textured cube scene from the WebGPU example, on the TV and the GamePad.

#![no_std]
#![no_main]

use core::ffi::{c_int, c_void};
use core::mem::{self, MaybeUninit};

use wut_sys::*;

#[repr(C)]
struct Vertex {
    position: [f32; 3],
    tex_coord: [f32; 2],
}

const fn vertex(position: [f32; 3], tex_coord: [f32; 2]) -> Vertex {
    Vertex {
        position,
        tex_coord,
    }
}

// Four corners for each face, so every face gets the whole texture. The GPU reads the vertices
// straight from memory, which needs 64 byte alignment.
#[repr(C, align(64))]
struct Vertices([Vertex; 24]);

#[rustfmt::skip]
static mut VERTICES: Vertices = Vertices([
    vertex([-1.0, -1.0, 1.0], [0.0, 1.0]), vertex([1.0, -1.0, 1.0], [1.0, 1.0]),
    vertex([1.0, 1.0, 1.0], [1.0, 0.0]), vertex([-1.0, 1.0, 1.0], [0.0, 0.0]),
    vertex([1.0, -1.0, -1.0], [0.0, 1.0]), vertex([-1.0, -1.0, -1.0], [1.0, 1.0]),
    vertex([-1.0, 1.0, -1.0], [1.0, 0.0]), vertex([1.0, 1.0, -1.0], [0.0, 0.0]),
    vertex([1.0, -1.0, 1.0], [0.0, 1.0]), vertex([1.0, -1.0, -1.0], [1.0, 1.0]),
    vertex([1.0, 1.0, -1.0], [1.0, 0.0]), vertex([1.0, 1.0, 1.0], [0.0, 0.0]),
    vertex([-1.0, -1.0, -1.0], [0.0, 1.0]), vertex([-1.0, -1.0, 1.0], [1.0, 1.0]),
    vertex([-1.0, 1.0, 1.0], [1.0, 0.0]), vertex([-1.0, 1.0, -1.0], [0.0, 0.0]),
    vertex([-1.0, 1.0, 1.0], [0.0, 1.0]), vertex([1.0, 1.0, 1.0], [1.0, 1.0]),
    vertex([1.0, 1.0, -1.0], [1.0, 0.0]), vertex([-1.0, 1.0, -1.0], [0.0, 0.0]),
    vertex([-1.0, -1.0, -1.0], [0.0, 1.0]), vertex([1.0, -1.0, -1.0], [1.0, 1.0]),
    vertex([1.0, -1.0, 1.0], [1.0, 0.0]), vertex([-1.0, -1.0, 1.0], [0.0, 0.0]),
]);

const NORMALS: [[f32; 3]; 6] = [
    [0.0, 0.0, 1.0],
    [0.0, 0.0, -1.0],
    [1.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, -1.0, 0.0],
];

#[repr(C, align(64))]
struct Shader<T: ?Sized>(T);

static SHADER: &Shader<[u8]> = &Shader(*include_bytes!(concat!(env!("OUT_DIR"), "/shader.gsh")));

// The GPU reads the BC1 textures in place, which needs the 4 KiB alignment of their tiles
#[repr(C, align(4096))]
struct Textures<T: ?Sized>(T);

static TEXTURES: &Textures<[u8]> =
    &Textures(*include_bytes!(concat!(env!("OUT_DIR"), "/textures.gtx")));

struct Cube {
    group: WHBGfxShaderGroup,
    projection_offset: u32,
    model_view_offset: u32,
    light_offset: u32,
    texture: GX2Texture,
    sampler: GX2Sampler,
}

impl Cube {
    unsafe fn new() -> Option<Self> {
        unsafe {
            let mut group = MaybeUninit::<WHBGfxShaderGroup>::zeroed().assume_init();
            if WHBGfxLoadGFDShaderGroup(&mut group, 0, SHADER.0.as_ptr().cast()) == FALSE {
                return None;
            }
            WHBGfxInitShaderAttribute(
                &mut group,
                c"aPosition".as_ptr(),
                0,
                0,
                GX2_ATTRIB_FORMAT_FLOAT_32_32_32,
            );
            WHBGfxInitShaderAttribute(
                &mut group,
                c"aTexCoord".as_ptr(),
                0,
                12,
                GX2_ATTRIB_FORMAT_FLOAT_32_32,
            );
            WHBGfxInitFetchShader(&mut group);
            let projection_offset =
                GX2GetVertexUniformVar(group.vertex_shader, c"uProjection")?.offset;
            let model_view_offset =
                GX2GetVertexUniformVar(group.vertex_shader, c"uModelView")?.offset;
            let light_offset = GX2GetPixelUniformVar(group.pixel_shader, c"uLight")?.offset;
            GX2Invalidate(
                GX2_INVALIDATE_MODE_CPU_ATTRIBUTE_BUFFER,
                (&raw mut VERTICES).cast(),
                mem::size_of::<Vertices>() as u32,
            );
            let mut texture = MaybeUninit::<GX2Texture>::zeroed().assume_init();
            if GFDGetTextureInPlace(&mut texture, 0, TEXTURES.0.as_ptr().cast()) == FALSE {
                return None;
            }
            let mut sampler = GX2Sampler::default();
            GX2InitSampler(
                &mut sampler,
                GX2_TEX_CLAMP_MODE_WRAP,
                GX2_TEX_XY_FILTER_MODE_LINEAR,
            );
            GX2InitSamplerZMFilter(
                &mut sampler,
                GX2_TEX_Z_FILTER_MODE_POINT,
                GX2_TEX_MIP_FILTER_MODE_LINEAR,
            );
            Some(Self {
                group,
                projection_offset,
                model_view_offset,
                light_offset,
                texture,
                sampler,
            })
        }
    }

    unsafe fn draw(&mut self, projection: &Mtx44, matrix: &Mtx) {
        unsafe {
            WHBGfxClearColor(0.0, 0.0, 0.0, 1.0);
            GX2SetFetchShader(&self.group.fetch_shader);
            GX2SetVertexShader(self.group.vertex_shader);
            GX2SetPixelShader(self.group.pixel_shader);
            GX2SetDepthOnlyControl(TRUE, TRUE, GX2_COMPARE_FUNC_LEQUAL);
            GX2SetVertexUniformReg(self.projection_offset, 16, projection.as_ptr().cast());
            GX2SetVertexUniformReg(self.model_view_offset, 12, matrix.as_ptr().cast());
            GX2SetPixelTexture(&self.texture, 0);
            GX2SetPixelSampler(&self.sampler, 0);
            GX2SetAttribBuffer(
                0,
                mem::size_of::<Vertices>() as u32,
                mem::size_of::<Vertex>() as u32,
                (&raw const VERTICES).cast::<c_void>(),
            );
            for (face, normal) in NORMALS.iter().enumerate() {
                // Like the WebGPU shader: the rotated normal against a fixed light, lit from
                // both sides
                let light = [0.4, 0.8, 0.6];
                let dot: f32 = (0..3)
                    .flat_map(|row| (0..3).map(move |column| (row, column)))
                    .map(|(row, column)| matrix[row][column] * normal[column] * light[row])
                    .sum();
                let shade = 0.35 + 0.65 * fabsf(dot) / sqrtf(1.16);
                let color = [shade, shade, shade, 1.0];
                GX2SetPixelUniformReg(self.light_offset, 4, color.as_ptr().cast());
                GX2DrawEx(GX2_PRIMITIVE_MODE_QUADS, 4, face as u32 * 4, 1);
            }
        }
    }
}

impl Drop for Cube {
    fn drop(&mut self) {
        unsafe { WHBGfxFreeShaderGroup(&mut self.group) };
    }
}

/// The same model and camera rotations as the WebGPU cube shader
fn cube_matrix(elapsed: f32) -> Mtx {
    let mut matrix = [[0.0; 4]; 3];
    let mut temp = [[0.0; 4]; 3];
    unsafe {
        ps_guMtxRotRad(&mut matrix, b'x' as _, elapsed * 0.55);
        for (axis, angle) in [(b'z', elapsed * 0.35), (b'y', 0.4), (b'x', 0.25)] {
            ps_guMtxRotRad(&mut temp, axis as _, angle);
            ps_guMtxConcat(&temp, &matrix, &mut matrix);
        }
    }
    matrix
}

#[unsafe(export_name = "main")]
pub extern "C" fn main() -> c_int {
    unsafe {
        WHBProcInit();
        WHBGfxInit();
        let Some(mut cube) = Cube::new() else {
            return 1;
        };

        // The TV renders at 1280x720 and the GamePad at 854x480, both about 16:9
        let mut projection = [[0.0; 4]; 4];
        guPerspective(&mut projection, 45.0, 16.0 / 9.0, 0.1, 100.0);
        let started = OSGetTime();
        let ticks_per_second = OSTimerClockSpeed() as f32;
        while WHBProcIsRunning() != FALSE {
            let mut matrix = cube_matrix((OSGetTime() - started) as f32 / ticks_per_second);
            matrix[2][3] = -4.5;

            WHBGfxBeginRender();
            WHBGfxBeginRenderTV();
            cube.draw(&projection, &matrix);
            WHBGfxFinishRenderTV();
            WHBGfxBeginRenderDRC();
            cube.draw(&projection, &matrix);
            WHBGfxFinishRenderDRC();
            WHBGfxFinishRender();
        }

        drop(cube);
        WHBGfxShutdown();
        WHBProcShutdown();
    }
    0
}
