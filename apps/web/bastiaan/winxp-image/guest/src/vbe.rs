/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The Bochs VBE (dispi) registers of the display adapter v86 emulates, for the miniport's mode
//! sets and the display driver's DirectDraw flips.

use crate::port;

const INDEX: u16 = 0x1CE;
const DATA: u16 = 0x1CF;
pub const ID: u16 = 0;
pub const XRES: u16 = 1;
pub const YRES: u16 = 2;
pub const BPP: u16 = 3;
pub const ENABLE: u16 = 4;
/// The first visible row, which flips between surfaces in VRAM
pub const Y_OFFSET: u16 = 9;
pub const VIDEO_MEMORY_64K: u16 = 0xA;
pub const ENABLED: u16 = 0x01;
pub const DAC_8BIT: u16 = 0x20;
pub const LFB: u16 = 0x40;

/// # Safety
///
/// Kernel mode, the adapter is the Bochs VBE.
pub unsafe fn write(index: u16, value: u16) {
    // SAFETY: The ports belong to the adapter
    unsafe {
        port::write_u16(INDEX, index);
        port::write_u16(DATA, value);
    }
}

/// # Safety
///
/// Kernel mode, the adapter is the Bochs VBE.
pub unsafe fn read(index: u16) -> u16 {
    // SAFETY: The ports belong to the adapter
    unsafe {
        port::write_u16(INDEX, index);
        port::read_u16(DATA)
    }
}
