/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Generates a random 64x64x64 voxel world and renders its visible block faces on the TV and the
//! GamePad.

#![no_std]
#![no_main]

use core::f64::consts::TAU;
use core::ffi::c_int;
use core::mem::{MaybeUninit, offset_of};

use wut_sys::*;

use crate::materials::Materials;
use crate::mesh::{Mesh, Vertex};
use crate::world::World;

mod materials;
mod memory;
mod mesh;
mod world;

#[repr(C, align(64))]
struct Shader<T: ?Sized>(T);

static SHADER: &Shader<[u8]> = &Shader(*include_bytes!(concat!(env!("OUT_DIR"), "/shader.gsh")));

struct Blocks {
    group: WHBGfxShaderGroup,
    projection_offset: u32,
    model_view_offset: u32,
    materials: Materials,
    mesh: Mesh,
}

impl Blocks {
    unsafe fn new() -> Option<Self> {
        unsafe {
            let mut group = MaybeUninit::<WHBGfxShaderGroup>::zeroed().assume_init();
            if WHBGfxLoadGFDShaderGroup(&mut group, 0, SHADER.0.as_ptr().cast()) == FALSE {
                return None;
            }
            for (name, offset, format) in [
                (
                    c"aPosition",
                    offset_of!(Vertex, position),
                    GX2_ATTRIB_FORMAT_FLOAT_32_32_32,
                ),
                (
                    c"aTexCoord",
                    offset_of!(Vertex, tex_coord),
                    GX2_ATTRIB_FORMAT_UNORM_8_8,
                ),
                (
                    c"aColor",
                    offset_of!(Vertex, color),
                    GX2_ATTRIB_FORMAT_UNORM_8_8_8_8,
                ),
            ] {
                WHBGfxInitShaderAttribute(&mut group, name.as_ptr(), 0, offset as u32, format);
            }
            WHBGfxInitFetchShader(&mut group);
            let projection_offset =
                GX2GetVertexUniformVar(group.vertex_shader, c"uProjection")?.offset;
            let model_view_offset =
                GX2GetVertexUniformVar(group.vertex_shader, c"uModelView")?.offset;
            let materials = Materials::load()?;
            let world = World::generate(OSGetTime() as u32);
            let mesh = Mesh::build(&world.faces_by_material())?;
            Some(Self {
                group,
                projection_offset,
                model_view_offset,
                materials,
                mesh,
            })
        }
    }

    unsafe fn draw(&self, projection: &Mtx44, matrix: &Mtx) {
        unsafe {
            WHBGfxClearColor(46.0 / 255.0, 84.0 / 255.0, 158.0 / 255.0, 1.0);
            GX2SetFetchShader(&self.group.fetch_shader);
            GX2SetVertexShader(self.group.vertex_shader);
            GX2SetPixelShader(self.group.pixel_shader);
            GX2SetVertexUniformReg(self.projection_offset, 16, projection.as_ptr().cast());
            GX2SetVertexUniformReg(self.model_view_offset, 12, matrix.as_ptr().cast());
            self.mesh.draw(&self.materials);
        }
    }
}

impl Drop for Blocks {
    fn drop(&mut self) {
        unsafe { WHBGfxFreeShaderGroup(&mut self.group) };
    }
}

#[unsafe(export_name = "main")]
pub extern "C" fn main() -> c_int {
    unsafe {
        WHBProcInit();
        WHBGfxInit();
        let Some(blocks) = Blocks::new() else {
            return 1;
        };

        // The TV renders at 1280x720 and the GamePad at 854x480, both about 16:9
        let mut projection = [[0.0; 4]; 4];
        guPerspective(&mut projection, 50.0, 16.0 / 9.0, 1.0, 500.0);
        let started = OSGetTime();
        let ticks_per_second = f64::from(OSTimerClockSpeed());
        while WHBProcIsRunning() != FALSE {
            // The same orbit around the world as the other block renderers
            let seconds = (OSGetTime() - started) as f64 / ticks_per_second;
            let angle = ((seconds * 0.16) % TAU) as f32;
            let (sin, cos) = (sinf(angle), cosf(angle));
            let eye = GuVector {
                x: 32.0 + sin * 88.0,
                y: 21.76 + 58.0,
                z: 32.0 + cos * 88.0,
            };
            let up = GuVector {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            };
            let target = GuVector {
                x: 32.0,
                y: 21.76,
                z: 32.0,
            };
            let mut view = [[0.0; 4]; 3];
            guLookAt(&mut view, &eye, &up, &target);

            WHBGfxBeginRender();
            WHBGfxBeginRenderTV();
            blocks.draw(&projection, &view);
            WHBGfxFinishRenderTV();
            WHBGfxBeginRenderDRC();
            blocks.draw(&projection, &view);
            WHBGfxFinishRenderDRC();
            WHBGfxFinishRender();
        }

        drop(blocks);
        WHBGfxShutdown();
        WHBProcShutdown();
    }
    0
}
