/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The surface layout and tiling of 2D textures on the Wii U's Latte GPU, the parts of AMD's R600
//! addrlib that `GX2CalcSurfaceSizeAndAlignment` uses for them. The GPU has 2 pipes, 4 banks and
//! 256 byte pipe interleave. Big levels are macro tiled (`2D_TILED_THIN1`), levels smaller than a
//! macro tile are micro tiled (`1D_TILED_THIN1`).

const PIPES: u32 = 2;
const BANKS: u32 = 4;
const GROUP_BYTES: u32 = 256;
const MICRO_TILE_SIZE: u32 = 8;
const MACRO_TILE_WIDTH: u32 = MICRO_TILE_SIZE * BANKS;
const MACRO_TILE_HEIGHT: u32 = MICRO_TILE_SIZE * PIPES;
/// The level where the tiling changes to micro tiling, in bits 16 to 23 of the swizzle field.
const SWIZZLE_LEVEL_SHIFT: u32 = 16;

/// A GX2 tile mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TileMode {
    /// 8x8 micro tiles one after another.
    Tiled1DThin1 = 2,
    /// Macro tiles of 4x2 micro tiles spread over the banks and pipes.
    Tiled2DThin1 = 4,
}

/// The layout of one level, in elements: pixels or 4x4 blocks of compressed formats.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LevelLayout {
    pub(crate) tile_mode: TileMode,
    pub(crate) pitch: u32,
    pub(crate) height: u32,
    pub(crate) size: u32,
    pub(crate) alignment: u32,
    pitch_align: u32,
    height_align: u32,
}

/// The layout of a texture with all its levels, like `GX2CalcSurfaceSizeAndAlignment` fills in.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SurfaceLayout {
    pub(crate) tile_mode: TileMode,
    pub(crate) swizzle: u32,
    pub(crate) alignment: u32,
    pub(crate) pitch: u32,
    pub(crate) image_size: u32,
    pub(crate) mipmap_size: u32,
    /// The offset of level 1 from the image, then the offsets of the levels from 2 in the
    /// mipmaps.
    pub(crate) mip_level_offset: [u32; 13],
    pub(crate) levels: Vec<LevelLayout>,
}

/// The size of a level in elements, like addrlib pads the levels of compressed formats.
fn level_size(width: u32, height: u32, level: u32, compressed: bool) -> (u32, u32) {
    let (width, height) = ((width >> level).max(1), (height >> level).max(1));
    if !compressed {
        return (width, height);
    }
    let (width, height) = if level == 0 {
        (width.next_multiple_of(4), height.next_multiple_of(4))
    } else {
        (width.next_power_of_two(), height.next_power_of_two())
    };
    ((width / 4).max(1), (height / 4).max(1))
}

/// Computes the layout of one level of a surface with `base` as its tile mode.
fn level_layout(
    base: TileMode,
    bpp: u32,
    width: u32,
    height: u32,
    level: u32,
    compressed: bool,
) -> LevelLayout {
    let (mut pitch, mut height) = level_size(width, height, level, compressed);
    if level > 0 {
        pitch = pitch.next_power_of_two();
        height = height.next_power_of_two();
    }

    // Levels smaller than a macro tile, as wide as a pipe interleave group, are micro tiled
    let micro_tile_bytes = MICRO_TILE_SIZE * MICRO_TILE_SIZE * bpp / 8;
    let width_align_factor = (GROUP_BYTES / micro_tile_bytes).max(1);
    let tile_mode = if base == TileMode::Tiled2DThin1
        && level > 0
        && (pitch < width_align_factor * MACRO_TILE_WIDTH || height < MACRO_TILE_HEIGHT)
    {
        TileMode::Tiled1DThin1
    } else {
        base
    };

    let (pitch_align, height_align, alignment) = match tile_mode {
        TileMode::Tiled1DThin1 => (
            (GROUP_BYTES / bpp).max(MICRO_TILE_SIZE),
            MICRO_TILE_SIZE,
            GROUP_BYTES,
        ),
        TileMode::Tiled2DThin1 => {
            let pitch_align =
                MACRO_TILE_WIDTH.max(MACRO_TILE_WIDTH * (GROUP_BYTES / bpp / MICRO_TILE_SIZE));
            let macro_tile_bytes = MACRO_TILE_WIDTH * MACRO_TILE_HEIGHT * bpp / 8;
            let alignment = macro_tile_bytes.max(MACRO_TILE_HEIGHT * pitch_align * bpp / 8);
            (pitch_align, MACRO_TILE_HEIGHT, alignment)
        }
    };
    let pitch = pitch.next_multiple_of(pitch_align);
    let height = height.next_multiple_of(height_align);
    LevelLayout {
        tile_mode,
        pitch,
        height,
        size: pitch * height * bpp / 8,
        alignment,
        pitch_align,
        height_align,
    }
}

/// Computes the layout of a 2D texture with the default tile mode, like
/// `GX2CalcSurfaceSizeAndAlignment`.
pub(crate) fn surface_layout(
    bpp: u32,
    width: u32,
    height: u32,
    mip_levels: u32,
    compressed: bool,
) -> SurfaceLayout {
    let mut tile_mode = TileMode::Tiled2DThin1;
    let mut swizzle = 13 << SWIZZLE_LEVEL_SHIFT;
    let mut base = level_layout(tile_mode, bpp, width, height, 0, compressed);
    // Small textures are micro tiled, GX2 compares their pixel size with the element alignment
    if width < base.pitch_align && height < base.height_align {
        tile_mode = TileMode::Tiled1DThin1;
        swizzle = 0;
        base = level_layout(tile_mode, bpp, width, height, 0, compressed);
    }

    let mut levels = vec![base];
    let mut mip_level_offset = [0; 13];
    let mut last_tile_mode = tile_mode;
    let mut previous_size = base.size;
    let mut offset0 = 0;
    for level in 1..mip_levels {
        let layout = level_layout(tile_mode, bpp, width, height, level, compressed);
        if last_tile_mode == TileMode::Tiled2DThin1 && layout.tile_mode == TileMode::Tiled1DThin1 {
            swizzle = level << SWIZZLE_LEVEL_SHIFT;
            last_tile_mode = layout.tile_mode;
        }
        // Each level starts at the size of the previous one aligned up, GX2 aligns that size
        // instead of the offset
        let offset = previous_size.next_multiple_of(layout.alignment);
        if level == 1 {
            offset0 = offset;
        } else {
            mip_level_offset[level as usize - 1] = offset + mip_level_offset[level as usize - 2];
        }
        previous_size = layout.size;
        levels.push(layout);
    }
    let mipmap_size = if mip_levels > 1 {
        previous_size + mip_level_offset[mip_levels as usize - 2]
    } else {
        0
    };
    mip_level_offset[0] = offset0;
    SurfaceLayout {
        tile_mode,
        swizzle,
        alignment: base.alignment,
        pitch: base.pitch,
        image_size: base.size,
        mipmap_size,
        mip_level_offset,
        levels,
    }
}

/// The index of an element in its 8x8 micro tile, in the order of displayable textures.
fn pixel_index(x: u32, y: u32, bpp: u32) -> u32 {
    let bit = |value: u32, index: u32| (value >> index) & 1;
    let bits = match bpp {
        8 => [
            bit(x, 0),
            bit(x, 1),
            bit(x, 2),
            bit(y, 1),
            bit(y, 0),
            bit(y, 2),
        ],
        16 => [
            bit(x, 0),
            bit(x, 1),
            bit(x, 2),
            bit(y, 0),
            bit(y, 1),
            bit(y, 2),
        ],
        64 => [
            bit(x, 0),
            bit(y, 0),
            bit(x, 1),
            bit(x, 2),
            bit(y, 1),
            bit(y, 2),
        ],
        128 => [
            bit(y, 0),
            bit(x, 0),
            bit(x, 1),
            bit(x, 2),
            bit(y, 1),
            bit(y, 2),
        ],
        _ => [
            bit(x, 0),
            bit(x, 1),
            bit(y, 0),
            bit(x, 2),
            bit(y, 1),
            bit(y, 2),
        ],
    };
    bits.iter()
        .enumerate()
        .fold(0, |index, (position, bit)| index | bit << position)
}

/// Returns the byte offset of an element in a level, without pipe and bank swizzle.
pub(crate) fn element_offset(layout: &LevelLayout, bpp: u32, x: u32, y: u32) -> usize {
    let element_bytes = pixel_index(x, y, bpp) * bpp / 8;
    let micro_tile_bytes = MICRO_TILE_SIZE * MICRO_TILE_SIZE * bpp / 8;
    let offset = match layout.tile_mode {
        TileMode::Tiled1DThin1 => {
            let micro_tile = x / MICRO_TILE_SIZE + y / MICRO_TILE_SIZE * layout.pitch / 8;
            micro_tile * micro_tile_bytes + element_bytes
        }
        TileMode::Tiled2DThin1 => {
            let bit = |value: u32, index: u32| (value >> index) & 1;
            let pipe = bit(y, 3) ^ bit(x, 3);
            let bank_y = y / PIPES;
            let bank = (bit(bank_y, 4) ^ bit(x, 3)) | (bit(bank_y, 3) ^ bit(x, 4)) << 1;
            let macro_tile_bytes = MACRO_TILE_WIDTH * MACRO_TILE_HEIGHT * bpp / 8;
            let macro_tile =
                x / MACRO_TILE_WIDTH + y / MACRO_TILE_HEIGHT * (layout.pitch / MACRO_TILE_WIDTH);
            // The pipe and bank select 256 byte groups within each 2 KiB of the macro tiles
            let total = element_bytes + macro_tile * macro_tile_bytes / (PIPES * BANKS);
            let group_mask = GROUP_BYTES - 1;
            ((total & !group_mask) * PIPES * BANKS)
                | (bank * PIPES * GROUP_BYTES)
                | (pipe * GROUP_BYTES)
                | (total & group_mask)
        }
    };
    offset as usize
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macro_tiles_big_levels_and_micro_tiles_small_ones() {
        // A 256x256 RGBA8 texture with mipmaps, like GX2CalcSurfaceSizeAndAlignment computes
        let layout = surface_layout(32, 256, 256, 9, false);
        assert_eq!(layout.tile_mode, TileMode::Tiled2DThin1);
        assert_eq!((layout.alignment, layout.pitch), (2048, 256));
        assert_eq!(layout.image_size, 256 * 256 * 4);
        // Levels from 16x16 are smaller than a macro tile of 2 pipe interleave groups wide
        let modes: Vec<_> = layout.levels.iter().map(|level| level.tile_mode).collect();
        assert_eq!(modes[3], TileMode::Tiled2DThin1);
        assert_eq!(modes[4], TileMode::Tiled1DThin1);
        assert_eq!(layout.swizzle, 4 << SWIZZLE_LEVEL_SHIFT);
        assert_eq!(layout.mip_level_offset[0], layout.image_size);
        assert_eq!(layout.mip_level_offset[1], 128 * 128 * 4);
    }

    #[test]
    fn small_textures_are_micro_tiled() {
        // GX2 compares the pixel size with the macro tile size in elements
        let layout = surface_layout(64, 16, 8, 1, true);
        assert_eq!(layout.tile_mode, TileMode::Tiled1DThin1);
        assert_eq!(
            (layout.swizzle, layout.alignment, layout.pitch),
            (0, 256, 8)
        );
    }

    #[test]
    fn tiling_is_a_permutation() {
        for (bpp, width, height) in [(32, 64, 32), (64, 32, 16), (32, 8, 8)] {
            for tile_mode in [TileMode::Tiled1DThin1, TileMode::Tiled2DThin1] {
                let layout = level_layout(tile_mode, bpp, width, height, 0, false);
                let mut seen = vec![false; layout.size as usize / (bpp as usize / 8)];
                for y in 0..layout.height {
                    for x in 0..layout.pitch {
                        let index = element_offset(&layout, bpp, x, y) / (bpp as usize / 8);
                        assert!(!seen[index], "{bpp} {tile_mode:?} {x} {y}");
                        seen[index] = true;
                    }
                }
            }
        }
    }
}
