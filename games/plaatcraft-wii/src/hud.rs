/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! 2D drawing over the world: text, rectangles, block icons, the hotbar and the menu.

use libogc_sys::*;
use plaatcraft_engine::hotbar::{Hotbar, SLOTS};
use plaatcraft_engine::menu::{Align, Canvas, Color, Font};
use plaatcraft_engine::tiles::tile;
use plaatcraft_engine::world;

use crate::font;
use crate::render::{Display, pipe_f32, pipe_u32, set_vertex_color_texture};
use crate::textures::Textures;

pub const WHITE: u32 = 0xffff_ffff;
const SLOT: f32 = 44.0;
const MARGIN: f32 = 12.0;

pub struct Hud<'a> {
    textures: &'a Textures,
    pub width: f32,
    pub height: f32,
    textured: Option<bool>,
}

impl<'a> Hud<'a> {
    pub fn begin(display: &Display, textures: &'a Textures) -> Self {
        let mut projection = [[0.0; 4]; 4];
        let mut identity = [[0.0; 4]; 3];
        unsafe {
            guOrtho(
                &mut projection,
                0.0,
                display.height,
                0.0,
                display.width,
                0.0,
                1.0,
            );
            GX_LoadProjectionMtx(&projection, GX_ORTHOGRAPHIC);
            ps_guMtxIdentity(&mut identity);
            GX_LoadPosMtxImm(&identity, GX_PNMTX0);
            GX_SetZMode(GX_DISABLE, GX_ALWAYS, GX_FALSE);
            GX_SetCullMode(GX_CULL_NONE);
            GX_SetBlendMode(GX_BM_BLEND, GX_BL_SRCALPHA, GX_BL_INVSRCALPHA, GX_LO_CLEAR);
            GX_SetAlphaCompare(GX_ALWAYS, 0, GX_AOP_AND, GX_ALWAYS, 0);
            GX_ClearVtxDesc();
            GX_SetVtxDesc(GX_VA_POS, GX_DIRECT);
            GX_SetVtxDesc(GX_VA_CLR0, GX_DIRECT);
            GX_SetVtxDesc(GX_VA_TEX0, GX_DIRECT);
            GX_SetVtxAttrFmt(GX_VTXFMT2, GX_VA_POS as u32, GX_POS_XY, GX_F32, 0);
            GX_SetVtxAttrFmt(GX_VTXFMT2, GX_VA_CLR0 as u32, GX_CLR_RGBA, GX_RGBA8, 0);
            GX_SetVtxAttrFmt(GX_VTXFMT2, GX_VA_TEX0 as u32, GX_TEX_ST, GX_F32, 0);
        }
        Self {
            textures,
            width: display.width,
            height: display.height,
            textured: None,
        }
    }

    fn texture(&mut self, texture: Option<&GXTexObj>) {
        if self.textured != Some(texture.is_some()) {
            self.textured = Some(texture.is_some());
            set_vertex_color_texture(texture.is_some());
        }
        if let Some(texture) = texture {
            unsafe { GX_LoadTexObj(texture, GX_TEXMAP0 as u8) };
        }
    }

    // A quad with corners clockwise from the top left, each with a color and texture coordinate
    fn quad(&mut self, corners: [[f32; 2]; 4], uvs: [[f32; 2]; 4], colors: [u32; 4]) {
        unsafe {
            GX_Begin(GX_QUADS, GX_VTXFMT2, 4);
            for ((corner, uv), color) in corners.into_iter().zip(uvs).zip(colors) {
                pipe_f32(corner[0]);
                pipe_f32(corner[1]);
                pipe_u32(color);
                pipe_f32(uv[0]);
                pipe_f32(uv[1]);
            }
        }
    }

    pub fn rect(&mut self, x: f32, y: f32, width: f32, height: f32, color: u32) {
        self.texture(None);
        self.quad(
            [
                [x, y],
                [x + width, y],
                [x + width, y + height],
                [x, y + height],
            ],
            [[0.0; 2]; 4],
            [color; 4],
        );
    }

    pub fn frame(&mut self, x: f32, y: f32, width: f32, height: f32, border: f32, color: u32) {
        self.rect(x, y, width, border, color);
        self.rect(x, y + height - border, width, border, color);
        self.rect(x, y + border, border, height - border * 2.0, color);
        self.rect(
            x + width - border,
            y + border,
            border,
            height - border * 2.0,
            color,
        );
    }

    pub fn text_width(text: &str, size: f32) -> f32 {
        let scale = size / font::RENDER_SIZE;
        text.chars().map(|ch| advance(ch) * scale).sum()
    }

    pub fn text(&mut self, text: &str, mut x: f32, y: f32, size: f32, color: u32) {
        let textures = self.textures;
        self.texture(Some(&textures.font));
        let scale = size / font::RENDER_SIZE;
        for ch in text.chars() {
            let Some(glyph) = font::glyph(ch) else {
                x += advance(ch) * scale;
                continue;
            };
            let [left, top] = [glyph.x, glyph.y].map(|v| f32::from(v) / font::ATLAS_SIZE);
            let [right, bottom] =
                [glyph.x + glyph.w, glyph.y + glyph.h].map(|v| f32::from(v) / font::ATLAS_SIZE);
            let width = f32::from(glyph.w) * scale;
            let height = f32::from(glyph.h) * scale;
            let top_y = y + f32::from(glyph.a) * scale;
            self.quad(
                [
                    [x, top_y],
                    [x + width, top_y],
                    [x + width, top_y + height],
                    [x, top_y + height],
                ],
                [[left, top], [right, top], [right, bottom], [left, bottom]],
                [color; 4],
            );
            x += (f32::from(glyph.w) + 2.0) * scale;
        }
    }

    /// Text with a dark shadow, readable over the bright sky.
    pub fn label(&mut self, text: &str, x: f32, y: f32, size: f32) {
        self.text(text, x + 1.5, y + 1.5, size, 0x0000_00c0);
        self.text(text, x, y, size, WHITE);
    }

    /// A block as an isometric cube, plants as their flat sprite.
    pub fn block(&mut self, material: u8, x: f32, y: f32, size: f32) {
        let textures = self.textures;
        let uvs = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        if plaatcraft_engine::world::is_passable(material) && !matches!(material, 22 | 24) {
            self.texture(Some(&textures.tiles[tile(material, 6)]));
            self.quad(
                [[x, y], [x + size, y], [x + size, y + size], [x, y + size]],
                uvs,
                [WHITE; 4],
            );
            return;
        }
        let full = [[[0.0; 3], [1.0; 3]]];
        let boxes = world::boxes(material);
        let boxes = if boxes.is_empty() { &full[..] } else { boxes };
        let project = |p: [f64; 3]| {
            [
                x + size * (0.5 + (p[0] - p[2]) as f32 * 0.5),
                y + size * (0.5 + (p[0] + p[2]) as f32 * 0.25 - p[1] as f32 * 0.5),
            ]
        };
        for &[lo, hi] in boxes {
            for (face, points, shade) in [
                (
                    2,
                    [
                        [lo[0], hi[1], hi[2]],
                        [lo[0], hi[1], lo[2]],
                        [hi[0], hi[1], lo[2]],
                        [hi[0], hi[1], hi[2]],
                    ],
                    WHITE,
                ),
                (
                    0,
                    [
                        [hi[0], hi[1], hi[2]],
                        [hi[0], hi[1], lo[2]],
                        [hi[0], lo[1], lo[2]],
                        [hi[0], lo[1], hi[2]],
                    ],
                    0xcccc_ccff,
                ),
                (
                    4,
                    [
                        [lo[0], hi[1], hi[2]],
                        [hi[0], hi[1], hi[2]],
                        [hi[0], lo[1], hi[2]],
                        [lo[0], lo[1], hi[2]],
                    ],
                    0xa6a6_a6ff,
                ),
            ] {
                self.texture(Some(&textures.tiles[tile(material, face)]));
                self.quad(points.map(project), uvs, [shade; 4]);
            }
        }
    }

    pub fn crosshair(&mut self) {
        let [x, y] = [self.width / 2.0, self.height / 2.0];
        self.rect(x - 10.0, y - 1.5, 20.0, 3.0, 0xffff_ffd0);
        self.rect(x - 1.5, y - 10.0, 3.0, 20.0, 0xffff_ffd0);
    }

    pub fn hotbar(&mut self, hotbar: &Hotbar) -> f32 {
        let x = (self.width - SLOT * SLOTS as f32) / 2.0;
        let y = self.height - SLOT - MARGIN;
        self.rect(
            x - 2.0,
            y - 2.0,
            SLOT * SLOTS as f32 + 4.0,
            SLOT + 4.0,
            0x0000_0080,
        );
        for (slot, &material) in hotbar.slots.iter().enumerate() {
            let slot_x = x + slot as f32 * SLOT;
            self.frame(slot_x, y, SLOT, SLOT, 2.0, 0x8b8b_8bc0);
            self.block(material, slot_x + 8.0, y + 8.0, SLOT - 16.0);
        }
        self.frame(
            x + hotbar.selected as f32 * SLOT - 2.0,
            y - 2.0,
            SLOT + 4.0,
            SLOT + 4.0,
            3.0,
            WHITE,
        );
        y
    }

    /// The Wii Remote cursor, rolled with the remote like in the Wii menu.
    pub fn cursor(&mut self, [x, y, angle]: [f32; 3]) {
        let textures = self.textures;
        self.texture(Some(&textures.cursor));
        // The cursor image's tip is at its center, so it turns around the pointer
        let (sin, cos) = (sinf(angle.to_radians()), cosf(angle.to_radians()));
        let corner = |dx: f32, dy: f32| [x + dx * cos - dy * sin, y + dx * sin + dy * cos];
        self.quad(
            [
                corner(-24.0, -24.0),
                corner(24.0, -24.0),
                corner(24.0, 24.0),
                corner(-24.0, 24.0),
            ],
            [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            [WHITE; 4],
        );
    }
}

// The engine's menus drawn with the bitmap font, which has no bold, so bold text is drawn twice.
impl Canvas for Hud<'_> {
    fn rect(&mut self, x: f32, y: f32, width: f32, height: f32, color: Color) {
        Hud::rect(self, x, y, width, height, u32::from_be_bytes(color));
    }

    fn text(&mut self, text: &str, x: f32, y: f32, font: Font) {
        let left = match font.align {
            Align::Left => x,
            Align::Center => x - Self::text_width(text, font.size) / 2.0,
        };
        // The font's glyphs hang from the top, the middle of a line is about half its size below it
        let top = y - font.size * 0.55;
        let color = u32::from_be_bytes(font.color);
        Hud::text(self, text, left, top, font.size, color);
        if font.bold {
            Hud::text(self, text, left + font.size / 24.0, top, font.size, color);
        }
    }
}

// How far a character moves the text at the font's size, a tab is four spaces.
fn advance(ch: char) -> f32 {
    match font::glyph(ch) {
        Some(glyph) => f32::from(glyph.w) + 2.0,
        None if ch == '\t' => font::SPACE_WIDTH * 4.0,
        None => font::SPACE_WIDTH,
    }
}
