/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The display and drawing with GX: the chunks as display lists of fixed point vertices lit
//! by an indexed palette, the other draws in immediate mode.

use alloc::vec::Vec;
use core::ops::Range;
use core::ptr;

use libogc_sys::*;
use plaatcraft_engine::client::{
    Fog, Gpu, Mtx, Mtx44, Pass, State, Texture, Vertex, mesh, palette,
};
use plaatcraft_engine::world::SECTION_COUNT;

use crate::memory::{self, AlignedBuffer, Framebuffer};
use crate::textures::{self, Textures};

const FIFO_SIZE: usize = 256 * 1024;

/// A part of the display list with quads of one tile.
struct Draw {
    tile: usize,
    list: Range<usize>,
}

pub struct Mesh {
    /// 32-byte aligned by the allocator and flushed from the CPU cache.
    list: Vec<u8>,
    draws: Vec<Draw>,
    /// The draws of each section and pass.
    sections: [[Range<usize>; 3]; SECTION_COUNT],
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Chunk,
    Immediate,
}

pub struct Gx {
    buffers: [Framebuffer; 2],
    current: usize,
    first_frame: bool,
    _fifo: AlignedBuffer,
    size: [f32; 2],
    textures: Textures,
    palette: Vec<[u8; 4]>,
    daylight: f32,
    format: Option<Format>,
    textured: Option<bool>,
    bound: Option<Texture>,
}

impl Gx {
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
            // Clears the embedded framebuffer, which still holds the loader's last frame
            GX_CopyDisp(buffers[0].scanout(), GX_TRUE);
            Some(Self {
                buffers,
                current: 0,
                first_frame: true,
                _fifo: fifo,
                size: [480.0 * aspect, 480.0],
                textures: Textures::load(),
                palette: Vec::new(),
                daylight: -1.0,
                format: None,
                textured: None,
                bound: None,
            })
        }
    }

    fn format(&mut self, format: Format) {
        if self.format == Some(format) {
            return;
        }
        self.format = Some(format);
        unsafe {
            GX_ClearVtxDesc();
            GX_SetVtxDesc(GX_VA_POS, GX_DIRECT);
            GX_SetVtxDesc(GX_VA_TEX0, GX_DIRECT);
            match format {
                Format::Chunk => {
                    GX_SetVtxDesc(GX_VA_CLR0, GX_INDEX16);
                    GX_SetVtxAttrFmt(GX_VTXFMT0, GX_VA_POS as u32, GX_POS_XYZ, GX_U16, 8);
                    GX_SetVtxAttrFmt(GX_VTXFMT0, GX_VA_CLR0 as u32, GX_CLR_RGBA, GX_RGBA8, 0);
                    GX_SetVtxAttrFmt(GX_VTXFMT0, GX_VA_TEX0 as u32, GX_TEX_ST, GX_U16, 8);
                    GX_SetArray(
                        GX_VA_CLR0 as u32,
                        self.palette.as_ptr().cast_mut().cast(),
                        4,
                    );
                    GX_InvVtxCache();
                }
                Format::Immediate => {
                    GX_SetVtxDesc(GX_VA_CLR0, GX_DIRECT);
                    GX_SetVtxAttrFmt(GX_VTXFMT1, GX_VA_POS as u32, GX_POS_XYZ, GX_F32, 0);
                    GX_SetVtxAttrFmt(GX_VTXFMT1, GX_VA_CLR0 as u32, GX_CLR_RGBA, GX_RGBA8, 0);
                    GX_SetVtxAttrFmt(GX_VTXFMT1, GX_VA_TEX0 as u32, GX_TEX_ST, GX_F32, 0);
                }
            }
        }
    }

    // Uses one color channel from the vertices and the first texture, or no texture.
    fn texture(&mut self, texture: Texture) {
        let textured = texture != Texture::None;
        if self.textured != Some(textured) {
            self.textured = Some(textured);
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
        if self.bound == Some(texture) {
            return;
        }
        let object = match texture {
            Texture::None => return,
            Texture::Tile(tile) => &self.textures.tiles[tile],
            Texture::Sky(layer) => &self.textures.sky[layer],
            Texture::Character(part) => &self.textures.characters[part],
            Texture::Font => &self.textures.font,
            Texture::Cursor => &self.textures.cursor,
        };
        self.bound = Some(texture);
        unsafe { GX_LoadTexObj(object, GX_TEXMAP0 as u8) };
    }
}

#[inline]
unsafe fn pipe_f32(value: f32) {
    unsafe { ptr::write_volatile(wgPipe.cast::<f32>(), value) }
}

#[inline]
unsafe fn pipe_u32(value: u32) {
    unsafe { ptr::write_volatile(wgPipe.cast::<u32>(), value) }
}

unsafe fn pipe(primitive: u8, vertices: &[Vertex]) {
    unsafe {
        GX_Begin(primitive, GX_VTXFMT1, vertices.len() as u16);
        for vertex in vertices {
            for value in vertex.position {
                pipe_f32(value);
            }
            pipe_u32(vertex.color);
            pipe_f32(vertex.uv[0]);
            pipe_f32(vertex.uv[1]);
        }
    }
}

const fn pass_index(pass: Pass) -> usize {
    match pass {
        Pass::Solid => 0,
        Pass::Cutout => 1,
        Pass::Translucent => 2,
    }
}

impl Gpu for Gx {
    type Mesh = Mesh;

    fn mesh(source: &mesh::Mesh) -> Mesh {
        let mut mesh = Mesh {
            list: Vec::with_capacity(source.vertices.len() * 12 + source.groups.len() * 32),
            draws: Vec::with_capacity(source.groups.len()),
            sections: Default::default(),
        };
        for group in &source.groups {
            let offset = mesh.list.len();
            mesh.list.push(GX_QUADS);
            mesh.list
                .extend((group.vertices.len() as u16).to_be_bytes());
            // Fixed point retains sub-block shapes and fluid heights with compact vertices.
            let fixed = |value: f32| ((value * 256.0 + 0.5) as u16).to_be_bytes();
            for vertex in &source.vertices[group.vertices.clone()] {
                for value in vertex.position {
                    mesh.list.extend(fixed(value));
                }
                mesh.list.extend(vertex.light.to_be_bytes());
                for value in vertex.uv {
                    mesh.list.extend(fixed(value));
                }
            }
            // Display lists are called in 32 byte blocks, padded with GX_NOP commands
            mesh.list.resize(mesh.list.len().next_multiple_of(32), 0);
            let draws = &mut mesh.sections[group.section][pass_index(group.pass)];
            if draws.start == draws.end {
                *draws = mesh.draws.len()..mesh.draws.len();
            }
            draws.end += 1;
            mesh.draws.push(Draw {
                tile: group.tile,
                list: offset..mesh.list.len(),
            });
        }
        memory::flush(&mesh.list);
        mesh
    }

    fn has_alpha(tile: usize) -> bool {
        textures::has_alpha(tile)
    }

    fn draws(mesh: &Mesh, section: usize, pass: Pass) -> bool {
        !mesh.sections[section][pass_index(pass)].is_empty()
    }

    fn size(&self) -> [f32; 2] {
        self.size
    }

    fn begin(&mut self, [r, g, b]: [u8; 3], daylight: f32, _time: f32) {
        if self.daylight != daylight {
            self.daylight = daylight;
            self.palette = palette(daylight);
            memory::flush(&self.palette);
            // The palette moved, so the chunks' format points GX at it again
            self.format = None;
        }
        unsafe { GX_SetCopyClear(GXColor { r, g, b, a: 255 }, GX_MAX_Z24) };
    }

    fn projection(&mut self, matrix: &Mtx44, orthographic: bool) {
        let kind = if orthographic {
            GX_ORTHOGRAPHIC
        } else {
            GX_PERSPECTIVE
        };
        unsafe { GX_LoadProjectionMtx(matrix, kind) };
    }

    fn model_view(&mut self, matrix: &Mtx) {
        unsafe { GX_LoadPosMtxImm(matrix, GX_PNMTX0) };
    }

    fn state(&mut self, state: State) {
        unsafe {
            GX_SetCullMode(if state.cull {
                GX_CULL_BACK
            } else {
                GX_CULL_NONE
            });
            if state.depth_test {
                GX_SetZMode(GX_ENABLE, GX_LEQUAL, u8::from(state.depth_write));
            } else {
                GX_SetZMode(GX_DISABLE, GX_ALWAYS, GX_FALSE);
            }
            if state.blend {
                GX_SetBlendMode(GX_BM_BLEND, GX_BL_SRCALPHA, GX_BL_INVSRCALPHA, GX_LO_CLEAR);
            } else {
                GX_SetBlendMode(GX_BM_NONE, GX_BL_ONE, GX_BL_ZERO, GX_LO_CLEAR);
            }
            // Solid draws test depth before texturing, so hidden texels are never fetched,
            // cutouts after it, so they only write depth for their visible texels
            if state.cutout {
                GX_SetZCompLoc(GX_FALSE);
                GX_SetAlphaCompare(GX_GREATER, 127, GX_AOP_AND, GX_ALWAYS, 0);
            } else {
                GX_SetZCompLoc(GX_TRUE);
                GX_SetAlphaCompare(GX_ALWAYS, 0, GX_AOP_AND, GX_ALWAYS, 0);
            }
        }
    }

    fn fog(&mut self, fog: Option<Fog>) {
        let (kind, start, end, [near, far], [r, g, b]) = fog
            .map_or((GX_FOG_NONE, 0.0, 1.0, [0.0, 1.0], [0; 3]), |fog| {
                (GX_FOG_LIN, fog.start, fog.end, fog.planes, fog.color)
            });
        unsafe { GX_SetFog(kind, start, end, near, far, GXColor { r, g, b, a: 255 }) };
    }

    fn quads(&mut self, texture: Texture, vertices: &[Vertex]) {
        self.format(Format::Immediate);
        self.texture(texture);
        unsafe { pipe(GX_QUADS, vertices) };
    }

    fn lines(&mut self, vertices: &[Vertex]) {
        self.format(Format::Immediate);
        self.texture(Texture::None);
        unsafe {
            GX_SetLineWidth(12, GX_TO_ZERO);
            pipe(GX_LINES, vertices);
        }
    }

    fn chunk(&mut self, mesh: &Mesh, section: usize, pass: Pass) {
        self.format(Format::Chunk);
        for draw in &mesh.draws[mesh.sections[section][pass_index(pass)].clone()] {
            self.texture(Texture::Tile(draw.tile));
            let list = &mesh.list[draw.list.clone()];
            unsafe { GX_CallDispList(list.as_ptr().cast(), list.len() as u32) };
        }
    }

    fn present(&mut self) {
        unsafe {
            self.current ^= 1;
            GX_SetZMode(GX_ENABLE, GX_LEQUAL, GX_TRUE);
            GX_CopyDisp(self.buffers[self.current].scanout(), GX_TRUE);
            // The copy follows the drawing, so this waits for both before the frame is shown
            GX_DrawDone();
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
