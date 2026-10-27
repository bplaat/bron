/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use core::ffi::c_char;

use libogc_sys::*;

use crate::font::{FONT, FONT_RENDER_SIZE};
use crate::texture::Texture;

#[inline]
unsafe fn pipe_u32(value: u32) {
    unsafe { core::ptr::write_volatile(wgPipe.cast::<u32>(), value) }
}
#[inline]
unsafe fn pipe_f32(value: f32) {
    unsafe { core::ptr::write_volatile(wgPipe.cast::<f32>(), value) }
}

#[inline]
unsafe fn gx_position2(x: f32, y: f32) {
    unsafe {
        pipe_f32(x);
        pipe_f32(y);
    }
}

#[inline]
unsafe fn gx_position3(x: f32, y: f32, z: f32) {
    unsafe {
        pipe_f32(x);
        pipe_f32(y);
        pipe_f32(z);
    }
}

#[inline]
unsafe fn gx_texcoord(s: f32, t: f32) {
    unsafe {
        pipe_f32(s);
        pipe_f32(t);
    }
}

#[inline]
unsafe fn gx_color(color: u32) {
    unsafe {
        pipe_u32(color);
    }
}

unsafe fn draw_quad(color: u32, uvs: [[f32; 2]; 4]) {
    unsafe {
        GX_Begin(GX_QUADS, GX_VTXFMT0, 4);
        for (position, uv) in [
            ([0.5, -0.5], uvs[0]),
            ([0.5, 0.5], uvs[1]),
            ([-0.5, 0.5], uvs[2]),
            ([-0.5, -0.5], uvs[3]),
        ] {
            gx_position2(position[0], position[1]);
            gx_color(color);
            gx_texcoord(uv[0], uv[1]);
        }
    }
}

pub unsafe fn draw_image(
    texture: &Texture,
    transform: &Mtx,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: u32,
) {
    unsafe {
        texture.bind(GX_TEXMAP0 as u8);
        let source = [
            [width, 0.0, 0.0, x + width / 2.0],
            [0.0, height, 0.0, y + height / 2.0],
            [0.0, 0.0, 1.0, 0.0],
        ];
        let mut matrix = [[0.0; 4]; 3];
        ps_guMtxConcat(&source, transform, &mut matrix);
        GX_LoadPosMtxImm(&matrix, GX_PNMTX0);
        // Map the PNG's top row to the top vertices of the quad.
        draw_quad(color, [[1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]]);
    }
}

pub unsafe fn draw_text(
    texture: &Texture,
    transform: &Mtx,
    text: &str,
    mut x: f32,
    y: f32,
    size: f32,
    color: u32,
) {
    unsafe {
        texture.bind(GX_TEXMAP0 as u8);
        let scale = size / FONT_RENDER_SIZE as f32;
        for ch in text.chars() {
            if ch == ' ' {
                x += 16.0 * scale;
                continue;
            }
            let Some(glyph) = FONT.iter().find(|glyph| glyph.n == ch as u32) else {
                continue;
            };
            if glyph.w == 0 || glyph.h == 0 {
                continue;
            }
            let width = glyph.w as f32 * scale;
            let height = glyph.h as f32 * scale;
            let source = [
                [width, 0.0, 0.0, x + width / 2.0],
                [0.0, height, 0.0, y + glyph.a as f32 * scale + height / 2.0],
                [0.0, 0.0, 1.0, 0.0],
            ];
            let mut matrix = [[0.0; 4]; 3];
            ps_guMtxConcat(&source, transform, &mut matrix);
            GX_LoadPosMtxImm(&matrix, GX_PNMTX0);
            // The glyph atlas uses the same top-to-bottom PNG orientation.
            const TEXEL: f32 = 1.0 / 480.0;
            let left = (glyph.x as f32 + 0.5) * TEXEL;
            let top = (glyph.y as f32 + 0.5) * TEXEL;
            let right = (glyph.x as f32 + glyph.w as f32 - 0.5) * TEXEL;
            let bottom = (glyph.y as f32 + glyph.h as f32 - 0.5) * TEXEL;
            draw_quad(
                if glyph.c { u32::MAX } else { color },
                [[right, top], [right, bottom], [left, bottom], [left, top]],
            );
            x += (glyph.w as f32 + 2.0) * scale;
        }
    }
}

pub unsafe fn draw_cube(texture: &Texture, rotation: f32, projection: &Mtx44) {
    unsafe {
        GX_LoadProjectionMtx(projection, GX_PERSPECTIVE);
        GX_SetZMode(GX_ENABLE, GX_LEQUAL, GX_TRUE);
        GX_SetCullMode(GX_CULL_NONE);
        GX_ClearVtxDesc();
        GX_SetVtxDesc(GX_VA_POS, GX_DIRECT);
        GX_SetVtxDesc(GX_VA_TEX0, GX_DIRECT);
        GX_SetVtxAttrFmt(GX_VTXFMT0, GX_VA_POS as u32, GX_POS_XYZ, GX_F32, 0);
        GX_SetVtxAttrFmt(GX_VTXFMT0, GX_VA_TEX0 as u32, GX_TEX_ST, GX_F32, 0);
        GX_SetNumChans(1);
        GX_SetNumTexGens(1);
        GX_SetTexCoordGen(GX_TEXCOORD0, GX_TG_MTX2X4, GX_TG_TEX0, GX_IDENTITY);
        GX_SetTevOrder(GX_TEVSTAGE0, GX_TEXCOORD0 as u8, GX_TEXMAP0, GX_COLOR0A0);
        GX_SetTevOp(GX_TEVSTAGE0, GX_REPLACE);
        texture.bind(GX_TEXMAP0 as u8);
        let mut matrix = [[0.0; 4]; 3];
        let mut temp = [[0.0; 4]; 3];
        rotate_degrees(&mut matrix, b'x' as c_char, rotation);
        rotate_degrees(&mut temp, b'y' as c_char, rotation);
        ps_guMtxConcat(&matrix, &temp, &mut matrix);
        ps_guMtxTransApply(&matrix, &mut matrix, 0.0, 0.0, -8.0);
        GX_LoadPosMtxImm(&matrix, GX_PNMTX0);
        let faces: [[([f32; 3], [f32; 2]); 4]; 6] = [
            [
                ([-1.0, 1.0, -1.0], [0.0, 0.0]),
                ([-1.0, 1.0, 1.0], [1.0, 0.0]),
                ([-1.0, -1.0, 1.0], [1.0, 1.0]),
                ([-1.0, -1.0, -1.0], [0.0, 1.0]),
            ],
            [
                ([1.0, 1.0, -1.0], [0.0, 0.0]),
                ([1.0, -1.0, -1.0], [1.0, 0.0]),
                ([1.0, -1.0, 1.0], [1.0, 1.0]),
                ([1.0, 1.0, 1.0], [0.0, 1.0]),
            ],
            [
                ([-1.0, -1.0, 1.0], [0.0, 0.0]),
                ([1.0, -1.0, 1.0], [1.0, 0.0]),
                ([1.0, -1.0, -1.0], [1.0, 1.0]),
                ([-1.0, -1.0, -1.0], [0.0, 1.0]),
            ],
            [
                ([-1.0, 1.0, 1.0], [0.0, 0.0]),
                ([-1.0, 1.0, -1.0], [1.0, 0.0]),
                ([1.0, 1.0, -1.0], [1.0, 1.0]),
                ([1.0, 1.0, 1.0], [0.0, 1.0]),
            ],
            [
                ([1.0, -1.0, -1.0], [0.0, 0.0]),
                ([1.0, 1.0, -1.0], [1.0, 0.0]),
                ([-1.0, 1.0, -1.0], [1.0, 1.0]),
                ([-1.0, -1.0, -1.0], [0.0, 1.0]),
            ],
            [
                ([1.0, -1.0, 1.0], [0.0, 0.0]),
                ([-1.0, -1.0, 1.0], [1.0, 0.0]),
                ([-1.0, 1.0, 1.0], [1.0, 1.0]),
                ([1.0, 1.0, 1.0], [0.0, 1.0]),
            ],
        ];
        GX_Begin(GX_QUADS, GX_VTXFMT0, 24);
        for face in faces {
            for (position, uv) in face {
                gx_position3(position[0], position[1], position[2]);
                gx_texcoord(uv[0], uv[1]);
            }
        }
    }
}
#[inline]
pub unsafe fn rotate_degrees(matrix: *mut Mtx, axis: c_char, degrees: f32) {
    unsafe { ps_guMtxRotRad(matrix, axis, degrees * (core::f32::consts::PI / 180.0)) }
}
