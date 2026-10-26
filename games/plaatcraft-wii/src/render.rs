/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The display, and drawing the world with GX: the chunk display lists lit by a palette of
//! the terrain shader's light levels, fog in the distance and the targeted block outline.

use alloc::vec::Vec;
use core::ffi::c_void;
use core::ptr;

use libogc_sys::*;
use plaatcraft_engine::camera::View;
use plaatcraft_engine::world::{CHUNK_SIZE, Coord, LAVA, SECTION_COUNT, WATER};

use crate::memory::{AlignedBuffer, Framebuffer};
use crate::mesh::{PALETTE_SIZE, TINT_BEDROCK, TINT_LAVA, TINT_PINE, TINT_WATER};
use crate::stream::{Chunk, RENDER_DISTANCE};
use crate::textures::Textures;

const FIFO_SIZE: usize = 256 * 1024;
const FOV: f32 = 75.0;
const NEAR: f32 = 0.05;

// MARK: Display
pub struct Display {
    buffers: [Framebuffer; 2],
    current: usize,
    first_frame: bool,
    _fifo: AlignedBuffer,
    pub aspect: f32,
    /// The size of the 2D screen space, square pixels with a height of 480.
    pub width: f32,
    pub height: f32,
}

impl Display {
    pub fn new() -> Option<Self> {
        unsafe {
            VIDEO_Init();
            VIDEO_SetBlack(true);
            let mode = VIDEO_GetPreferredMode(ptr::null_mut());
            let mode_ref = mode.as_ref()?;
            let buffers = [Framebuffer::new(mode_ref)?, Framebuffer::new(mode_ref)?];
            let fifo = AlignedBuffer::new_zeroed(FIFO_SIZE)?;
            let aspect = if CONF_GetAspectRatio() == CONF_ASPECT_16_9 {
                16.0 / 9.0
            } else {
                4.0 / 3.0
            };
            VIDEO_Configure(mode);
            VIDEO_SetNextFramebuffer(buffers[0].scanout());
            VIDEO_Flush();
            VIDEO_WaitVSync();
            if mode_ref.vi_tv_mode & VI_NON_INTERLACE != 0 {
                VIDEO_WaitVSync();
            }

            GX_Init(fifo.as_ptr(), fifo.size());
            GX_SetViewport(
                0.0,
                0.0,
                f32::from(mode_ref.fb_width),
                f32::from(mode_ref.efb_height),
                0.0,
                1.0,
            );
            GX_SetScissor(
                0,
                0,
                u32::from(mode_ref.fb_width),
                u32::from(mode_ref.efb_height),
            );
            let scale = GX_GetYScaleFactor(mode_ref.efb_height, mode_ref.xfb_height);
            let xfb_height = GX_SetDispCopyYScale(scale);
            GX_SetDispCopySrc(0, 0, mode_ref.fb_width, mode_ref.efb_height);
            GX_SetDispCopyDst(mode_ref.fb_width, xfb_height as u16);
            GX_SetCopyFilter(
                mode_ref.aa,
                mode_ref.sample_pattern.as_ptr(),
                GX_TRUE,
                mode_ref.vfilter.as_ptr(),
            );
            GX_SetFieldMode(
                mode_ref.field_rendering,
                if mode_ref.vi_height == 2 * mode_ref.xfb_height {
                    GX_ENABLE
                } else {
                    GX_DISABLE
                },
            );
            GX_SetPixelFmt(
                if mode_ref.aa != 0 {
                    GX_PF_RGB565_Z16
                } else {
                    GX_PF_RGB8_Z24
                },
                GX_ZC_LINEAR,
            );
            GX_SetDispCopyGamma(GX_GM_1_0);
            GX_InvVtxCache();
            GX_InvalidateTexAll();
            Some(Self {
                buffers,
                current: 0,
                first_frame: true,
                _fifo: fifo,
                aspect,
                width: 480.0 * aspect,
                height: 480.0,
            })
        }
    }

    pub fn present(&mut self) {
        unsafe {
            GX_DrawDone();
            self.current ^= 1;
            GX_SetZMode(GX_ENABLE, GX_LEQUAL, GX_TRUE);
            GX_SetColorUpdate(GX_TRUE);
            GX_CopyDisp(self.buffers[self.current].scanout(), GX_TRUE);
            VIDEO_SetNextFramebuffer(self.buffers[self.current].scanout());
            if self.first_frame {
                VIDEO_SetBlack(false);
                self.first_frame = false;
            }
            VIDEO_Flush();
            VIDEO_WaitVSync();
        }
    }
}

// MARK: Light
fn encode(linear: f32) -> u8 {
    // GX has no sRGB, so light is applied to the gamma encoded textures
    (powf(linear.clamp(0.0, 1.0), 1.0 / 2.2) * 255.0 + 0.5) as u8
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    core::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

fn fog_color(daylight: f32) -> [f32; 3] {
    mix([0.012, 0.018, 0.04], [0.64, 0.79, 0.90], daylight)
}

fn gx_color(color: [f32; 3]) -> GXColor {
    GXColor {
        r: encode(color[0]),
        g: encode(color[1]),
        b: encode(color[2]),
        a: 255,
    }
}

// The illumination of the terrain and liquid shaders for every tint, face and light byte
fn palette(daylight: f32) -> Vec<[u8; 4]> {
    const TERRAIN_FACES: [f32; 8] = [0.8, 0.8, 1.0, 0.5, 0.65, 0.65, 1.0, 1.0];
    const LIQUID_FACES: [f32; 8] = [0.85, 0.85, 1.0, 0.65, 0.75, 0.75, 1.0, 1.0];
    let curve: [f32; 16] = core::array::from_fn(|level| powf(level as f32 / 15.0, 1.8));
    let mut palette = Vec::with_capacity(PALETTE_SIZE);
    for index in 0..PALETTE_SIZE as u16 {
        let tint = index >> 11;
        let face = usize::from(index >> 8 & 7);
        let light = index & 255;
        let sky = curve[usize::from(light & 15)] * daylight;
        let block = curve[usize::from(light >> 4)];
        let block = [block, block * 0.62, block * 0.30];
        let (sky, minimum, faces, color, alpha) = match tint {
            TINT_WATER => ([sky; 3], 0.025, LIQUID_FACES, [0.15, 0.58, 0.75], 158),
            TINT_PINE => (
                sky_color(sky),
                0.018,
                TERRAIN_FACES,
                [0.48, 0.68, 0.55],
                255,
            ),
            TINT_BEDROCK => (
                sky_color(sky),
                0.018,
                TERRAIN_FACES,
                [0.25, 0.27, 0.30],
                255,
            ),
            _ => (sky_color(sky), 0.018, TERRAIN_FACES, [1.0; 3], 255),
        };
        let lit: [f32; 3] = if tint == TINT_LAVA {
            [1.0; 3]
        } else {
            core::array::from_fn(|i| sky[i].max(block[i]).max(minimum) * faces[face] * color[i])
        };
        palette.push([encode(lit[0]), encode(lit[1]), encode(lit[2]), alpha]);
    }
    crate::memory::flush(&palette);
    palette
}

fn sky_color(sky: f32) -> [f32; 3] {
    [0.85 * sky, 0.92 * sky, sky]
}

// MARK: Renderer
pub struct Renderer {
    palette: Vec<[u8; 4]>,
    daylight: f32,
    projection: Mtx44,
    far: f32,
}

impl Renderer {
    pub fn new(display: &Display) -> Self {
        let far = (RENDER_DISTANCE * CHUNK_SIZE as i64 * 3) as f32;
        let mut projection = [[0.0; 4]; 4];
        unsafe { guPerspective(&mut projection, FOV, display.aspect, NEAR, far) };
        Self {
            palette: Vec::new(),
            daylight: -1.0,
            projection,
            far,
        }
    }

    /// The view for culling, its size has the aspect ratio of the screen.
    pub fn view(&self, camera: &plaatcraft_engine::camera::Camera, display: &Display) -> View {
        camera.view(display.width as u32, display.height as u32, RENDER_DISTANCE)
    }

    /// Clears the screen to the color of the sky and the fog.
    pub fn begin(&mut self, daylight: f32) {
        if self.daylight != daylight {
            self.daylight = daylight;
            self.palette = palette(daylight);
        }
        unsafe { GX_SetCopyClear(gx_color(fog_color(daylight)), GX_MAX_Z24) };
    }

    pub fn draw_world(
        &self,
        view: &View,
        center: Coord,
        chunks: &alloc::collections::BTreeMap<Coord, Chunk>,
        textures: &Textures,
        eye_material: Option<u8>,
    ) {
        unsafe {
            GX_LoadProjectionMtx(&self.projection, GX_PERSPECTIVE);
            GX_ClearVtxDesc();
            GX_SetVtxDesc(GX_VA_POS, GX_DIRECT);
            GX_SetVtxDesc(GX_VA_CLR0, GX_INDEX16);
            GX_SetVtxDesc(GX_VA_TEX0, GX_DIRECT);
            GX_SetVtxAttrFmt(GX_VTXFMT0, GX_VA_POS as u32, GX_POS_XYZ, GX_U8, 0);
            GX_SetVtxAttrFmt(GX_VTXFMT0, GX_VA_CLR0 as u32, GX_CLR_RGBA, GX_RGBA8, 0);
            GX_SetVtxAttrFmt(GX_VTXFMT0, GX_VA_TEX0 as u32, GX_TEX_ST, GX_U8, 0);
            GX_SetArray(
                GX_VA_CLR0 as u32,
                self.palette.as_ptr().cast_mut().cast(),
                4,
            );
            GX_InvVtxCache();
            set_vertex_color_texture(true);
            GX_SetTevOp(GX_TEVSTAGE0, GX_MODULATE);

            // The fog of water and lava is close and colored, like the shaders' immersion fog
            let fog_end = (RENDER_DISTANCE as f32 - 1.5) * CHUNK_SIZE as f32;
            let (start, end, color) = match eye_material {
                Some(WATER) => (
                    0.0,
                    24.0,
                    [0.025, 0.16, 0.23].map(|c| c * self.daylight.max(0.06)),
                ),
                Some(LAVA) => (0.0, 4.0, [0.65, 0.035, 0.005]),
                _ => (fog_end * 0.75, fog_end, fog_color(self.daylight)),
            };
            GX_SetFog(GX_FOG_LIN, start, end, NEAR, self.far, gx_color(color));
        }

        // Nearest chunks first, so the depth test rejects more of the farther ones
        let mut visible: Vec<(i64, Coord, [f32; 3])> = chunks
            .keys()
            .map(|&coord| (coord, offset(coord, center)))
            .filter(|(_, offset)| view.visible(*offset))
            .map(|(coord, offset)| {
                (
                    (coord.0 - center.0).pow(2) + (coord.1 - center.1).pow(2),
                    coord,
                    offset,
                )
            })
            .collect();
        visible.sort_unstable_by_key(|&(distance, ..)| distance);

        unsafe {
            GX_SetCullMode(GX_CULL_BACK);
            GX_SetZMode(GX_ENABLE, GX_LEQUAL, GX_TRUE);
            GX_SetBlendMode(GX_BM_NONE, GX_BL_ONE, GX_BL_ZERO, GX_LO_CLEAR);
            // Cutouts like leaves and plants only write depth for their visible texels
            GX_SetZCompLoc(GX_FALSE);
            GX_SetAlphaCompare(GX_GREATER, 127, GX_AOP_AND, GX_ALWAYS, 0);
        }
        let mut bound = None;
        for &(_, coord, offset) in &visible {
            self.draw_chunk(view, &chunks[&coord], offset, 0, textures, &mut bound);
        }
        // Translucent blocks blend over everything else, farthest first
        unsafe {
            GX_SetCullMode(GX_CULL_NONE);
            GX_SetZMode(GX_ENABLE, GX_LEQUAL, GX_FALSE);
            GX_SetBlendMode(GX_BM_BLEND, GX_BL_SRCALPHA, GX_BL_INVSRCALPHA, GX_LO_CLEAR);
            GX_SetAlphaCompare(GX_ALWAYS, 0, GX_AOP_AND, GX_ALWAYS, 0);
        }
        for &(_, coord, offset) in visible.iter().rev() {
            self.draw_chunk(view, &chunks[&coord], offset, 1, textures, &mut bound);
        }
        unsafe {
            GX_SetZMode(GX_ENABLE, GX_LEQUAL, GX_TRUE);
            GX_SetZCompLoc(GX_TRUE);
            GX_SetFog(
                GX_FOG_NONE,
                0.0,
                1.0,
                0.0,
                1.0,
                GXColor {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 0,
                },
            );
        }
    }

    fn draw_chunk(
        &self,
        view: &View,
        chunk: &Chunk,
        offset: [f32; 3],
        pass: usize,
        textures: &Textures,
        bound: &mut Option<u8>,
    ) {
        let matrix = model_view(view, offset);
        unsafe { GX_LoadPosMtxImm(&matrix, GX_PNMTX0) };
        for section in 0..SECTION_COUNT {
            let draws = chunk.mesh.sections[section][pass].clone();
            if draws.is_empty() || !view.section_visible(offset, section) {
                continue;
            }
            for draw in &chunk.mesh.draws[draws] {
                if *bound != Some(draw.tile) {
                    *bound = Some(draw.tile);
                    unsafe {
                        GX_LoadTexObj(&textures.tiles[usize::from(draw.tile)], GX_TEXMAP0 as u8)
                    };
                }
                let list = &chunk.mesh.list[draw.list.clone()];
                unsafe { GX_CallDispList(list.as_ptr().cast::<c_void>(), list.len() as u32) };
            }
        }
    }

    /// Outlines the targeted block like the selection shader.
    pub fn draw_selection(&self, view: &View, center: Coord, block: [i64; 3]) {
        const GROW: f32 = 0.004;
        let size = CHUNK_SIZE as i64;
        let offset = [
            (block[0] - center.0 * size) as f32,
            block[1] as f32,
            (block[2] - center.1 * size) as f32,
        ];
        let matrix = model_view(view, offset);
        unsafe {
            GX_LoadPosMtxImm(&matrix, GX_PNMTX0);
            GX_ClearVtxDesc();
            GX_SetVtxDesc(GX_VA_POS, GX_DIRECT);
            GX_SetVtxDesc(GX_VA_CLR0, GX_DIRECT);
            GX_SetVtxAttrFmt(GX_VTXFMT1, GX_VA_POS as u32, GX_POS_XYZ, GX_F32, 0);
            GX_SetVtxAttrFmt(GX_VTXFMT1, GX_VA_CLR0 as u32, GX_CLR_RGBA, GX_RGBA8, 0);
            set_vertex_color_texture(false);
            GX_SetBlendMode(GX_BM_BLEND, GX_BL_SRCALPHA, GX_BL_INVSRCALPHA, GX_LO_CLEAR);
            GX_SetLineWidth(12, GX_TO_ZERO);
            GX_Begin(GX_LINES, GX_VTXFMT1, 24);
            for axis in 0..3 {
                for corner in 0..4 {
                    for end in 0..2 {
                        let mut p = [0.0f32; 3];
                        p[axis] = if end == 0 { -GROW } else { 1.0 + GROW };
                        p[(axis + 1) % 3] = if corner & 1 == 0 { -GROW } else { 1.0 + GROW };
                        p[(axis + 2) % 3] = if corner & 2 == 0 { -GROW } else { 1.0 + GROW };
                        pipe_f32(p[0]);
                        pipe_f32(p[1]);
                        pipe_f32(p[2]);
                        pipe_u32(0x0000_00a0);
                    }
                }
            }
            GX_SetBlendMode(GX_BM_NONE, GX_BL_ONE, GX_BL_ZERO, GX_LO_CLEAR);
        }
    }
}

/// Uses one color channel from the vertices and the first texture, or no texture.
pub fn set_vertex_color_texture(textured: bool) {
    unsafe {
        GX_SetNumChans(1);
        GX_SetChanCtrl(
            GX_COLOR0A0 as i32,
            GX_DISABLE,
            GX_SRC_VTX,
            GX_SRC_VTX,
            0,
            GX_DF_NONE,
            GX_AF_NONE,
        );
        GX_SetNumTevStages(1);
        if textured {
            GX_SetNumTexGens(1);
            GX_SetTexCoordGen(GX_TEXCOORD0, GX_TG_MTX2X4, GX_TG_TEX0, GX_IDENTITY);
            GX_SetTevOrder(GX_TEVSTAGE0, GX_TEXCOORD0 as u8, GX_TEXMAP0, GX_COLOR0A0);
            GX_SetTevOp(GX_TEVSTAGE0, GX_MODULATE);
        } else {
            GX_SetNumTexGens(0);
            GX_SetTevOrder(GX_TEVSTAGE0, GX_TEXCOORDNULL, GX_TEXMAP_NULL, GX_COLOR0A0);
            GX_SetTevOp(GX_TEVSTAGE0, GX_PASSCLR);
        }
    }
}

// Chunks are drawn relative to the player's chunk, so the floats stay small
const fn offset(coord: Coord, center: Coord) -> [f32; 3] {
    [
        ((coord.0 - center.0) * CHUNK_SIZE as i64) as f32,
        0.0,
        ((coord.1 - center.1) * CHUNK_SIZE as i64) as f32,
    ]
}

// The camera looks along negative z in GX eye space
fn model_view(view: &View, offset: [f32; 3]) -> Mtx {
    let rows = [view.right, view.up, view.forward.map(|v| -v)];
    let relative: [f32; 3] = core::array::from_fn(|i| offset[i] - view.eye[i]);
    rows.map(|row| {
        [
            row[0],
            row[1],
            row[2],
            row[0] * relative[0] + row[1] * relative[1] + row[2] * relative[2],
        ]
    })
}

#[inline]
pub unsafe fn pipe_f32(value: f32) {
    unsafe { ptr::write_volatile(wgPipe.cast::<f32>(), value) }
}

#[inline]
pub unsafe fn pipe_u32(value: u32) {
    unsafe { ptr::write_volatile(wgPipe.cast::<u32>(), value) }
}
