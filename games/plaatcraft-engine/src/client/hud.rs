/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! 2D drawing over the world: text, rectangles, block icons, the hotbar and the menu.

use super::font;
use super::platform::{Gpu, State, Texture, Vertex};
use super::render::ortho;
use crate::hotbar::{Hotbar, SLOTS};
#[cfg(not(feature = "std"))]
use crate::math::Float;
use crate::menu::{Align, Canvas, Color, Font};
use crate::tiles::tile;
use crate::world;

pub(super) const WHITE: u32 = 0xffff_ffff;
const SLOT: f32 = 44.0;
const MARGIN: f32 = 12.0;

pub(super) struct Hud<'a, G> {
    gpu: &'a mut G,
    pub(super) width: f32,
    pub(super) height: f32,
}

impl<'a, G: Gpu> Hud<'a, G> {
    pub(super) fn begin(gpu: &'a mut G) -> Self {
        let [width, height] = gpu.size();
        gpu.projection(&ortho(width, height), true);
        gpu.model_view(&[
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ]);
        gpu.state(State {
            blend: true,
            ..State::default()
        });
        gpu.fog(None);
        Self { gpu, width, height }
    }

    // A quad with corners clockwise from the top left, each with a color and texture coordinate
    fn quad(
        &mut self,
        texture: Texture,
        corners: [[f32; 2]; 4],
        uvs: [[f32; 2]; 4],
        colors: [u32; 4],
    ) {
        let vertices: [Vertex; 4] = core::array::from_fn(|i| Vertex {
            position: [corners[i][0], corners[i][1], 0.0],
            color: colors[i],
            uv: uvs[i],
        });
        self.gpu.quads(texture, &vertices);
    }

    pub(super) fn rect(&mut self, x: f32, y: f32, width: f32, height: f32, color: u32) {
        self.quad(
            Texture::None,
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

    pub(super) fn frame(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        border: f32,
        color: u32,
    ) {
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

    pub(super) fn text(&mut self, text: &str, mut x: f32, y: f32, size: f32, color: u32) {
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
                Texture::Font,
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
    pub(super) fn label(&mut self, text: &str, x: f32, y: f32, size: f32) {
        self.text(text, x + 1.5, y + 1.5, size, 0x0000_00c0);
        self.text(text, x, y, size, WHITE);
    }

    /// A block as an isometric cube, plants as their flat sprite.
    pub(super) fn block(&mut self, material: u8, x: f32, y: f32, size: f32) {
        let uvs = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        if world::is_passable(material) && !matches!(material, 22 | 24) {
            self.quad(
                Texture::Tile(tile(material, 6)),
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
                self.quad(
                    Texture::Tile(tile(material, face)),
                    points.map(project),
                    uvs,
                    [shade; 4],
                );
            }
        }
    }

    pub(super) fn crosshair(&mut self) {
        let [x, y] = [self.width / 2.0, self.height / 2.0];
        self.rect(x - 10.0, y - 1.5, 20.0, 3.0, 0xffff_ffd0);
        self.rect(x - 1.5, y - 10.0, 3.0, 20.0, 0xffff_ffd0);
    }

    pub(super) fn hotbar(&mut self, hotbar: &Hotbar) -> f32 {
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
    pub(super) fn cursor(&mut self, [x, y, angle]: [f32; 3]) {
        // The cursor image's tip is at its center, so it turns around the pointer
        let (sin, cos) = angle.to_radians().sin_cos();
        let corner = |dx: f32, dy: f32| [x + dx * cos - dy * sin, y + dx * sin + dy * cos];
        self.quad(
            Texture::Cursor,
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
impl<G: Gpu> Canvas for Hud<'_, G> {
    fn rect(&mut self, x: f32, y: f32, width: f32, height: f32, color: Color) {
        Hud::rect(self, x, y, width, height, u32::from_be_bytes(color));
    }

    fn text(&mut self, text: &str, x: f32, y: f32, font: Font) {
        let left = match font.align {
            Align::Left => x,
            Align::Center => x - text_width(text, font.size) / 2.0,
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

pub(super) fn text_width(text: &str, size: f32) -> f32 {
    let scale = size / font::RENDER_SIZE;
    text.chars().map(|ch| advance(ch) * scale).sum()
}

/// The hotbar slot at a position of the 2D screen space.
pub(super) fn hotbar_slot([width, height]: [f32; 2], [x, y]: [f32; 2]) -> Option<usize> {
    let left = (width - SLOT * SLOTS as f32) / 2.0;
    let top = height - SLOT - MARGIN;
    let slot = ((x - left) / SLOT).floor();
    ((top..top + SLOT).contains(&y) && (0.0..SLOTS as f32).contains(&slot)).then_some(slot as usize)
}

// How far a character moves the text at the font's size, a tab is four spaces.
fn advance(ch: char) -> f32 {
    match font::glyph(ch) {
        Some(glyph) => f32::from(glyph.w) + 2.0,
        None if ch == '\t' => font::SPACE_WIDTH * 4.0,
        None => font::SPACE_WIDTH,
    }
}
