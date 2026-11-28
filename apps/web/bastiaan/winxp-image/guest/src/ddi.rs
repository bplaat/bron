/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The display driver interface of Windows XP for `v86disp.dll`: the GDI DDI (`winddi.h`) and the
//! DirectDraw HAL of NT (`ddrawint.h`), the subset the driver uses. Display drivers may only import
//! from `win32k.sys`. Sizes and offsets are checked against the DDK headers.

#![allow(non_snake_case)]

use core::ffi::c_void;
use core::mem::offset_of;

pub type Bool = i32;
pub type Handle = *mut c_void;
pub type Hsurf = *mut c_void;
pub type Hdev = *mut c_void;
pub type Hpalette = *mut c_void;
pub type Dhpdev = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rectl {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pointl {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sizel {
    pub cx: i32,
    pub cy: i32,
}

// GDI objects

#[repr(C)]
pub struct SurfObj {
    pub dhsurf: *mut c_void,
    pub hsurf: Hsurf,
    pub dhpdev: Dhpdev,
    pub hdev: Hdev,
    pub sizl_bitmap: Sizel,
    pub cj_bits: u32,
    pub pv_bits: *mut c_void,
    pub pv_scan0: *mut c_void,
    pub l_delta: i32,
    pub i_uniq: u32,
    pub i_bitmap_format: u32,
    pub i_type: u16,
    pub fj_bitmap: u16,
}
const _: () = assert!(size_of::<SurfObj>() == 52);

#[repr(C)]
pub struct ClipObj {
    pub i_uniq: u32,
    pub rcl_bounds: Rectl,
    pub i_d_complexity: u8,
    pub i_f_complexity: u8,
    pub i_mode: u8,
    pub fj_options: u8,
}
const _: () = assert!(size_of::<ClipObj>() == 24);

#[repr(C)]
pub struct BrushObj {
    pub i_solid_color: u32,
    pub pv_rbrush: *mut c_void,
    pub fl_color_type: u32,
}
const _: () = assert!(size_of::<BrushObj>() == 12);

#[repr(C)]
pub struct XlateObj {
    pub i_uniq: u32,
    pub fl_xlate: u32,
    pub i_src_type: u16,
    pub i_dst_type: u16,
    pub c_entries: u32,
    pub pul_xlate: *const u32,
}
const _: () = assert!(size_of::<XlateObj>() == 20);

#[repr(C)]
pub struct GlyphBits {
    pub ptl_origin: Pointl,
    pub sizl_bitmap: Sizel,
    /// The rows, one bit per pixel, each padded to a byte
    pub aj: [u8; 0],
}

/// `GLYPHDEF`, a union of the glyph bitmap and its outline
#[repr(C)]
pub struct GlyphDef {
    pub pgb: *const GlyphBits,
}

#[repr(C)]
pub struct GlyphPos {
    pub hg: u32,
    pub pgdf: *const GlyphDef,
    pub ptl: Pointl,
}
const _: () = assert!(size_of::<GlyphPos>() == 16);

#[repr(C)]
pub struct StrObj {
    pub c_glyphs: u32,
    pub fl_accel: u32,
    pub ul_char_inc: u32,
    pub rcl_bk_ground: Rectl,
    pub pgp: *mut GlyphPos,
    pub pwsz_org: *const u16,
}
const _: () = assert!(size_of::<StrObj>() == 36);
const _: () = assert!(offset_of!(StrObj, pgp) == 28);

#[repr(C)]
pub struct FontObj {
    pub i_uniq: u32,
    pub i_face: u32,
    pub cx_max: u32,
    pub fl_font_type: u32,
    pub i_tt_uniq: usize,
    pub i_file: usize,
    pub siz_log_res_ppi: Sizel,
    pub ul_style_size: u32,
    pub pv_consumer: *mut c_void,
    pub pv_producer: *mut c_void,
}
const _: () = assert!(size_of::<FontObj>() == 44);
const _: () = assert!(offset_of!(FontObj, fl_font_type) == 12);

#[repr(C)]
pub struct PathObj {
    pub fl: u32,
    pub c_curves: u32,
}

/// 28.4 fixed point coordinates
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PointFix {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
pub struct PathData {
    pub flags: u32,
    pub count: u32,
    pub pptfx: *const PointFix,
}
const _: () = assert!(size_of::<PathData>() == 12);

#[repr(C)]
pub struct LineAttrs {
    pub fl: u32,
    pub i_join: u32,
    pub i_end_cap: u32,
    pub el_width: u32,
    pub e_miter_limit: u32,
    pub cstyle: u32,
    pub pstyle: *const u32,
    pub el_style_state: u32,
}
const _: () = assert!(size_of::<LineAttrs>() == 32);
const _: () = assert!(offset_of!(LineAttrs, pstyle) == 24);

#[repr(C)]
pub struct BlendObj {
    pub blend_op: u8,
    pub blend_flags: u8,
    pub source_constant_alpha: u8,
    pub alpha_format: u8,
}

/// An opaque engine object
#[repr(C)]
pub struct Opaque {
    _private: [u8; 0],
}
pub type PalObj = Opaque;
pub type XformObj = Opaque;
pub type ColorAdjustment = Opaque;

/// `ENUMRECTS` with room for a batch of rectangles
#[repr(C)]
pub struct EnumRects<const N: usize> {
    pub c: u32,
    pub arcl: [Rectl; N],
}

// Driver setup

#[repr(C)]
pub struct DrvFn {
    pub i_func: u32,
    pub pfn: *const c_void,
}

#[repr(C)]
pub struct DrvEnableData {
    pub i_driver_version: u32,
    pub c: u32,
    pub pdrvfn: *const DrvFn,
}
const _: () = assert!(size_of::<DrvEnableData>() == 12);

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CieChroma {
    pub x: i32,
    pub y: i32,
    pub y_luminance: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ColorInfo {
    pub red: CieChroma,
    pub green: CieChroma,
    pub blue: CieChroma,
    pub cyan: CieChroma,
    pub magenta: CieChroma,
    pub yellow: CieChroma,
    pub alignment_white: CieChroma,
    pub red_gamma: i32,
    pub green_gamma: i32,
    pub blue_gamma: i32,
    pub dyes: [i32; 6],
}
const _: () = assert!(size_of::<ColorInfo>() == 120);

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct GdiInfo {
    pub ul_version: u32,
    pub ul_technology: u32,
    pub ul_horz_size: u32,
    pub ul_vert_size: u32,
    pub ul_horz_res: u32,
    pub ul_vert_res: u32,
    pub c_bits_pixel: u32,
    pub c_planes: u32,
    pub ul_num_colors: u32,
    pub fl_raster: u32,
    pub ul_log_pixels_x: u32,
    pub ul_log_pixels_y: u32,
    pub fl_text_caps: u32,
    pub ul_dac_red: u32,
    pub ul_dac_green: u32,
    pub ul_dac_blue: u32,
    pub ul_aspect_x: u32,
    pub ul_aspect_y: u32,
    pub ul_aspect_xy: u32,
    pub x_style_step: i32,
    pub y_style_step: i32,
    pub den_style_step: i32,
    pub ptl_phys_offset: Pointl,
    pub szl_phys_size: Sizel,
    pub ul_num_pal_reg: u32,
    pub ci_device: ColorInfo,
    pub ul_device_pels_dpi: u32,
    pub ul_primary_order: u32,
    pub ul_ht_pattern_size: u32,
    pub ul_ht_output_format: u32,
    pub fl_ht_flags: u32,
    pub ul_v_refresh: u32,
    pub ul_blt_alignment: u32,
    pub ul_panning_horz_res: u32,
    pub ul_panning_vert_res: u32,
    pub x_panning_alignment: u32,
    pub y_panning_alignment: u32,
    pub cx_ht_pat: u32,
    pub cy_ht_pat: u32,
    pub p_ht_pat_a: usize,
    pub p_ht_pat_b: usize,
    pub p_ht_pat_c: usize,
    pub fl_shade_blend: u32,
    pub ul_physical_pixel_characteristics: u32,
    pub ul_physical_pixel_gamma: u32,
}
const _: () = assert!(size_of::<GdiInfo>() == 304);
const _: () = assert!(offset_of!(GdiInfo, ci_device) == 108);
const _: () = assert!(offset_of!(GdiInfo, ul_device_pels_dpi) == 228);
const _: () = assert!(offset_of!(GdiInfo, ul_v_refresh) == 248);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct LogFontW {
    pub lf_height: i32,
    pub lf_width: i32,
    pub lf_escapement: i32,
    pub lf_orientation: i32,
    pub lf_weight: i32,
    pub lf_italic: u8,
    pub lf_underline: u8,
    pub lf_strike_out: u8,
    pub lf_char_set: u8,
    pub lf_out_precision: u8,
    pub lf_clip_precision: u8,
    pub lf_quality: u8,
    pub lf_pitch_and_family: u8,
    pub lf_face_name: [u16; 32],
}
const _: () = assert!(size_of::<LogFontW>() == 92);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DevInfo {
    pub fl_graphics_caps: u32,
    pub lf_default_font: LogFontW,
    pub lf_ansi_var_font: LogFontW,
    pub lf_ansi_fix_font: LogFontW,
    pub c_fonts: u32,
    pub i_dither_format: u32,
    pub cx_dither: u16,
    pub cy_dither: u16,
    pub hpal_default: Hpalette,
    pub fl_graphics_caps2: u32,
}
const _: () = assert!(size_of::<DevInfo>() == 300);
const _: () = assert!(offset_of!(DevInfo, c_fonts) == 280);
const _: () = assert!(offset_of!(DevInfo, hpal_default) == 292);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DevModeW {
    pub dm_device_name: [u16; 32],
    pub dm_spec_version: u16,
    pub dm_driver_version: u16,
    pub dm_size: u16,
    pub dm_driver_extra: u16,
    pub dm_fields: u32,
    pub dm_position: Pointl,
    pub dm_display_orientation: u32,
    pub dm_display_fixed_output: u32,
    pub dm_color: i16,
    pub dm_duplex: i16,
    pub dm_y_resolution: i16,
    pub dm_tt_option: i16,
    pub dm_collate: i16,
    pub dm_form_name: [u16; 32],
    pub dm_log_pixels: u16,
    pub dm_bits_per_pel: u32,
    pub dm_pels_width: u32,
    pub dm_pels_height: u32,
    pub dm_display_flags: u32,
    pub dm_display_frequency: u32,
    pub dm_icm_method: u32,
    pub dm_icm_intent: u32,
    pub dm_media_type: u32,
    pub dm_dither_type: u32,
    pub dm_reserved1: u32,
    pub dm_reserved2: u32,
    pub dm_panning_width: u32,
    pub dm_panning_height: u32,
}
const _: () = assert!(size_of::<DevModeW>() == 220);
const _: () = assert!(offset_of!(DevModeW, dm_fields) == 72);
const _: () = assert!(offset_of!(DevModeW, dm_bits_per_pel) == 168);
const _: () = assert!(offset_of!(DevModeW, dm_display_frequency) == 184);

pub const DDI_DRIVER_VERSION_NT5_01: u32 = 0x0003_0100;
pub const GDI_DRIVER_VERSION: u32 = 0x4000;

pub const INDEX_DRV_ENABLE_PDEV: u32 = 0;
pub const INDEX_DRV_COMPLETE_PDEV: u32 = 1;
pub const INDEX_DRV_DISABLE_PDEV: u32 = 2;
pub const INDEX_DRV_ENABLE_SURFACE: u32 = 3;
pub const INDEX_DRV_DISABLE_SURFACE: u32 = 4;
pub const INDEX_DRV_ASSERT_MODE: u32 = 5;
pub const INDEX_DRV_DISABLE_DRIVER: u32 = 8;
pub const INDEX_DRV_REALIZE_BRUSH: u32 = 12;
pub const INDEX_DRV_STROKE_PATH: u32 = 14;
pub const INDEX_DRV_BIT_BLT: u32 = 18;
pub const INDEX_DRV_COPY_BITS: u32 = 19;
pub const INDEX_DRV_STRETCH_BLT: u32 = 20;
pub const INDEX_DRV_SET_PALETTE: u32 = 22;
pub const INDEX_DRV_TEXT_OUT: u32 = 23;
pub const INDEX_DRV_SET_POINTER_SHAPE: u32 = 29;
pub const INDEX_DRV_MOVE_POINTER: u32 = 30;
pub const INDEX_DRV_LINE_TO: u32 = 31;
pub const INDEX_DRV_GET_MODES: u32 = 41;
pub const INDEX_DRV_GET_DIRECT_DRAW_INFO: u32 = 59;
pub const INDEX_DRV_ENABLE_DIRECT_DRAW: u32 = 60;
pub const INDEX_DRV_DISABLE_DIRECT_DRAW: u32 = 61;
pub const INDEX_DRV_ALPHA_BLEND: u32 = 71;
pub const INDEX_DRV_TRANSPARENT_BLT: u32 = 74;

pub const GCAPS_ALTERNATEFILL: u32 = 0x4;
pub const GCAPS_WINDINGFILL: u32 = 0x8;
pub const GCAPS_COLOR_DITHER: u32 = 0x20;
pub const GCAPS_OPAQUERECT: u32 = 0x100;
pub const GCAPS_MONO_DITHER: u32 = 0x400;
pub const GCAPS_ASYNCMOVE: u32 = 0x1000;
pub const GCAPS_DIRECTDRAW: u32 = 0x4000;
pub const GCAPS_PALMANAGED: u32 = 0x8_0000;
pub const GCAPS_DITHERONREALIZE: u32 = 0x20_0000;

pub const BMF_1BPP: u32 = 1;
pub const BMF_4BPP: u32 = 2;
pub const BMF_8BPP: u32 = 3;
pub const BMF_16BPP: u32 = 4;
pub const BMF_24BPP: u32 = 5;
pub const BMF_32BPP: u32 = 6;

pub const HOOK_BITBLT: u32 = 0x1;
pub const HOOK_STRETCHBLT: u32 = 0x2;
pub const HOOK_TEXTOUT: u32 = 0x8;
pub const HOOK_STROKEPATH: u32 = 0x20;
pub const HOOK_LINETO: u32 = 0x100;
pub const HOOK_COPYBITS: u32 = 0x400;
pub const HOOK_TRANSPARENTBLT: u32 = 0x8000;
pub const HOOK_ALPHABLEND: u32 = 0x1_0000;

pub const DC_TRIVIAL: u8 = 0;
pub const DC_RECT: u8 = 1;
pub const DC_COMPLEX: u8 = 3;
pub const CT_RECTANGLES: u32 = 0;
pub const CD_RIGHTDOWN: u32 = 0;
pub const CD_LEFTWARDS: u32 = 1;
pub const CD_UPWARDS: u32 = 2;
pub const CD_ANY: u32 = 4;

pub const XO_TRIVIAL: u32 = 0x1;
pub const XO_TABLE: u32 = 0x2;
pub const PAL_INDEXED: u32 = 0x1;
pub const PAL_BITFIELDS: u32 = 0x2;
pub const FL_ZERO_MEMORY: u32 = 0x1;
pub const FL_NONPAGED_MEMORY: u32 = 0x2;
pub const FO_GRAY16: u32 = 0x1_0000;
pub const FO_CLEARTYPE_X: u32 = 0x1000_0000;
pub const LA_GEOMETRIC: u32 = 0x1;
pub const LA_STYLED: u32 = 0x8;
pub const PD_BEGINSUBPATH: u32 = 0x1;
pub const PD_CLOSEFIGURE: u32 = 0x8;
pub const PD_BEZIERS: u32 = 0x10;
pub const AC_SRC_ALPHA: u8 = 0x1;
pub const SPS_ERROR: u32 = 0;
pub const SPS_DECLINE: u32 = 1;
pub const SPS_ACCEPT_NOEXCLUDE: u32 = 2;
pub const DT_RASDISPLAY: u32 = 1;
pub const TC_RA_ABLE: u32 = 0x2000;
pub const PRIMARY_ORDER_CBA: u32 = 4;
pub const HT_PATSIZE_4X4_M: u32 = 3;
pub const HT_FORMAT_8BPP: u32 = 4;
pub const HT_FORMAT_16BPP: u32 = 5;
pub const HT_FORMAT_32BPP: u32 = 7;
pub const HT_FLAG_ADDITIVE_PRIMS: u32 = 0x4;
pub const R2_COPYPEN: u32 = 13;
pub const COLORONCOLOR: u32 = 3;
pub const HALFTONE: u32 = 4;
pub const DM_SPECVERSION: u16 = 0x0401;
pub const DM_BITSPERPEL: u32 = 0x4_0000;
pub const DM_PELSWIDTH: u32 = 0x8_0000;
pub const DM_PELSHEIGHT: u32 = 0x10_0000;
pub const DM_DISPLAYFLAGS: u32 = 0x20_0000;
pub const DM_DISPLAYFREQUENCY: u32 = 0x40_0000;

#[link(
    name = "win32k.sys",
    kind = "raw-dylib",
    modifiers = "+verbatim",
    import_name_type = "undecorated"
)]
unsafe extern "system" {
    pub fn EngCreateDeviceSurface(dhsurf: *mut c_void, sizl: Sizel, i_format: u32) -> Hsurf;
    pub fn EngModifySurface(
        hsurf: Hsurf,
        hdev: Hdev,
        fl_hooks: u32,
        fl_surface: u32,
        dhsurf: *mut c_void,
        pv_scan0: *mut c_void,
        l_delta: i32,
        pv_reserved: *mut c_void,
    ) -> Bool;
    pub fn EngDeleteSurface(hsurf: Hsurf) -> Bool;
    pub fn EngDeviceIoControl(
        h_device: Handle,
        code: u32,
        input: *mut c_void,
        input_size: u32,
        output: *mut c_void,
        output_size: u32,
        returned: *mut u32,
    ) -> u32;
    pub fn EngAllocMem(flags: u32, size: u32, tag: u32) -> *mut c_void;
    pub fn EngFreeMem(pv: *mut c_void);
    pub fn EngCreatePalette(
        i_mode: u32,
        c_colors: u32,
        pul_colors: *const u32,
        fl_red: u32,
        fl_green: u32,
        fl_blue: u32,
    ) -> Hpalette;
    pub fn EngDeletePalette(hpal: Hpalette) -> Bool;
    pub fn PALOBJ_cGetColors(ppalo: *mut PalObj, start: u32, count: u32, colors: *mut u32) -> u32;
    pub fn EngBitBlt(
        trg: *mut SurfObj,
        src: *mut SurfObj,
        mask: *mut SurfObj,
        pco: *mut ClipObj,
        pxlo: *mut XlateObj,
        prcl_trg: *mut Rectl,
        pptl_src: *mut Pointl,
        pptl_mask: *mut Pointl,
        pbo: *mut BrushObj,
        pptl_brush: *mut Pointl,
        rop4: u32,
    ) -> Bool;
    pub fn EngCopyBits(
        dst: *mut SurfObj,
        src: *mut SurfObj,
        pco: *mut ClipObj,
        pxlo: *mut XlateObj,
        prcl_dst: *mut Rectl,
        pptl_src: *mut Pointl,
    ) -> Bool;
    pub fn EngStretchBlt(
        dst: *mut SurfObj,
        src: *mut SurfObj,
        mask: *mut SurfObj,
        pco: *mut ClipObj,
        pxlo: *mut XlateObj,
        pca: *mut ColorAdjustment,
        pptl_ht_org: *mut Pointl,
        prcl_dst: *mut Rectl,
        prcl_src: *mut Rectl,
        pptl_mask: *mut Pointl,
        mode: u32,
    ) -> Bool;
    pub fn EngTextOut(
        pso: *mut SurfObj,
        pstro: *mut StrObj,
        pfo: *mut FontObj,
        pco: *mut ClipObj,
        prcl_extra: *mut Rectl,
        prcl_opaque: *mut Rectl,
        pbo_fore: *mut BrushObj,
        pbo_opaque: *mut BrushObj,
        pptl_org: *mut Pointl,
        mix: u32,
    ) -> Bool;
    pub fn EngStrokePath(
        pso: *mut SurfObj,
        ppo: *mut PathObj,
        pco: *mut ClipObj,
        pxo: *mut XformObj,
        pbo: *mut BrushObj,
        pptl_brush_org: *mut Pointl,
        plineattrs: *mut LineAttrs,
        mix: u32,
    ) -> Bool;
    pub fn EngLineTo(
        pso: *mut SurfObj,
        pco: *mut ClipObj,
        pbo: *mut BrushObj,
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        prcl_bounds: *mut Rectl,
        mix: u32,
    ) -> Bool;
    pub fn EngAlphaBlend(
        dst: *mut SurfObj,
        src: *mut SurfObj,
        pco: *mut ClipObj,
        pxlo: *mut XlateObj,
        prcl_dst: *mut Rectl,
        prcl_src: *mut Rectl,
        blend: *mut BlendObj,
    ) -> Bool;
    pub fn EngTransparentBlt(
        dst: *mut SurfObj,
        src: *mut SurfObj,
        pco: *mut ClipObj,
        pxlo: *mut XlateObj,
        prcl_dst: *mut Rectl,
        prcl_src: *mut Rectl,
        trans_color: u32,
        reserved: u32,
    ) -> Bool;
    pub fn CLIPOBJ_cEnumStart(
        pco: *mut ClipObj,
        all: Bool,
        kind: u32,
        direction: u32,
        limit: u32,
    ) -> u32;
    pub fn CLIPOBJ_bEnum(pco: *mut ClipObj, cj: u32, pul: *mut u32) -> Bool;
    pub fn BRUSHOBJ_pvAllocRbrush(pbo: *mut BrushObj, cj: u32) -> *mut c_void;
    pub fn BRUSHOBJ_pvGetRbrush(pbo: *mut BrushObj) -> *mut c_void;
    pub fn STROBJ_vEnumStart(pstro: *mut StrObj);
    pub fn STROBJ_bEnum(pstro: *mut StrObj, count: *mut u32, glyphs: *mut *mut GlyphPos) -> Bool;
    pub fn PATHOBJ_vEnumStart(ppo: *mut PathObj);
    pub fn PATHOBJ_bEnum(ppo: *mut PathObj, data: *mut PathData) -> Bool;
    pub fn XLATEOBJ_iXlate(pxlo: *mut XlateObj, color: u32) -> u32;
    pub fn EngBugCheckEx(code: u32, p1: usize, p2: usize, p3: usize, p4: usize) -> !;
}

// DirectDraw

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct DdPixelFormat {
    pub dw_size: u32,
    pub dw_flags: u32,
    pub dw_four_cc: u32,
    pub dw_rgb_bit_count: u32,
    pub dw_r_bit_mask: u32,
    pub dw_g_bit_mask: u32,
    pub dw_b_bit_mask: u32,
    pub dw_rgb_alpha_bit_mask: u32,
}
const _: () = assert!(size_of::<DdPixelFormat>() == 32);

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct DdColorKey {
    pub low: u32,
    pub high: u32,
}

#[repr(C)]
pub struct VideoMemoryInfo {
    pub fp_primary: usize,
    pub dw_flags: u32,
    pub dw_display_width: u32,
    pub dw_display_height: u32,
    pub l_display_pitch: i32,
    pub ddpf_display: DdPixelFormat,
    pub dw_offscreen_align: u32,
    pub dw_overlay_align: u32,
    pub dw_texture_align: u32,
    pub dw_z_buffer_align: u32,
    pub dw_alpha_align: u32,
    pub pv_primary: *mut c_void,
}
const _: () = assert!(size_of::<VideoMemoryInfo>() == 76);
const _: () = assert!(offset_of!(VideoMemoryInfo, ddpf_display) == 20);
const _: () = assert!(offset_of!(VideoMemoryInfo, pv_primary) == 72);

pub const DD_ROP_SPACE: usize = 8;

#[repr(C)]
pub struct DdNtCoreCaps {
    pub dw_size: u32,
    pub dw_caps: u32,
    pub dw_caps2: u32,
    pub dw_ckey_caps: u32,
    pub dw_fx_caps: u32,
    pub dw_fx_alpha_caps: u32,
    pub dw_pal_caps: u32,
    pub dw_sv_caps: u32,
    pub alpha_bit_depths: [u32; 6],
    pub dw_z_buffer_bit_depths: u32,
    pub dw_vid_mem_total: u32,
    pub dw_vid_mem_free: u32,
    pub dw_max_visible_overlays: u32,
    pub dw_curr_visible_overlays: u32,
    pub dw_num_four_cc_codes: u32,
    pub alignment: [u32; 5],
    pub dw_rops: [u32; DD_ROP_SPACE],
    pub dds_caps: u32,
    pub stretch_limits: [u32; 6],
    pub reserved: [u32; 3],
    pub dw_svb_caps: u32,
    pub dw_svb_ckey_caps: u32,
    pub dw_svb_fx_caps: u32,
    pub dw_svb_rops: [u32; DD_ROP_SPACE],
    pub dw_vsb_caps: u32,
    pub dw_vsb_ckey_caps: u32,
    pub dw_vsb_fx_caps: u32,
    pub dw_vsb_rops: [u32; DD_ROP_SPACE],
    pub dw_ssb_caps: u32,
    pub dw_ssb_ckey_caps: u32,
    pub dw_ssb_fx_caps: u32,
    pub dw_ssb_rops: [u32; DD_ROP_SPACE],
    pub dw_max_video_ports: u32,
    pub dw_curr_video_ports: u32,
    pub dw_svb_caps2: u32,
}
const _: () = assert!(size_of::<DdNtCoreCaps>() == 316);
const _: () = assert!(offset_of!(DdNtCoreCaps, dw_vid_mem_total) == 60);
const _: () = assert!(offset_of!(DdNtCoreCaps, dw_rops) == 100);
const _: () = assert!(offset_of!(DdNtCoreCaps, dds_caps) == 132);

pub type DdGetDriverInfo = unsafe extern "system" fn(*mut DdGetDriverInfoData) -> u32;

#[repr(C)]
pub struct DdHalInfo {
    pub dw_size: u32,
    pub vmi_data: VideoMemoryInfo,
    pub dd_caps: DdNtCoreCaps,
    pub get_driver_info: Option<DdGetDriverInfo>,
    pub dw_flags: u32,
    pub lp_d3d_global_driver_data: *mut c_void,
    pub lp_d3d_hal_callbacks: *mut c_void,
    pub lp_d3d_buf_callbacks: *mut c_void,
}
const _: () = assert!(size_of::<DdHalInfo>() == 416);
const _: () = assert!(offset_of!(DdHalInfo, dd_caps) == 80);
const _: () = assert!(offset_of!(DdHalInfo, get_driver_info) == 396);

#[repr(C)]
pub struct VideoMemory {
    pub dw_flags: u32,
    pub fp_start: usize,
    /// `dwWidth` in bytes for rectangular heaps
    pub fp_end_or_width: usize,
    pub dds_caps: u32,
    pub dds_caps_alt: u32,
    /// `dwHeight` for rectangular heaps
    pub height: usize,
}
const _: () = assert!(size_of::<VideoMemory>() == 24);

#[repr(C)]
pub struct DdDirectDrawGlobal {
    pub dhpdev: Dhpdev,
    pub dw_reserved1: usize,
    pub dw_reserved2: usize,
    pub lp_dd_video_port_caps: *mut c_void,
}

#[repr(C)]
pub struct DdSurfaceGlobal {
    pub dw_block_size_y: u32,
    pub lp_vid_mem_heap: *mut c_void,
    /// An offset from the start of the frame buffer for video memory surfaces
    pub fp_vid_mem: usize,
    pub l_pitch: i32,
    pub y_hint: i32,
    pub x_hint: i32,
    pub w_height: u32,
    pub w_width: u32,
    pub dw_reserved1: usize,
    pub ddpf_surface: DdPixelFormat,
    pub fp_heap_offset: usize,
    pub h_creator_process: Handle,
}
const _: () = assert!(size_of::<DdSurfaceGlobal>() == 76);
const _: () = assert!(offset_of!(DdSurfaceGlobal, fp_vid_mem) == 8);
const _: () = assert!(offset_of!(DdSurfaceGlobal, w_height) == 24);
const _: () = assert!(offset_of!(DdSurfaceGlobal, ddpf_surface) == 36);

#[repr(C)]
pub struct DdSurfaceLocal {
    pub lp_gbl: *mut DdSurfaceGlobal,
    pub dw_flags: u32,
    pub dds_caps: u32,
    pub dw_reserved1: usize,
    pub ddck_ck_src_blt: DdColorKey,
    pub ddck_ck_dest_blt: DdColorKey,
    pub lp_surf_more: *mut c_void,
    pub lp_attach_list: *mut c_void,
    pub lp_attach_list_from: *mut c_void,
    pub rc_overlay_src: Rectl,
}
const _: () = assert!(size_of::<DdSurfaceLocal>() == 60);
const _: () = assert!(offset_of!(DdSurfaceLocal, ddck_ck_src_blt) == 16);
const _: () = assert!(offset_of!(DdSurfaceLocal, ddck_ck_dest_blt) == 24);

#[repr(C)]
pub struct DdSurfaceDesc {
    pub dw_size: u32,
    pub dw_flags: u32,
    pub dw_height: u32,
    pub dw_width: u32,
    pub l_pitch: i32,
    pub dw_back_buffer_count: u32,
    pub dw_mip_map_count: u32,
    pub dw_alpha_bit_depth: u32,
    pub dw_reserved: u32,
    pub lp_surface: *mut c_void,
    pub color_keys: [DdColorKey; 4],
    pub ddpf_pixel_format: DdPixelFormat,
    pub dds_caps: u32,
}
const _: () = assert!(size_of::<DdSurfaceDesc>() == 108);
const _: () = assert!(offset_of!(DdSurfaceDesc, ddpf_pixel_format) == 72);

#[repr(C)]
pub struct DdBltFx {
    pub dw_size: u32,
    pub dw_ddfx: u32,
    pub dw_rop: u32,
    pub reserved: [u32; 17],
    pub dw_fill_color: u32,
    pub ddck_dest_colorkey: DdColorKey,
    pub ddck_src_colorkey: DdColorKey,
}
const _: () = assert!(size_of::<DdBltFx>() == 100);
const _: () = assert!(offset_of!(DdBltFx, dw_fill_color) == 80);
const _: () = assert!(offset_of!(DdBltFx, ddck_src_colorkey) == 92);

#[repr(C)]
pub struct DdBltData {
    pub lp_dd: *mut DdDirectDrawGlobal,
    pub lp_dd_dest_surface: *mut DdSurfaceLocal,
    pub r_dest: Rectl,
    pub lp_dd_src_surface: *mut DdSurfaceLocal,
    pub r_src: Rectl,
    pub dw_flags: u32,
    pub dw_rop_flags: u32,
    pub blt_fx: DdBltFx,
    pub dd_r_val: i32,
    pub blt: *mut c_void,
    pub is_clipped: Bool,
    pub r_orig_dest: Rectl,
    pub r_orig_src: Rectl,
    pub dw_rect_cnt: u32,
    pub pr_dest_rects: *const Rectl,
    pub dw_a_flags: u32,
    pub ddargb_scale_factors: u32,
}
const _: () = assert!(size_of::<DdBltData>() == 212);
const _: () = assert!(offset_of!(DdBltData, dw_flags) == 44);
const _: () = assert!(offset_of!(DdBltData, blt_fx) == 52);
const _: () = assert!(offset_of!(DdBltData, dd_r_val) == 152);
const _: () = assert!(offset_of!(DdBltData, is_clipped) == 160);
const _: () = assert!(offset_of!(DdBltData, dw_rect_cnt) == 196);

#[repr(C)]
pub struct DdFlipData {
    pub lp_dd: *mut DdDirectDrawGlobal,
    pub lp_surf_curr: *mut DdSurfaceLocal,
    pub lp_surf_targ: *mut DdSurfaceLocal,
    pub dw_flags: u32,
    pub dd_r_val: i32,
    pub flip: *mut c_void,
    pub lp_surf_curr_left: *mut DdSurfaceLocal,
    pub lp_surf_targ_left: *mut DdSurfaceLocal,
}
const _: () = assert!(size_of::<DdFlipData>() == 32);

#[repr(C)]
pub struct DdWaitForVerticalBlankData {
    pub lp_dd: *mut DdDirectDrawGlobal,
    pub dw_flags: u32,
    pub b_is_in_vb: u32,
    pub h_event: u32,
    pub dd_r_val: i32,
    pub wait_for_vertical_blank: *mut c_void,
}
const _: () = assert!(size_of::<DdWaitForVerticalBlankData>() == 24);

#[repr(C)]
pub struct DdGetScanLineData {
    pub lp_dd: *mut DdDirectDrawGlobal,
    pub dw_scan_line: u32,
    pub dd_r_val: i32,
    pub get_scan_line: *mut c_void,
}
const _: () = assert!(size_of::<DdGetScanLineData>() == 16);

#[repr(C)]
pub struct DdCanCreateSurfaceData {
    pub lp_dd: *mut DdDirectDrawGlobal,
    pub lp_dd_surface_desc: *mut DdSurfaceDesc,
    pub b_is_different_pixel_format: u32,
    pub dd_r_val: i32,
    pub can_create_surface: *mut c_void,
}
const _: () = assert!(size_of::<DdCanCreateSurfaceData>() == 20);

#[repr(C)]
pub struct DdMapMemoryData {
    pub lp_dd: *mut DdDirectDrawGlobal,
    pub b_map: Bool,
    pub h_process: Handle,
    pub fp_process: usize,
    pub dd_r_val: i32,
}
const _: () = assert!(size_of::<DdMapMemoryData>() == 20);

/// `DD_GETBLTSTATUSDATA` and `DD_GETFLIPSTATUSDATA`, which have the same layout
#[repr(C)]
pub struct DdGetStatusData {
    pub lp_dd: *mut DdDirectDrawGlobal,
    pub lp_dd_surface: *mut DdSurfaceLocal,
    pub dw_flags: u32,
    pub dd_r_val: i32,
    pub callback: *mut c_void,
}
const _: () = assert!(size_of::<DdGetStatusData>() == 20);

/// `PALETTEENTRY`
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PaletteEntry {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub flags: u8,
}

#[repr(C)]
pub struct DdSetEntriesData {
    pub lp_dd: *mut DdDirectDrawGlobal,
    pub lp_dd_palette: *mut c_void,
    pub dw_base: u32,
    pub dw_num_entries: u32,
    pub lp_entries: *const PaletteEntry,
    pub dd_r_val: i32,
    pub set_entries: *mut c_void,
}
const _: () = assert!(size_of::<DdSetEntriesData>() == 28);

#[repr(C)]
pub struct DdSetPaletteData {
    pub lp_dd: *mut DdDirectDrawGlobal,
    pub lp_dd_surface: *mut DdSurfaceLocal,
    pub lp_dd_palette: *mut c_void,
    pub dd_r_val: i32,
    pub set_palette: *mut c_void,
    pub attach: Bool,
}
const _: () = assert!(size_of::<DdSetPaletteData>() == 24);

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Guid {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

#[repr(C)]
pub struct DdGetDriverInfoData {
    pub dhpdev: Dhpdev,
    pub dw_size: u32,
    pub dw_flags: u32,
    pub guid_info: Guid,
    pub dw_expected_size: u32,
    pub lpv_data: *mut c_void,
    pub dw_actual_size: u32,
    pub dd_r_val: i32,
}
const _: () = assert!(size_of::<DdGetDriverInfoData>() == 44);

#[repr(C)]
pub struct DdFlipToGdiSurfaceData {
    pub lp_dd: *mut DdDirectDrawGlobal,
    pub dw_to_gdi: u32,
    pub dw_reserved: u32,
    pub dd_r_val: i32,
    pub flip_to_gdi_surface: *mut c_void,
}
const _: () = assert!(size_of::<DdFlipToGdiSurfaceData>() == 20);

pub type DdCallback<T> = unsafe extern "system" fn(*mut T) -> u32;

#[repr(C)]
pub struct DdNtCallbacks {
    pub dw_size: u32,
    pub dw_flags: u32,
    pub free_driver_memory: *const c_void,
    pub set_exclusive_mode: *const c_void,
    pub flip_to_gdi_surface: Option<DdCallback<DdFlipToGdiSurfaceData>>,
}
const _: () = assert!(size_of::<DdNtCallbacks>() == 20);

#[repr(C)]
pub struct DdCallbacks {
    pub dw_size: u32,
    pub dw_flags: u32,
    pub destroy_driver: *const c_void,
    pub create_surface: *const c_void,
    pub set_color_key: *const c_void,
    pub set_mode: *const c_void,
    pub wait_for_vertical_blank: Option<DdCallback<DdWaitForVerticalBlankData>>,
    pub can_create_surface: Option<DdCallback<DdCanCreateSurfaceData>>,
    pub create_palette: *const c_void,
    pub get_scan_line: Option<DdCallback<DdGetScanLineData>>,
    pub map_memory: Option<DdCallback<DdMapMemoryData>>,
}
const _: () = assert!(size_of::<DdCallbacks>() == 44);

#[repr(C)]
pub struct DdSurfaceCallbacks {
    pub dw_size: u32,
    pub dw_flags: u32,
    pub destroy_surface: *const c_void,
    pub flip: Option<DdCallback<DdFlipData>>,
    pub set_clip_list: *const c_void,
    pub lock: *const c_void,
    pub unlock: *const c_void,
    pub blt: Option<DdCallback<DdBltData>>,
    pub set_color_key: *const c_void,
    pub add_attached_surface: *const c_void,
    pub get_blt_status: Option<DdCallback<DdGetStatusData>>,
    pub get_flip_status: Option<DdCallback<DdGetStatusData>>,
    pub update_overlay: *const c_void,
    pub set_overlay_position: *const c_void,
    pub reserved4: *const c_void,
    pub set_palette: Option<DdCallback<DdSetPaletteData>>,
}
const _: () = assert!(size_of::<DdSurfaceCallbacks>() == 64);

#[repr(C)]
pub struct DdPaletteCallbacks {
    pub dw_size: u32,
    pub dw_flags: u32,
    pub destroy_palette: *const c_void,
    pub set_entries: Option<DdCallback<DdSetEntriesData>>,
}
const _: () = assert!(size_of::<DdPaletteCallbacks>() == 16);

pub const GUID_NT_CALLBACKS: Guid = Guid {
    data1: 0x6fe9_ecde,
    data2: 0xdf89,
    data3: 0x11d1,
    data4: [0x9d, 0xb0, 0x00, 0x60, 0x08, 0x27, 0x71, 0xba],
};

pub const DDHAL_DRIVER_NOTHANDLED: u32 = 0;
pub const DDHAL_DRIVER_HANDLED: u32 = 1;
pub const DDHAL_CB32_WAITFORVERTICALBLANK: u32 = 0x10;
pub const DDHAL_CB32_CANCREATESURFACE: u32 = 0x20;
pub const DDHAL_CB32_GETSCANLINE: u32 = 0x80;
pub const DDHAL_CB32_MAPMEMORY: u32 = 0x8000_0000;
pub const DDHAL_SURFCB32_FLIP: u32 = 0x2;
pub const DDHAL_SURFCB32_BLT: u32 = 0x20;
pub const DDHAL_SURFCB32_GETBLTSTATUS: u32 = 0x100;
pub const DDHAL_SURFCB32_GETFLIPSTATUS: u32 = 0x200;
pub const DDHAL_NTCB32_FLIPTOGDISURFACE: u32 = 0x4;
pub const DDHALINFO_GETDRIVERINFOSET: u32 = 0x4;
pub const VIDMEM_ISRECTANGULAR: u32 = 0x2;

pub const DD_OK: i32 = 0;
const fn dd_error(code: u32) -> i32 {
    (0x8876_0000 | code) as i32
}
pub const DDERR_GENERIC: i32 = 0x8000_4005_u32 as i32;
pub const DDERR_UNSUPPORTED: i32 = 0x8000_4001_u32 as i32;
pub const DDERR_CURRENTLYNOTAVAIL: i32 = dd_error(40);
pub const DDERR_INVALIDPIXELFORMAT: i32 = dd_error(145);
pub const DDERR_VERTICALBLANKINPROGRESS: i32 = dd_error(537);

pub const DDCAPS_BLT: u32 = 0x40;
pub const DDCAPS_BLTSTRETCH: u32 = 0x200;
pub const DDCAPS_COLORKEY: u32 = 0x40_0000;
pub const DDCAPS_BLTCOLORFILL: u32 = 0x400_0000;
pub const DDCAPS2_NOPAGELOCKREQUIRED: u32 = 0x800;
pub const DDCAPS2_WIDESURFACES: u32 = 0x1000;
pub const DDCAPS2_FLIPNOVSYNC: u32 = 0x40_0000;
pub const DDCKEYCAPS_DESTBLT: u32 = 0x1;
pub const DDCKEYCAPS_SRCBLT: u32 = 0x200;
pub const DDFXCAPS_BLTMIRRORLEFTRIGHT: u32 = 0x40;
pub const DDFXCAPS_BLTMIRRORUPDOWN: u32 = 0x80;
pub const DDFXCAPS_BLTSHRINKX: u32 = 0x400;
pub const DDFXCAPS_BLTSHRINKXN: u32 = 0x800;
pub const DDFXCAPS_BLTSHRINKY: u32 = 0x1000;
pub const DDFXCAPS_BLTSHRINKYN: u32 = 0x2000;
pub const DDFXCAPS_BLTSTRETCHX: u32 = 0x4000;
pub const DDFXCAPS_BLTSTRETCHXN: u32 = 0x8000;
pub const DDFXCAPS_BLTSTRETCHY: u32 = 0x1_0000;
pub const DDFXCAPS_BLTSTRETCHYN: u32 = 0x2_0000;
pub const DDSCAPS_BACKBUFFER: u32 = 0x4;
pub const DDSCAPS_COMPLEX: u32 = 0x8;
pub const DDSCAPS_FLIP: u32 = 0x10;
pub const DDSCAPS_FRONTBUFFER: u32 = 0x20;
pub const DDSCAPS_OFFSCREENPLAIN: u32 = 0x40;
pub const DDSCAPS_OVERLAY: u32 = 0x80;
pub const DDSCAPS_PRIMARYSURFACE: u32 = 0x200;
pub const DDSCAPS_SYSTEMMEMORY: u32 = 0x800;
pub const DDSCAPS_TEXTURE: u32 = 0x1000;
pub const DDSCAPS_3DDEVICE: u32 = 0x2000;
pub const DDSCAPS_VIDEOMEMORY: u32 = 0x4000;
pub const DDSCAPS_ZBUFFER: u32 = 0x2_0000;
pub const DDSCAPS_EXECUTEBUFFER: u32 = 0x80_0000;
pub const DDSCAPS_LOCALVIDMEM: u32 = 0x1000_0000;
pub const DDPF_PALETTEINDEXED8: u32 = 0x20;
pub const DDPF_RGB: u32 = 0x40;
pub const DDBLT_COLORFILL: u32 = 0x400;
pub const DDBLT_DDFX: u32 = 0x800;
pub const DDBLT_KEYDEST: u32 = 0x2000;
pub const DDBLT_KEYDESTOVERRIDE: u32 = 0x4000;
pub const DDBLT_KEYSRC: u32 = 0x8000;
pub const DDBLT_KEYSRCOVERRIDE: u32 = 0x1_0000;
pub const DDBLT_ROP: u32 = 0x2_0000;
pub const DDBLTFX_MIRRORLEFTRIGHT: u32 = 0x2;
pub const DDBLTFX_MIRRORUPDOWN: u32 = 0x4;
pub const DDWAITVB_BLOCKBEGIN: u32 = 0x1;
pub const DDWAITVB_BLOCKEND: u32 = 0x4;
pub const DDWAITVB_I_TESTVB: u32 = 0x8000_0006;
/// The Win32 ROP that DirectDraw passes in `dwROP`, SRCCOPY
pub const DD_SRCCOPY: u32 = 0x00CC_0020;
