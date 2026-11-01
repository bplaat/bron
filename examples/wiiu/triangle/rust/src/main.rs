/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal program that draws a rotating triangle on the TV and the GamePad.

#![no_std]
#![no_main]

use core::ffi::{c_int, c_void};
use core::mem::{self, MaybeUninit};

use wut_sys::*;

#[repr(C)]
struct Vertex {
    position: [f32; 3],
    color: [f32; 4],
}

// The GPU reads the vertices straight from memory, which needs 64 byte alignment
#[repr(C, align(64))]
struct Vertices([Vertex; 3]);

static mut VERTICES: Vertices = Vertices([
    Vertex {
        position: [0.0, 1.0, 0.0],
        color: [1.0, 0.0, 0.0, 1.0],
    },
    Vertex {
        position: [-1.0, -1.0, 0.0],
        color: [0.0, 1.0, 0.0, 1.0],
    },
    Vertex {
        position: [1.0, -1.0, 0.0],
        color: [0.0, 0.0, 1.0, 1.0],
    },
]);

#[repr(C, align(64))]
struct Shader<T: ?Sized>(T);

static SHADER: &Shader<[u8]> = &Shader(*include_bytes!(concat!(env!("OUT_DIR"), "/shader.gsh")));

struct Triangle {
    group: WHBGfxShaderGroup,
    projection_offset: u32,
    model_view_offset: u32,
}

impl Triangle {
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
                c"aColor".as_ptr(),
                0,
                12,
                GX2_ATTRIB_FORMAT_FLOAT_32_32_32_32,
            );
            WHBGfxInitFetchShader(&mut group);
            let projection_offset =
                GX2GetVertexUniformVar(group.vertex_shader, c"uProjection")?.offset;
            let model_view_offset =
                GX2GetVertexUniformVar(group.vertex_shader, c"uModelView")?.offset;
            GX2Invalidate(
                GX2_INVALIDATE_MODE_CPU_ATTRIBUTE_BUFFER,
                (&raw mut VERTICES).cast(),
                mem::size_of::<Vertices>() as u32,
            );
            Some(Self {
                group,
                projection_offset,
                model_view_offset,
            })
        }
    }

    unsafe fn draw(&mut self, projection: &Mtx44, matrix: &Mtx) {
        unsafe {
            WHBGfxClearColor(0.5, 0.5, 0.5, 1.0);
            GX2SetFetchShader(&self.group.fetch_shader);
            GX2SetVertexShader(self.group.vertex_shader);
            GX2SetPixelShader(self.group.pixel_shader);
            GX2SetVertexUniformReg(self.projection_offset, 16, projection.as_ptr().cast());
            GX2SetVertexUniformReg(self.model_view_offset, 12, matrix.as_ptr().cast());
            GX2SetAttribBuffer(
                0,
                mem::size_of::<Vertices>() as u32,
                mem::size_of::<Vertex>() as u32,
                (&raw const VERTICES).cast::<c_void>(),
            );
            GX2DrawEx(GX2_PRIMITIVE_MODE_TRIANGLES, 3, 0, 1);
        }
    }
}

impl Drop for Triangle {
    fn drop(&mut self) {
        unsafe { WHBGfxFreeShaderGroup(&mut self.group) };
    }
}

#[unsafe(export_name = "main")]
pub extern "C" fn main() -> c_int {
    unsafe {
        WHBProcInit();
        WHBGfxInit();
        let Some(mut triangle) = Triangle::new() else {
            return 1;
        };

        // The TV renders at 1280x720 and the GamePad at 854x480, both about 16:9
        let mut projection = [[0.0; 4]; 4];
        guPerspective(&mut projection, 45.0, 16.0 / 9.0, 0.1, 100.0);
        let started = OSGetTime();
        let ticks_per_second = OSTimerClockSpeed() as f32;
        while WHBProcIsRunning() != FALSE {
            let seconds = (OSGetTime() - started) as f32 / ticks_per_second;
            let mut matrix = [[0.0; 4]; 3];
            ps_guMtxRotRad(&mut matrix, b'y' as _, seconds);
            ps_guMtxTransApply(&matrix, &mut matrix, 0.0, 0.0, -3.0);

            WHBGfxBeginRender();
            WHBGfxBeginRenderTV();
            triangle.draw(&projection, &matrix);
            WHBGfxFinishRenderTV();
            WHBGfxBeginRenderDRC();
            triangle.draw(&projection, &matrix);
            WHBGfxFinishRenderDRC();
            WHBGfxFinishRender();
        }

        drop(triangle);
        WHBGfxShutdown();
        WHBProcShutdown();
    }
    0
}
