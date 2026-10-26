/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The glyphs of the white font atlas in assets/font.png, baked at 64 pixels.

pub struct Glyph {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    /// The distance from the top of the line to the top of the glyph.
    pub a: i16,
}

pub const RENDER_SIZE: f32 = 64.0;
pub const ATLAS_SIZE: f32 = 420.0;
pub const SPACE_WIDTH: f32 = 16.0;

/// The glyph of a character, the atlas has printable ASCII from '!' to '~'.
pub fn glyph(ch: char) -> Option<&'static Glyph> {
    GLYPHS.get((ch as usize).checked_sub('!' as usize)?)
}

#[rustfmt::skip]
static GLYPHS: [Glyph; 94] = [
    Glyph { x: 0, y: 0, w: 9, h: 46, a: 6 },
    Glyph { x: 9, y: 0, w: 19, h: 17, a: 6 },
    Glyph { x: 28, y: 0, w: 37, h: 48, a: 5 },
    Glyph { x: 65, y: 0, w: 33, h: 57, a: 2 },
    Glyph { x: 98, y: 0, w: 52, h: 49, a: 5 },
    Glyph { x: 150, y: 0, w: 41, h: 48, a: 5 },
    Glyph { x: 191, y: 0, w: 9, h: 17, a: 6 },
    Glyph { x: 200, y: 0, w: 18, h: 61, a: 5 },
    Glyph { x: 218, y: 0, w: 18, h: 61, a: 5 },
    Glyph { x: 236, y: 0, w: 23, h: 20, a: 5 },
    Glyph { x: 259, y: 0, w: 33, h: 31, a: 14 },
    Glyph { x: 292, y: 0, w: 9, h: 16, a: 46 },
    Glyph { x: 301, y: 0, w: 20, h: 6, a: 33 },
    Glyph { x: 321, y: 0, w: 9, h: 7, a: 46 },
    Glyph { x: 330, y: 0, w: 20, h: 48, a: 5 },
    Glyph { x: 350, y: 0, w: 32, h: 47, a: 6 },
    Glyph { x: 382, y: 0, w: 19, h: 46, a: 6 },
    Glyph { x: 0, y: 61, w: 33, h: 46, a: 6 },
    Glyph { x: 33, y: 61, w: 32, h: 47, a: 6 },
    Glyph { x: 65, y: 61, w: 34, h: 46, a: 6 },
    Glyph { x: 99, y: 61, w: 33, h: 46, a: 7 },
    Glyph { x: 132, y: 61, w: 33, h: 47, a: 6 },
    Glyph { x: 165, y: 61, w: 32, h: 46, a: 7 },
    Glyph { x: 197, y: 61, w: 33, h: 47, a: 6 },
    Glyph { x: 230, y: 61, w: 33, h: 47, a: 6 },
    Glyph { x: 263, y: 61, w: 9, h: 34, a: 19 },
    Glyph { x: 272, y: 61, w: 9, h: 43, a: 19 },
    Glyph { x: 281, y: 61, w: 33, h: 32, a: 14 },
    Glyph { x: 314, y: 61, w: 33, h: 20, a: 20 },
    Glyph { x: 347, y: 61, w: 33, h: 32, a: 14 },
    Glyph { x: 380, y: 61, w: 32, h: 47, a: 5 },
    Glyph { x: 0, y: 108, w: 62, h: 61, a: 5 },
    Glyph { x: 62, y: 108, w: 45, h: 46, a: 6 },
    Glyph { x: 107, y: 108, w: 37, h: 46, a: 6 },
    Glyph { x: 144, y: 108, w: 43, h: 48, a: 5 },
    Glyph { x: 187, y: 108, w: 40, h: 46, a: 6 },
    Glyph { x: 227, y: 108, w: 37, h: 46, a: 6 },
    Glyph { x: 264, y: 108, w: 33, h: 46, a: 6 },
    Glyph { x: 297, y: 108, w: 45, h: 48, a: 5 },
    Glyph { x: 342, y: 108, w: 38, h: 46, a: 6 },
    Glyph { x: 380, y: 108, w: 9, h: 46, a: 6 },
    Glyph { x: 389, y: 108, w: 28, h: 47, a: 6 },
    Glyph { x: 0, y: 169, w: 40, h: 46, a: 6 },
    Glyph { x: 40, y: 169, w: 31, h: 46, a: 6 },
    Glyph { x: 71, y: 169, w: 46, h: 46, a: 6 },
    Glyph { x: 117, y: 169, w: 39, h: 46, a: 6 },
    Glyph { x: 156, y: 169, w: 46, h: 48, a: 5 },
    Glyph { x: 202, y: 169, w: 37, h: 46, a: 6 },
    Glyph { x: 239, y: 169, w: 47, h: 51, a: 5 },
    Glyph { x: 286, y: 169, w: 43, h: 46, a: 6 },
    Glyph { x: 329, y: 169, w: 39, h: 48, a: 5 },
    Glyph { x: 368, y: 169, w: 39, h: 46, a: 6 },
    Glyph { x: 0, y: 220, w: 39, h: 47, a: 6 },
    Glyph { x: 39, y: 220, w: 44, h: 46, a: 6 },
    Glyph { x: 83, y: 220, w: 61, h: 46, a: 6 },
    Glyph { x: 144, y: 220, w: 44, h: 46, a: 6 },
    Glyph { x: 188, y: 220, w: 44, h: 46, a: 6 },
    Glyph { x: 232, y: 220, w: 39, h: 46, a: 6 },
    Glyph { x: 271, y: 220, w: 15, h: 59, a: 6 },
    Glyph { x: 286, y: 220, w: 20, h: 48, a: 5 },
    Glyph { x: 306, y: 220, w: 15, h: 59, a: 6 },
    Glyph { x: 321, y: 220, w: 29, h: 26, a: 5 },
    Glyph { x: 350, y: 220, w: 40, h: 5, a: 61 },
    Glyph { x: 390, y: 220, w: 14, h: 9, a: 6 },
    Glyph { x: 0, y: 279, w: 33, h: 35, a: 18 },
    Glyph { x: 33, y: 279, w: 31, h: 47, a: 6 },
    Glyph { x: 64, y: 279, w: 31, h: 35, a: 18 },
    Glyph { x: 95, y: 279, w: 31, h: 47, a: 6 },
    Glyph { x: 126, y: 279, w: 33, h: 35, a: 18 },
    Glyph { x: 159, y: 279, w: 22, h: 47, a: 5 },
    Glyph { x: 181, y: 279, w: 32, h: 48, a: 18 },
    Glyph { x: 213, y: 279, w: 30, h: 46, a: 6 },
    Glyph { x: 243, y: 279, w: 8, h: 46, a: 6 },
    Glyph { x: 251, y: 279, w: 15, h: 60, a: 6 },
    Glyph { x: 266, y: 279, w: 30, h: 46, a: 6 },
    Glyph { x: 296, y: 279, w: 8, h: 46, a: 6 },
    Glyph { x: 304, y: 279, w: 47, h: 34, a: 18 },
    Glyph { x: 351, y: 279, w: 29, h: 34, a: 18 },
    Glyph { x: 380, y: 279, w: 34, h: 35, a: 18 },
    Glyph { x: 0, y: 339, w: 31, h: 47, a: 18 },
    Glyph { x: 31, y: 339, w: 31, h: 47, a: 18 },
    Glyph { x: 62, y: 339, w: 21, h: 34, a: 18 },
    Glyph { x: 83, y: 339, w: 30, h: 35, a: 18 },
    Glyph { x: 113, y: 339, w: 19, h: 46, a: 7 },
    Glyph { x: 132, y: 339, w: 29, h: 34, a: 19 },
    Glyph { x: 161, y: 339, w: 33, h: 34, a: 19 },
    Glyph { x: 194, y: 339, w: 48, h: 34, a: 19 },
    Glyph { x: 242, y: 339, w: 34, h: 34, a: 19 },
    Glyph { x: 276, y: 339, w: 33, h: 47, a: 19 },
    Glyph { x: 309, y: 339, w: 32, h: 34, a: 19 },
    Glyph { x: 341, y: 339, w: 21, h: 61, a: 5 },
    Glyph { x: 362, y: 339, w: 7, h: 61, a: 5 },
    Glyph { x: 369, y: 339, w: 21, h: 61, a: 5 },
    Glyph { x: 0, y: 400, w: 34, h: 11, a: 24 },
];
