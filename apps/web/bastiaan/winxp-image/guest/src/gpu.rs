/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The 2D command protocol of the paravirtualized graphics adapter.
//!
//! The display driver writes one command into nonpaged memory and calls [`V86_CMD_GPU`] with its
//! linear address. The page (`web/src/gpu/`) translates the address through the page tables,
//! executes the command before the port access returns and answers [`V86_GPU_DONE`], or
//! [`V86_GPU_PUNT`] without touching a pixel, after which the driver lets the engine draw.
//! `web/src/gpu/protocol.ts` mirrors these constants, a test keeps them equal.
//!
//! Commands are little endian `u32` words: a header with fixed word positions, then arrays.
//! Surfaces are [`SURFACE_WORDS`] words: kind, address (VRAM offset or linear address of row 0),
//! pitch in bytes (negative for bottom up bitmaps), bits per pixel, width and height. Rectangles
//! are left, top, right and bottom, exclusive of right and bottom.

/// EBX = linear address of the command, ESI = its length in bytes; returns a `V86_GPU_*` status
pub const V86_CMD_GPU: u32 = 0x8610;
/// Returns the scanline of the host's 60 Hz timeline, with [`V86_GPU_VBLANK`] in the blank
pub const V86_CMD_GPU_SCANLINE: u32 = 0x8611;

pub const V86_GPU_DONE: u32 = 0;
/// Not supported or memory not present, nothing was drawn
pub const V86_GPU_PUNT: u32 = 1;
pub const V86_GPU_INVALID: u32 = 2;
pub const V86_GPU_VBLANK: u32 = 0x8000_0000;

pub const GPU_SURFACE_VRAM: u32 = 0;
pub const GPU_SURFACE_GUEST: u32 = 1;
pub const SURFACE_WORDS: usize = 6;
pub const RECT_WORDS: usize = 4;

/// Rectangles of `dst` combine with the brush (color or 8x8 pattern) and optionally a source
/// rectangle, stretched when its size differs, by a ROP3. Header words: op, flags, rop, dst
/// surface, src surface, dst rect, src rect, color, source key, destination key, pattern origin x
/// and y, translation count, clip count. Then 64 pattern pixels with [`GPU_BLT_PATTERN`], the
/// translation table (source index to destination pixel, empty for identity) and the clip rects.
pub const GPU_OP_BLT: u32 = 1;
/// Monochrome glyphs in a color, after an optional opaque rectangle. Header words: op, flags, dst
/// surface, fore color, back color, opaque rect, glyph count, clip count. Then glyphs of
/// [`GLYPH_WORDS`] words (x, y, width, height, linear address of the rows, one bit per pixel with
/// rows padded to bytes) and the clip rects.
pub const GPU_OP_TEXT: u32 = 2;
/// 32 bpp source over 32 bpp destination with a constant and optionally per pixel premultiplied
/// alpha, without stretching. Header words: op, flags, constant alpha, dst surface, src surface, dst
/// rect, src rect, clip count. Then the clip rects.
pub const GPU_OP_ALPHA: u32 = 3;

pub const GPU_BLT_SOURCE: u32 = 0x1;
pub const GPU_BLT_PATTERN: u32 = 0x2;
/// Skips source pixels equal to the source key, compared before translation
pub const GPU_BLT_SOURCE_KEY: u32 = 0x4;
/// Writes only where the destination equals the destination key
pub const GPU_BLT_DEST_KEY: u32 = 0x8;
pub const GPU_BLT_MIRROR_X: u32 = 0x10;
pub const GPU_BLT_MIRROR_Y: u32 = 0x20;
pub const GPU_TEXT_OPAQUE: u32 = 0x1;
pub const GPU_ALPHA_PER_PIXEL: u32 = 0x1;

pub const BLT_HEADER_WORDS: usize = 30;
pub const TEXT_HEADER_WORDS: usize = 16;
pub const ALPHA_HEADER_WORDS: usize = 24;
pub const GLYPH_WORDS: usize = 5;
pub const PATTERN_PIXELS: usize = 64;
pub const XLATE_MAX: usize = 256;

pub const ROP_SRCCOPY: u32 = 0xCC;
pub const ROP_PATCOPY: u32 = 0xF0;

/// Whether a ROP3 reads the source
pub const fn rop_uses_source(rop: u32) -> bool {
    ((rop >> 2) ^ rop) & 0x33 != 0
}

/// Whether a ROP3 reads the pattern
pub const fn rop_uses_pattern(rop: u32) -> bool {
    ((rop >> 4) ^ rop) & 0x0F != 0
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    pub const fn is_empty(&self) -> bool {
        self.left >= self.right || self.top >= self.bottom
    }

    pub const fn width(&self) -> i32 {
        self.right - self.left
    }

    pub const fn height(&self) -> i32 {
        self.bottom - self.top
    }

    /// The overlap, empty when there is none
    pub fn intersect(&self, other: &Self) -> Self {
        Self {
            left: self.left.max(other.left),
            top: self.top.max(other.top),
            right: self.right.min(other.right),
            bottom: self.bottom.min(other.bottom),
        }
    }

    /// The same rectangle with left <= right and top <= bottom
    pub fn ordered(&self) -> Self {
        Self {
            left: self.left.min(self.right),
            top: self.top.min(self.bottom),
            right: self.left.max(self.right),
            bottom: self.top.max(self.bottom),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Surface {
    pub kind: u32,
    pub address: u32,
    pub pitch: i32,
    pub bpp: u32,
    pub width: u32,
    pub height: u32,
}

impl Surface {
    pub const fn words(&self) -> [u32; SURFACE_WORDS] {
        [
            self.kind,
            self.address,
            self.pitch as u32,
            self.bpp,
            self.width,
            self.height,
        ]
    }
}

/// Writes a command into a word buffer, every push fails once it is full
pub struct Command<'a> {
    words: &'a mut [u32],
    len: usize,
}

impl<'a> Command<'a> {
    pub fn new(words: &'a mut [u32]) -> Self {
        Self { words, len: 0 }
    }

    pub const fn len(&self) -> usize {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The words that still fit
    pub const fn remaining(&self) -> usize {
        self.words.len() - self.len
    }

    pub fn as_words(&self) -> &[u32] {
        &self.words[..self.len]
    }

    pub fn push(&mut self, word: u32) -> Option<()> {
        *self.words.get_mut(self.len)? = word;
        self.len += 1;
        Some(())
    }

    pub fn push_all(&mut self, words: &[u32]) -> Option<()> {
        let end = self.len.checked_add(words.len())?;
        self.words.get_mut(self.len..end)?.copy_from_slice(words);
        self.len = end;
        Some(())
    }

    pub fn push_rect(&mut self, rect: &Rect) -> Option<()> {
        self.push_all(&[
            rect.left as u32,
            rect.top as u32,
            rect.right as u32,
            rect.bottom as u32,
        ])
    }

    pub fn push_surface(&mut self, surface: &Surface) -> Option<()> {
        self.push_all(&surface.words())
    }

    /// Drops the words after `len`, to reuse a header for the next batch
    pub fn truncate(&mut self, len: usize) {
        self.len = self.len.min(len);
    }

    /// Overwrites an earlier word, for counts known after their arrays
    pub fn set(&mut self, index: usize, word: u32) {
        if index < self.len {
            self.words[index] = word;
        }
    }
}

/// The header of a [`GPU_OP_BLT`] command, written before its arrays
#[derive(Clone, Copy, Debug, Default)]
pub struct Blt {
    pub flags: u32,
    pub rop: u32,
    pub dst: Surface,
    pub src: Surface,
    pub dst_rect: Rect,
    pub src_rect: Rect,
    pub color: u32,
    pub src_key: u32,
    pub dst_key: u32,
    pub pattern_x: i32,
    pub pattern_y: i32,
    pub xlate_count: u32,
}

impl Blt {
    /// Index of the clip count, written once the clip rects are in
    pub const CLIP_COUNT: usize = 29;

    pub fn encode(&self, command: &mut Command) -> Option<()> {
        command.push_all(&[GPU_OP_BLT, self.flags, self.rop])?;
        command.push_surface(&self.dst)?;
        command.push_surface(&self.src)?;
        command.push_rect(&self.dst_rect)?;
        command.push_rect(&self.src_rect)?;
        command.push_all(&[
            self.color,
            self.src_key,
            self.dst_key,
            self.pattern_x as u32,
            self.pattern_y as u32,
            self.xlate_count,
            0,
        ])
    }
}

/// The header of a [`GPU_OP_TEXT`] command
#[derive(Clone, Copy, Debug, Default)]
pub struct Text {
    pub flags: u32,
    pub dst: Surface,
    pub fore: u32,
    pub back: u32,
    pub opaque: Rect,
}

impl Text {
    pub const GLYPH_COUNT: usize = 14;
    pub const CLIP_COUNT: usize = 15;

    pub fn encode(&self, command: &mut Command) -> Option<()> {
        command.push_all(&[GPU_OP_TEXT, self.flags])?;
        command.push_surface(&self.dst)?;
        command.push_all(&[self.fore, self.back])?;
        command.push_rect(&self.opaque)?;
        command.push_all(&[0, 0])
    }
}

/// The header of a [`GPU_OP_ALPHA`] command
#[derive(Clone, Copy, Debug, Default)]
pub struct Alpha {
    pub flags: u32,
    pub alpha: u32,
    pub dst: Surface,
    pub src: Surface,
    pub dst_rect: Rect,
    pub src_rect: Rect,
}

impl Alpha {
    pub const CLIP_COUNT: usize = 23;

    pub fn encode(&self, command: &mut Command) -> Option<()> {
        command.push_all(&[GPU_OP_ALPHA, self.flags, self.alpha])?;
        command.push_surface(&self.dst)?;
        command.push_surface(&self.src)?;
        command.push_rect(&self.dst_rect)?;
        command.push_rect(&self.src_rect)?;
        command.push(0)
    }
}

/// The 3D borders of the XP theme and window frames are cosmetic lines: the pixels of an axis
/// aligned segment as a rectangle, without its last pixel like GDI draws them. Coordinates are
/// whole pixels, `None` for diagonal segments.
pub fn line_rect(x1: i32, y1: i32, x2: i32, y2: i32) -> Option<Rect> {
    if y1 == y2 {
        let (left, right) = if x1 <= x2 { (x1, x2) } else { (x2 + 1, x1 + 1) };
        Some(Rect::new(left, y1, right, y1 + 1))
    } else if x1 == x2 {
        let (top, bottom) = if y1 <= y2 { (y1, y2) } else { (y2 + 1, y1 + 1) };
        Some(Rect::new(x1, top, x1 + 1, bottom))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rop_operands() {
        assert!(rop_uses_source(ROP_SRCCOPY));
        assert!(!rop_uses_pattern(ROP_SRCCOPY));
        assert!(rop_uses_pattern(ROP_PATCOPY));
        assert!(!rop_uses_source(ROP_PATCOPY));
        // DSTINVERT, BLACKNESS and WHITENESS use neither
        for rop in [0x55, 0x00, 0xFF] {
            assert!(!rop_uses_source(rop) && !rop_uses_pattern(rop));
        }
        // PSDPxax, the transparent text and mask blit, uses both
        assert!(rop_uses_source(0xB8) && rop_uses_pattern(0xB8));
    }

    #[test]
    fn rect_intersection_and_order() {
        let a = Rect::new(0, 0, 10, 10);
        assert_eq!(
            a.intersect(&Rect::new(5, 5, 20, 20)),
            Rect::new(5, 5, 10, 10)
        );
        assert!(a.intersect(&Rect::new(10, 0, 20, 10)).is_empty());
        assert_eq!(Rect::new(10, 8, 2, 4).ordered(), Rect::new(2, 4, 10, 8));
    }

    #[test]
    fn lines_leave_out_their_last_pixel() {
        assert_eq!(line_rect(2, 5, 6, 5), Some(Rect::new(2, 5, 6, 6)));
        assert_eq!(line_rect(6, 5, 2, 5), Some(Rect::new(3, 5, 7, 6)));
        assert_eq!(line_rect(3, 1, 3, 4), Some(Rect::new(3, 1, 4, 4)));
        assert_eq!(line_rect(3, 4, 3, 1), Some(Rect::new(3, 2, 4, 5)));
        assert_eq!(line_rect(4, 4, 4, 4), Some(Rect::new(4, 4, 4, 5)));
        assert_eq!(line_rect(0, 0, 1, 1), None);
    }

    #[test]
    fn headers_have_their_documented_sizes() {
        let mut words = [0; 64];
        let mut command = Command::new(&mut words);
        Blt::default().encode(&mut command).expect("fits");
        assert_eq!(command.len(), BLT_HEADER_WORDS);
        let mut command = Command::new(&mut words);
        Text::default().encode(&mut command).expect("fits");
        assert_eq!(command.len(), TEXT_HEADER_WORDS);
        let mut command = Command::new(&mut words);
        Alpha::default().encode(&mut command).expect("fits");
        assert_eq!(command.len(), ALPHA_HEADER_WORDS);
        assert_eq!(Blt::CLIP_COUNT, BLT_HEADER_WORDS - 1);
        assert_eq!(Text::CLIP_COUNT, TEXT_HEADER_WORDS - 1);
        assert_eq!(Alpha::CLIP_COUNT, ALPHA_HEADER_WORDS - 1);
    }

    #[test]
    fn commands_stop_when_full() {
        let mut words = [0; 4];
        let mut command = Command::new(&mut words);
        assert!(command.push_all(&[1, 2, 3]).is_some());
        assert!(command.push_rect(&Rect::default()).is_none());
        assert_eq!(command.as_words(), &[1, 2, 3]);
        command.set(1, 7);
        command.set(9, 7);
        assert_eq!(command.as_words(), &[1, 7, 3]);
    }
}
