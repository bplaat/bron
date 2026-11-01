/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use alloc::vec::Vec;
use core::mem;
use core::ptr::NonNull;

use wut_sys::*;

use crate::materials::Materials;
use crate::world::{Face, Material};

// The texture coordinates are normalized bytes, 255 is 1.0
const UV: [[u8; 2]; 4] = [[0, 255], [255, 255], [255, 0], [0, 0]];

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Vertex {
    pub position: [f32; 3],
    pub tex_coord: [u8; 2],
    pub color: [u8; 4],
    _padding: [u8; 2],
}

/// The vertices of all faces, sorted by material, so each material draws one range
pub struct Mesh {
    vertices: NonNull<Vertex>,
    vertex_count: usize,
    ranges: [(u32, u32); Material::COUNT],
}

impl Mesh {
    pub fn build(faces: &[Vec<Face>; Material::COUNT]) -> Option<Self> {
        let vertex_count = faces.iter().map(|faces| faces.len() * 4).sum::<usize>();
        // The GPU reads the vertices straight from memory, which needs 64 byte alignment
        let vertices = NonNull::new(
            unsafe {
                memalign(
                    GX2_VERTEX_BUFFER_ALIGNMENT,
                    vertex_count * mem::size_of::<Vertex>(),
                )
            }
            .cast::<Vertex>(),
        )?;
        let mut ranges = [(0, 0); Material::COUNT];
        let mut next = 0;
        for (material, faces) in Material::ALL.into_iter().zip(faces) {
            ranges[material.index()] = (next as u32, (faces.len() * 4) as u32);
            for face in faces {
                for (index, vertex) in face_vertices(face, material == Material::Water)
                    .into_iter()
                    .enumerate()
                {
                    unsafe { vertices.add(next + index).write(vertex) };
                }
                next += 4;
            }
        }
        unsafe {
            GX2Invalidate(
                GX2_INVALIDATE_MODE_CPU_ATTRIBUTE_BUFFER,
                vertices.as_ptr().cast(),
                (vertex_count * mem::size_of::<Vertex>()) as u32,
            );
        }
        Some(Self {
            vertices,
            vertex_count,
            ranges,
        })
    }

    pub fn draw(&self, materials: &Materials) {
        unsafe {
            GX2SetAttribBuffer(
                0,
                (self.vertex_count * mem::size_of::<Vertex>()) as u32,
                mem::size_of::<Vertex>() as u32,
                self.vertices.as_ptr().cast(),
            );
            GX2SetPixelSampler(materials.sampler(), 0);

            // Faces wind counter-clockwise seen from outside, so culling the back faces skips
            // the faces seen from behind
            GX2SetCullOnlyControl(GX2_FRONT_FACE_CCW, FALSE, TRUE);
            GX2SetDepthOnlyControl(TRUE, TRUE, GX2_COMPARE_FUNC_LEQUAL);
            GX2SetColorControl(GX2_LOGIC_OP_COPY, 0, FALSE, TRUE);
            for material in Material::ALL.into_iter().filter(|&m| m != Material::Water) {
                self.draw_material(materials, material);
            }

            // The translucent water goes last, blended over the terrain without writing depth
            GX2SetCullOnlyControl(GX2_FRONT_FACE_CCW, FALSE, FALSE);
            GX2SetDepthOnlyControl(TRUE, FALSE, GX2_COMPARE_FUNC_LEQUAL);
            GX2SetColorControl(GX2_LOGIC_OP_COPY, 1, FALSE, TRUE);
            GX2SetBlendControl(
                GX2_RENDER_TARGET_0,
                GX2_BLEND_MODE_SRC_ALPHA,
                GX2_BLEND_MODE_INV_SRC_ALPHA,
                GX2_BLEND_COMBINE_MODE_ADD,
                TRUE,
                GX2_BLEND_MODE_ONE,
                GX2_BLEND_MODE_INV_SRC_ALPHA,
                GX2_BLEND_COMBINE_MODE_ADD,
            );
            self.draw_material(materials, Material::Water);
        }
    }

    unsafe fn draw_material(&self, materials: &Materials, material: Material) {
        let (first, count) = self.ranges[material.index()];
        if count > 0 {
            unsafe {
                GX2SetPixelTexture(materials.texture(material), 0);
                GX2DrawEx(GX2_PRIMITIVE_MODE_QUADS, count, first, 1);
            }
        }
    }
}

impl Drop for Mesh {
    fn drop(&mut self) {
        unsafe { free(self.vertices.as_ptr().cast()) }
    }
}

fn face_vertices(face: &Face, water: bool) -> [Vertex; 4] {
    let corners = face.side.corners();
    let shade = ((0.42 + 0.58 * face.side.light()) * f32::from(face.ambient_occlusion)) as u8;
    core::array::from_fn(|corner| Vertex {
        position: core::array::from_fn(|axis| {
            f32::from(face.position[axis] + corners[corner][axis])
        }),
        tex_coord: UV[corner],
        color: [shade, shade, shade, if water { 153 } else { 255 }],
        _padding: [0; 2],
    })
}
