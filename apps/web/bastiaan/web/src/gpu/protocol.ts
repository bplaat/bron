/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// The 2D command protocol of the display driver, see winxp-image/guest/src/gpu.rs for the layouts. A test keeps
// these constants equal to the Rust ones.

export const V86_CMD_GPU = 0x8610;
export const V86_CMD_GPU_SCANLINE = 0x8611;

export const V86_GPU_DONE = 0;
export const V86_GPU_PUNT = 1;
export const V86_GPU_INVALID = 2;
export const V86_GPU_VBLANK = 0x80000000;

export const GPU_SURFACE_VRAM = 0;
export const GPU_SURFACE_GUEST = 1;
export const SURFACE_WORDS = 6;
export const RECT_WORDS = 4;

export const GPU_OP_BLT = 1;
export const GPU_OP_TEXT = 2;
export const GPU_OP_ALPHA = 3;

export const GPU_BLT_SOURCE = 0x1;
export const GPU_BLT_PATTERN = 0x2;
export const GPU_BLT_SOURCE_KEY = 0x4;
export const GPU_BLT_DEST_KEY = 0x8;
export const GPU_BLT_MIRROR_X = 0x10;
export const GPU_BLT_MIRROR_Y = 0x20;
export const GPU_TEXT_OPAQUE = 0x1;
export const GPU_ALPHA_PER_PIXEL = 0x1;

export const BLT_HEADER_WORDS = 30;
export const TEXT_HEADER_WORDS = 16;
export const ALPHA_HEADER_WORDS = 24;
export const GLYPH_WORDS = 5;
export const PATTERN_PIXELS = 64;
export const XLATE_MAX = 256;

export const ROP_SRCCOPY = 0xcc;
export const ROP_PATCOPY = 0xf0;
