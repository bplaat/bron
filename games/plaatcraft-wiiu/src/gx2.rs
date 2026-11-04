/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Drawing with GX2 on the TV, the GamePad shows a copy. Linear lighting, sRGB textures and
//! output, per-pixel radial fog and animated water follow the desktop shaders. The chunks
//! have day and night colors so the night needs no new meshes. Other draws stream vertices
//! to a buffer that starts over every frame.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::mem::{MaybeUninit, offset_of};
use core::ops::Range;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicPtr, Ordering};

use plaatcraft_engine::client::{
    Fog, Gpu, Mtx, Mtx44, NIGHT_DAYLIGHT, Pass, State, Texture, Vertex, linear_palette, linear_rgb,
    mesh,
};
use plaatcraft_engine::world::SECTION_COUNT;
use wut_sys::*;

use crate::memory;

mod textures {
    include!(concat!(env!("OUT_DIR"), "/textures.rs"));
}

#[repr(C, align(64))]
struct Shader<T: ?Sized>(T);

static SHADER: &Shader<[u8]> = &Shader(*include_bytes!(concat!(env!("OUT_DIR"), "/shader.gsh")));

// The GPU reads the textures in place, which needs the 4 KiB alignment of their tiles
#[repr(C, align(4096))]
struct Textures<T: ?Sized>(T);

static TEXTURES: &Textures<[u8]> =
    &Textures(*include_bytes!(concat!(env!("OUT_DIR"), "/textures.gtx")));

// The vertices of the draws besides the chunks of a frame
const STREAM_SIZE: usize = 4 * 1024 * 1024;
// The worker writes chunk vertices to memory the GPU's vertex cache may still hold
const INVALIDATE_ATTRIBUTE_BUFFER: u32 = 1 << 0;

/// A vertex of the shader, the night color only differs for chunks.
#[repr(C)]
#[derive(Clone, Copy)]
struct GpuVertex {
    position: [f32; 3],
    color: [f32; 4],
    uv: [f32; 2],
    night: [f32; 4],
}

// Vertex attributes must start on a four-byte boundary on the console.
const _: () = assert!(
    offset_of!(GpuVertex, color).is_multiple_of(4)
        && offset_of!(GpuVertex, night).is_multiple_of(4)
        && size_of::<GpuVertex>().is_multiple_of(4)
);

/// Memory the GPU reads, freed when dropped.
struct Buffer {
    pointer: NonNull<GpuVertex>,
    length: usize,
}

// The buffer is only written before the GPU reads it
unsafe impl Send for Buffer {}

impl Buffer {
    // Zeroed, so the vertices are valid before they are written
    fn new(length: usize) -> Self {
        let size = length.max(1) * size_of::<GpuVertex>();
        let pointer = unsafe { memalign(GX2_VERTEX_BUFFER_ALIGNMENT, size) };
        let pointer = NonNull::new(pointer.cast::<GpuVertex>()).expect("Out of memory");
        unsafe { ptr::write_bytes(pointer.as_ptr().cast::<u8>(), 0, size) };
        Self { pointer, length }
    }

    fn slice(&mut self) -> &mut [GpuVertex] {
        unsafe { core::slice::from_raw_parts_mut(self.pointer.as_ptr(), self.length) }
    }

    // The GPU reads vertex buffers from their aligned start, draws give their first vertex
    unsafe fn bind(&self) {
        unsafe {
            GX2SetAttribBuffer(
                0,
                (self.length * size_of::<GpuVertex>()) as u32,
                size_of::<GpuVertex>() as u32,
                self.pointer.as_ptr().cast(),
            );
        }
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        unsafe { free(self.pointer.as_ptr().cast()) }
    }
}

/// Quads of one tile.
struct Draw {
    tile: usize,
    vertices: Range<usize>,
}

pub struct Mesh {
    buffer: Buffer,
    draws: Vec<Draw>,
    /// The draws of each section and pass.
    sections: [[Range<usize>; 3]; SECTION_COUNT],
}

/// The colors of the light palette during the day and at night.
struct Palettes {
    day: Vec<[f32; 4]>,
    night: Vec<[f32; 4]>,
    srgb: [f32; 256],
}

static PALETTES: AtomicPtr<Palettes> = AtomicPtr::new(ptr::null_mut());

// The palettes, made once for all chunks, two threads that both make them keep one
fn palettes() -> &'static Palettes {
    let mut palettes = PALETTES.load(Ordering::Acquire);
    if palettes.is_null() {
        let made = Box::into_raw(Box::new(Palettes {
            day: linear_palette(1.0),
            night: linear_palette(NIGHT_DAYLIGHT),
            srgb: core::array::from_fn(|value| linear_rgb([value as u8; 3])[0]),
        }));
        palettes = match PALETTES.compare_exchange(
            ptr::null_mut(),
            made,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => made,
            Err(existing) => {
                drop(unsafe { Box::from_raw(made) });
                existing
            }
        };
    }
    unsafe { &*palettes }
}

const fn pass_index(pass: Pass) -> usize {
    match pass {
        Pass::Solid => 0,
        Pass::Cutout => 1,
        Pass::Translucent => 2,
    }
}

fn linear_color(color: u32, srgb: &[f32; 256]) -> [f32; 4] {
    let [r, g, b, a] = color.to_be_bytes();
    [
        srgb[usize::from(r)],
        srgb[usize::from(g)],
        srgb[usize::from(b)],
        f32::from(a) / 255.0,
    ]
}

pub struct Gx2 {
    group: WHBGfxShaderGroup,
    projection: u32,
    model_view: u32,
    params: u32,
    fog_color: u32,
    fog_params: u32,
    tint: u32,
    water: u32,
    scroll: u32,
    textures: Vec<GX2Texture>,
    tiles: GX2Sampler,
    repeat: GX2Sampler,
    clamp: GX2Sampler,
    stream: Buffer,
    used: usize,
    size: [f32; 2],
    night: f32,
    time: f32,
    bound: Option<Texture>,
}

impl Gx2 {
    pub fn new() -> Option<Self> {
        unsafe {
            if WHBGfxInit() == FALSE {
                return None;
            }
            let mut group = MaybeUninit::<WHBGfxShaderGroup>::zeroed().assume_init();
            if WHBGfxLoadGFDShaderGroup(&mut group, 0, SHADER.0.as_ptr().cast()) == FALSE {
                return None;
            }
            for (name, offset, format) in [
                (
                    c"aPosition",
                    offset_of!(GpuVertex, position),
                    GX2_ATTRIB_FORMAT_FLOAT_32_32_32,
                ),
                (
                    c"aTexCoord",
                    offset_of!(GpuVertex, uv),
                    GX2_ATTRIB_FORMAT_FLOAT_32_32,
                ),
                (
                    c"aColor",
                    offset_of!(GpuVertex, color),
                    GX2_ATTRIB_FORMAT_FLOAT_32_32_32_32,
                ),
                (
                    c"aNight",
                    offset_of!(GpuVertex, night),
                    GX2_ATTRIB_FORMAT_FLOAT_32_32_32_32,
                ),
            ] {
                WHBGfxInitShaderAttribute(&mut group, name.as_ptr(), 0, offset as u32, format);
            }
            WHBGfxInitFetchShader(&mut group);
            let projection = GX2GetVertexUniformVar(group.vertex_shader, c"uProjection")?.offset;
            let model_view = GX2GetVertexUniformVar(group.vertex_shader, c"uModelView")?.offset;
            let params = GX2GetVertexUniformVar(group.vertex_shader, c"uParams")?.offset;
            let fog_color = GX2GetPixelUniformVar(group.pixel_shader, c"uFogColor")?.offset;
            let fog_params = GX2GetPixelUniformVar(group.pixel_shader, c"uFogParams")?.offset;
            let tint = GX2GetPixelUniformVar(group.pixel_shader, c"uTint")?.offset;
            let water = GX2GetPixelUniformVar(group.pixel_shader, c"uWater")?.offset;
            let scroll = GX2GetVertexUniformVar(group.vertex_shader, c"uScroll")?.offset;

            let mut textures = Vec::with_capacity(textures::COUNT);
            for index in 0..textures::COUNT {
                let mut texture = MaybeUninit::<GX2Texture>::zeroed().assume_init();
                if GFDGetTextureInPlace(&mut texture, index as u32, TEXTURES.0.as_ptr().cast())
                    == FALSE
                {
                    return None;
                }
                texture.surface.format = if texture.surface.format == 0x31 {
                    GX2_SURFACE_FORMAT_SRGB_BC1
                } else {
                    GX2_SURFACE_FORMAT_SRGB_R8_G8_B8_A8
                };
                GX2InitTextureRegs(&mut texture);
                textures.push(texture);
            }
            let sampler = |clamp, filter, mipmaps| {
                let mut sampler = GX2Sampler::default();
                GX2InitSampler(&mut sampler, clamp, filter);
                if mipmaps {
                    // Match desktop and GX: nearest magnification, linear minification.
                    GX2InitSamplerXYFilter(
                        &mut sampler,
                        GX2_TEX_XY_FILTER_MODE_POINT,
                        GX2_TEX_XY_FILTER_MODE_LINEAR,
                        GX2_TEX_ANISO_RATIO_NONE,
                    );
                    GX2InitSamplerZMFilter(
                        &mut sampler,
                        GX2_TEX_Z_FILTER_MODE_POINT,
                        GX2_TEX_MIP_FILTER_MODE_LINEAR,
                    );
                }
                sampler
            };
            let buffer = WHBGfxGetTVColourBuffer();
            let surface = &mut (*buffer).surface;
            surface.format = GX2_SURFACE_FORMAT_SRGB_R8_G8_B8_A8;
            let aspect = surface.width as f32 / surface.height as f32;
            GX2InitColorBufferRegs(buffer);
            Some(Self {
                group,
                projection,
                model_view,
                params,
                fog_color,
                fog_params,
                tint,
                water,
                scroll,
                textures,
                // Pixel art stays sharp up close and blends between mipmaps in the distance
                tiles: sampler(GX2_TEX_CLAMP_MODE_WRAP, GX2_TEX_XY_FILTER_MODE_POINT, true),
                repeat: sampler(
                    GX2_TEX_CLAMP_MODE_WRAP,
                    GX2_TEX_XY_FILTER_MODE_LINEAR,
                    false,
                ),
                clamp: sampler(
                    GX2_TEX_CLAMP_MODE_CLAMP,
                    GX2_TEX_XY_FILTER_MODE_LINEAR,
                    false,
                ),
                stream: Buffer::new(STREAM_SIZE / size_of::<GpuVertex>()),
                used: 0,
                size: [480.0 * aspect, 480.0],
                night: 0.0,
                time: 0.0,
                bound: None,
            })
        }
    }

    fn texture(&mut self, texture: Texture) {
        if self.bound == Some(texture) {
            return;
        }
        self.bound = Some(texture);
        let (index, sampler) = match texture {
            Texture::Tile(tile) => (textures::TILES + tile, &self.tiles),
            Texture::Sky(layer) => (textures::SKY + layer, &self.repeat),
            Texture::Character(part) => (textures::CHARACTERS + part, &self.clamp),
            Texture::Font => (textures::FONT, &self.clamp),
            Texture::Cursor(player) => (textures::CURSORS + player, &self.clamp),
            Texture::None => (textures::WHITE, &self.clamp),
        };
        unsafe {
            GX2SetPixelTexture(&self.textures[index], 0);
            GX2SetPixelSampler(sampler, 0);
        }
    }

    fn water(&self, enabled: bool) {
        let water = f32::from(u8::from(enabled));
        let scroll = [
            self.time * 0.007_812_5 * water,
            self.time * 0.003_906_25 * water,
            0.0,
            0.0,
        ];
        let params = [water, 0.75, 0.0, 0.62];
        unsafe {
            GX2SetVertexUniformReg(self.scroll, 4, scroll.as_ptr().cast());
            GX2SetPixelUniformReg(self.water, 4, params.as_ptr().cast());
        }
    }

    // Writes vertices to the stream buffer and draws them
    fn draw(&mut self, mode: u32, texture: Texture, vertices: &[Vertex]) {
        self.water(false);
        let range = self.used..self.used + vertices.len();
        if range.end > self.stream.length {
            return;
        }
        self.used = range.end;
        self.texture(texture);
        let stream = &mut self.stream.slice()[range.clone()];
        let srgb = &palettes().srgb;
        for (target, vertex) in stream.iter_mut().zip(vertices) {
            let color = linear_color(vertex.color, srgb);
            *target = GpuVertex {
                position: vertex.position,
                color,
                uv: vertex.uv,
                night: color,
            };
        }
        unsafe {
            GX2Invalidate(
                GX2_INVALIDATE_MODE_CPU_ATTRIBUTE_BUFFER,
                stream.as_mut_ptr().cast::<c_void>(),
                size_of_val(stream) as u32,
            );
            self.stream.bind();
            GX2DrawEx(mode, vertices.len() as u32, range.start as u32, 1);
        }
    }
}

impl Gpu for Gx2 {
    type Mesh = Mesh;

    fn mesh(source: &mesh::Mesh) -> Mesh {
        let mut mesh = Mesh {
            buffer: Buffer::new(source.vertices.len()),
            draws: Vec::with_capacity(source.groups.len()),
            sections: Default::default(),
        };
        let palettes = palettes();
        let vertices = mesh.buffer.slice();
        for (target, vertex) in vertices.iter_mut().zip(&source.vertices) {
            let light = usize::from(vertex.light);
            *target = GpuVertex {
                position: vertex.position,
                color: palettes.day[light],
                uv: vertex.uv,
                night: palettes.night[light],
            };
        }
        memory::flush(vertices);
        for group in &source.groups {
            let draws = &mut mesh.sections[group.section][pass_index(group.pass)];
            if draws.start == draws.end {
                *draws = mesh.draws.len()..mesh.draws.len();
            }
            draws.end += 1;
            mesh.draws.push(Draw {
                tile: group.tile,
                vertices: group.vertices.clone(),
            });
        }
        mesh
    }

    fn has_alpha(tile: usize) -> bool {
        textures::HAS_ALPHA[tile]
    }

    fn draws(mesh: &Mesh, section: usize, pass: Pass) -> bool {
        let draws = &mesh.sections[section][pass_index(pass)];
        draws.start != draws.end
    }

    fn size(&self) -> [f32; 2] {
        self.size
    }

    fn begin(&mut self, clear: [u8; 3], daylight: f32, time: f32) {
        let [r, g, b] = linear_rgb(clear);
        self.night = if daylight < 0.5 { 1.0 } else { 0.0 };
        self.time = time;
        self.used = 0;
        self.bound = None;
        unsafe {
            WHBGfxBeginRender();
            WHBGfxBeginRenderTV();
            // Clearing sets the TV's context state, so the shaders follow
            WHBGfxClearColor(r, g, b, 1.0);
            // libwhb restores its original context after clearing; rebind the sRGB target.
            GX2SetColorBuffer(WHBGfxGetTVColourBuffer(), GX2_RENDER_TARGET_0);
            GX2SetFetchShader(&self.group.fetch_shader);
            GX2SetVertexShader(self.group.vertex_shader);
            GX2SetPixelShader(self.group.pixel_shader);
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
            GX2SetLineWidth(3.0);
            GX2Invalidate(INVALIDATE_ATTRIBUTE_BUFFER, ptr::null_mut(), u32::MAX);
        }
        let params = [self.night, 0.0, 0.0, 0.0];
        unsafe { GX2SetVertexUniformReg(self.params, 4, params.as_ptr().cast()) };
        self.water(false);
        self.fog(None);
    }

    fn viewport(&mut self, [x, y, width, height]: [f32; 4]) {
        unsafe {
            let surface = &(*WHBGfxGetTVColourBuffer()).surface;
            let fw = surface.width as f32;
            let fh = surface.height as f32;
            GX2SetViewport(x * fw, y * fh, width * fw, height * fh, 0.0, 1.0);
            GX2SetScissor(
                (x * fw) as u32,
                (y * fh) as u32,
                (width * fw) as u32,
                (height * fh) as u32,
            );
        }
    }

    fn projection(&mut self, matrix: &Mtx44, _orthographic: bool) {
        unsafe { GX2SetVertexUniformReg(self.projection, 16, matrix.as_ptr().cast()) };
    }

    fn model_view(&mut self, matrix: &Mtx) {
        unsafe { GX2SetVertexUniformReg(self.model_view, 12, matrix.as_ptr().cast()) };
    }

    fn state(&mut self, state: State) {
        let func = if state.depth_test {
            GX2_COMPARE_FUNC_LEQUAL
        } else {
            GX2_COMPARE_FUNC_ALWAYS
        };
        let flag = |value: bool| if value { TRUE } else { FALSE };
        unsafe {
            GX2SetDepthOnlyControl(flag(state.depth_test), flag(state.depth_write), func);
            // The quads turn clockwise for their front like on GX
            GX2SetCullOnlyControl(GX2_FRONT_FACE_CW, FALSE, flag(state.cull));
            GX2SetColorControl(GX2_LOGIC_OP_COPY, u8::from(state.blend), FALSE, TRUE);
            // Test the exported alpha before cutouts write color or depth.
            GX2SetAlphaTest(flag(state.cutout), GX2_COMPARE_FUNC_GREATER, 0.5);
        }
    }

    fn fog(&mut self, fog: Option<Fog>) {
        let (params, color, tint) = fog.map_or(([0.0; 4], [0; 3], [1.0; 3]), |fog| {
            let [start, end] = fog.distance;
            let scale = 1.0 / (end - start);
            (
                [scale, -start * scale, fog.density, 1.0],
                fog.color,
                fog.tint,
            )
        });
        let [r, g, b] = linear_rgb(color);
        let color = [r, g, b, 1.0];
        let tint = [tint[0], tint[1], tint[2], 1.0];
        unsafe {
            GX2SetPixelUniformReg(self.fog_color, 4, color.as_ptr().cast());
            GX2SetPixelUniformReg(self.fog_params, 4, params.as_ptr().cast());
            GX2SetPixelUniformReg(self.tint, 4, tint.as_ptr().cast());
        }
    }

    fn quads(&mut self, texture: Texture, vertices: &[Vertex]) {
        self.draw(GX2_PRIMITIVE_MODE_QUADS, texture, vertices);
    }

    fn lines(&mut self, vertices: &[Vertex]) {
        self.draw(GX2_PRIMITIVE_MODE_LINES, Texture::None, vertices);
    }

    fn chunk(&mut self, mesh: &Mesh, section: usize, pass: Pass) {
        unsafe { mesh.buffer.bind() };
        for draw in &mesh.draws[mesh.sections[section][pass_index(pass)].clone()] {
            self.water(
                pass == Pass::Translucent
                    && draw.tile
                        == plaatcraft_engine::tiles::tile(plaatcraft_engine::world::WATER, 2),
            );
            self.texture(Texture::Tile(draw.tile));
            unsafe {
                GX2DrawEx(
                    GX2_PRIMITIVE_MODE_QUADS,
                    draw.vertices.len() as u32,
                    draw.vertices.start as u32,
                    1,
                );
            }
        }
    }

    fn present(&mut self) {
        unsafe {
            // libwhb's scan buffers are UNORM. Copy the stored sRGB bytes through an UNORM
            // view, so presentation does not decode them to linear and darken the image.
            let mut scanout = *WHBGfxGetTVColourBuffer();
            scanout.surface.format = GX2_SURFACE_FORMAT_UNORM_R8_G8_B8_A8;
            GX2InitColorBufferRegs(&mut scanout);
            GX2CopyColorBufferToScanBuffer(&scanout, GX2_SCAN_TARGET_TV);
            // The GamePad shows the TV image scaled down, instead of drawing the scene twice
            GX2CopyColorBufferToScanBuffer(&scanout, GX2_SCAN_TARGET_DRC);
            WHBGfxFinishRender();
        }
    }
}

impl Drop for Gx2 {
    fn drop(&mut self) {
        unsafe {
            WHBGfxFreeShaderGroup(&mut self.group);
            WHBGfxShutdown();
        }
    }
}
