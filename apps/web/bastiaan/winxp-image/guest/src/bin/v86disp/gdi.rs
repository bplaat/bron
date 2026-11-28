/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The accelerated GDI calls: blits with every ROP3 and brush, glyphs, axis aligned cosmetic
//! lines, stretching, alpha and color keys onto VRAM, executed by the page. A call the page or the
//! driver can't do goes to the engine as a whole, before anything is drawn.

use core::ptr::{self, addr_of};
use core::slice;

use v86::ddi::*;
use v86::gpu::*;
use v86::port::{self, Regs};

use crate::{MAX_RECTS, Pdev, pdev};

const TRUE: Bool = 1;
const SOLID_NONE: u32 = u32::MAX;
const AC_SRC_OVER: u8 = 0;
const SO_VERTICAL: u32 = 0x4;
const SO_REVERSED: u32 = 0x8;

/// The realized 8x8 pattern of a brush, in the target's pixel format
#[repr(C)]
struct Rbrush {
    pixels: [u32; PATTERN_PIXELS],
}

/// Executes a command, returns whether the page drew it
///
/// # Safety
///
/// Kernel mode, the command is in the nonpaged command buffer of a PDEV.
unsafe fn submit(command: &Command) -> bool {
    let words = command.as_words();
    let regs = Regs {
        ebx: words.as_ptr() as u32,
        ecx: V86_CMD_GPU,
        esi: size_of_val(words) as u32,
        ..Regs::default()
    };
    // SAFETY: The command buffer is nonpaged and the page reads it during the call
    unsafe { port::backdoor(regs) }.eax == V86_GPU_DONE
}

const fn bpp(format: u32) -> Option<u32> {
    match format {
        BMF_1BPP => Some(1),
        BMF_4BPP => Some(4),
        BMF_8BPP => Some(8),
        BMF_16BPP => Some(16),
        BMF_24BPP => Some(24),
        BMF_32BPP => Some(32),
        _ => None,
    }
}

const fn rect(rect: &Rectl) -> Rect {
    Rect::new(rect.left, rect.top, rect.right, rect.bottom)
}

/// The surface as the page sees it: an offset into VRAM when its bits are there, else the linear
/// address of its first row
pub(crate) fn surface(pdev: &Pdev, so: &SurfObj) -> Option<Surface> {
    let bpp = bpp(so.i_bitmap_format)?;
    if so.pv_scan0.is_null() {
        return None;
    }
    let (kind, address) = match pdev.vram_offset(so.pv_scan0) {
        Some(offset) if so.l_delta > 0 => (GPU_SURFACE_VRAM, offset),
        Some(_) => return None,
        None => (GPU_SURFACE_GUEST, so.pv_scan0 as u32),
    };
    Some(Surface {
        kind,
        address,
        pitch: so.l_delta,
        bpp,
        width: so.sizl_bitmap.cx as u32,
        height: so.sizl_bitmap.cy as u32,
    })
}

/// The PDEV of the first surface that belongs to the driver
///
/// # Safety
///
/// The surfaces are null or valid.
unsafe fn device<'a>(surfaces: &[*mut SurfObj]) -> Option<&'a mut Pdev> {
    surfaces.iter().find_map(|&so| {
        // SAFETY: GDI passes valid surfaces, ours carry our PDEV
        let dhpdev = unsafe { so.as_ref()?.dhpdev };
        // SAFETY: A non-null dhpdev on a surface GDI hands this driver is its PDEV
        (!dhpdev.is_null())
            .then(|| unsafe { pdev(dhpdev) })
            .filter(|pdev| pdev.host)
    })
}

/// The destination when it is in VRAM
///
/// # Safety
///
/// The surface is valid.
unsafe fn target(pdev: &Pdev, so: *mut SurfObj) -> Option<Surface> {
    // SAFETY: GDI passes a valid target surface
    surface(pdev, unsafe { so.as_ref()? }).filter(|surface| surface.kind == GPU_SURFACE_VRAM)
}

enum Xlate<'a> {
    Identity,
    Table(&'a [u32]),
}

/// How source pixels become destination pixels, when the page can do it: unchanged, or through a
/// table for 1, 4 and 8 bpp sources. RGB translations that keep every channel count as unchanged.
///
/// # Safety
///
/// The translation is null or valid.
unsafe fn xlate<'a>(pxlo: *mut XlateObj, src_bpp: u32, dst_bpp: u32) -> Option<Xlate<'a>> {
    let same = src_bpp == dst_bpp || (src_bpp == 24 && dst_bpp == 32);
    // SAFETY: GDI passes a valid translation or none
    let Some(xlo) = (unsafe { pxlo.as_ref() }) else {
        return same.then_some(Xlate::Identity);
    };
    if xlo.fl_xlate & XO_TRIVIAL != 0 {
        return same.then_some(Xlate::Identity);
    }
    if xlo.fl_xlate & XO_TABLE != 0 {
        let entries = xlo.c_entries as usize;
        return (src_bpp <= 8 && entries <= XLATE_MAX && !xlo.pul_xlate.is_null())
            // SAFETY: The table holds cEntries entries
            .then(|| Xlate::Table(unsafe { slice::from_raw_parts(xlo.pul_xlate, entries) }));
    }
    let channels: &[u32] = match dst_bpp {
        16 => &[0xF800, 0x07E0, 0x001F, 0x8410],
        32 => &[0xFF_0000, 0xFF00, 0xFF, 0x80_8080],
        _ => return None,
    };
    // SAFETY: The translation is valid
    let keeps = channels
        .iter()
        .all(|&color| unsafe { XLATEOBJ_iXlate(pxlo, color) } == color);
    (same && keeps).then_some(Xlate::Identity)
}

/// The clip rectangles inside `bounds`, in the given enumeration direction, in the PDEV's clip
/// buffer. `None` when there are more than it holds.
///
/// # Safety
///
/// The clip object is null or valid.
unsafe fn clips<'a>(
    pdev: &Pdev,
    pco: *mut ClipObj,
    bounds: Rect,
    direction: u32,
) -> Option<&'a [Rect]> {
    // SAFETY: The PDEV's clip buffer holds MAX_RECTS rectangles
    let out = unsafe { slice::from_raw_parts_mut(pdev.clips, MAX_RECTS) };
    let mut count = 0;
    let mut add = |clip: Rect| {
        let clip = clip.intersect(&bounds);
        if clip.is_empty() {
            return true;
        }
        let Some(slot) = out.get_mut(count) else {
            return false;
        };
        *slot = clip;
        count += 1;
        true
    };
    // SAFETY: GDI passes a valid clip object or none, enumeration fills the batch it is given
    unsafe {
        match pco.as_ref().map(|pco| pco.i_d_complexity) {
            None | Some(DC_TRIVIAL) => {
                add(bounds);
            }
            Some(DC_RECT) => {
                add(rect(&(*pco).rcl_bounds));
            }
            _ => {
                CLIPOBJ_cEnumStart(pco, 0, CT_RECTANGLES, direction, 0);
                let mut batch = EnumRects::<32> {
                    c: 0,
                    arcl: [Rectl::default(); 32],
                };
                loop {
                    let more = CLIPOBJ_bEnum(
                        pco,
                        size_of::<EnumRects<32>>() as u32,
                        (&raw mut batch).cast(),
                    ) != 0;
                    let batch_count = (batch.c as usize).min(batch.arcl.len());
                    if !batch.arcl[..batch_count].iter().all(|clip| add(rect(clip))) {
                        return None;
                    }
                    if !more {
                        break;
                    }
                }
            }
        }
        Some(slice::from_raw_parts(pdev.clips, count))
    }
}

/// Writes `header`, then the clip rectangles, in as many commands as they need. `count` is the
/// header word with the clip count. Returns whether the page drew everything.
///
/// # Safety
///
/// Kernel mode, the PDEV's buffers are allocated.
unsafe fn run(
    pdev: &mut Pdev,
    header: impl Fn(&mut Command) -> Option<()>,
    count: usize,
    rects: &[Rect],
) -> bool {
    if rects.is_empty() {
        return true;
    }
    // SAFETY: The PDEV's command buffer is allocated
    let mut command = Command::new(unsafe { pdev.commands() });
    if header(&mut command).is_none() || command.remaining() < RECT_WORDS {
        return false;
    }
    let prefix = command.len();
    let mut batch = 0;
    for (i, rect) in rects.iter().enumerate() {
        if command.push_rect(rect).is_none() {
            return false;
        }
        batch += 1;
        if i + 1 == rects.len() || command.remaining() < RECT_WORDS {
            command.set(count, batch);
            // SAFETY: The command is in the PDEV's command buffer
            if !unsafe { submit(&command) } {
                return false;
            }
            command.truncate(prefix);
            batch = 0;
        }
    }
    true
}

/// The ROP3 of a ROP2 mix with the brush as pattern
const fn rop2_to_rop3(mix: u32) -> u32 {
    // The ROP2 minus one is its truth table over pattern (bit 1) and destination (bit 0)
    let table = (mix.wrapping_sub(1)) & 0xF;
    let mut rop = 0;
    let mut index = 0;
    while index < 8 {
        let (pattern, dest) = ((index >> 2) & 1, index & 1);
        rop |= ((table >> ((pattern << 1) | dest)) & 1) << index;
        index += 1;
    }
    rop
}

/// A blit of a ROP3 with a solid or realized brush and an optional source, onto VRAM
///
/// # Safety
///
/// GDI's arguments of DrvBitBlt.
#[allow(clippy::too_many_arguments)]
unsafe fn bit_blt(
    trg: *mut SurfObj,
    src: *mut SurfObj,
    pco: *mut ClipObj,
    pxlo: *mut XlateObj,
    prcl_trg: *mut Rectl,
    pptl_src: *mut Pointl,
    pbo: *mut BrushObj,
    pptl_brush: *mut Pointl,
    rop4: u32,
) -> bool {
    // SAFETY: GDI passes valid arguments for the ROP
    unsafe {
        let Some(pdev) = device(&[trg, src]) else {
            return false;
        };
        let rop = rop4 & 0xFF;
        let Some(dst) = target(pdev, trg) else {
            return false;
        };
        if (rop4 >> 8) & 0xFF != rop {
            return false;
        }
        let dst_rect = rect(&*prcl_trg);
        let mut blt = Blt {
            rop,
            dst,
            dst_rect,
            ..Blt::default()
        };
        let mut pattern: Option<&Rbrush> = None;
        if rop_uses_pattern(rop) {
            let Some(brush) = pbo.as_mut() else {
                return false;
            };
            if brush.i_solid_color != SOLID_NONE {
                blt.color = brush.i_solid_color;
            } else {
                let Some(realized) = BRUSHOBJ_pvGetRbrush(brush).cast::<Rbrush>().as_ref() else {
                    return false;
                };
                pattern = Some(realized);
                blt.flags |= GPU_BLT_PATTERN;
                let origin = pptl_brush.as_ref().copied().unwrap_or_default();
                (blt.pattern_x, blt.pattern_y) = (origin.x, origin.y);
            }
        }
        let mut table: &[u32] = &[];
        let mut direction = CD_ANY;
        if rop_uses_source(rop) {
            let (Some(source), Some(origin)) = (src.as_ref(), pptl_src.as_ref()) else {
                return false;
            };
            let Some(src_surface) = surface(pdev, source) else {
                return false;
            };
            match xlate(pxlo, src_surface.bpp, dst.bpp) {
                Some(Xlate::Identity) => {}
                Some(Xlate::Table(entries)) => table = entries,
                None => return false,
            }
            blt.flags |= GPU_BLT_SOURCE;
            blt.src = src_surface;
            blt.src_rect = Rect::new(
                origin.x,
                origin.y,
                origin.x + dst_rect.width(),
                origin.y + dst_rect.height(),
            );
            blt.xlate_count = table.len() as u32;
            // Copies within a surface take the clips in the order that reads before writing
            if src_surface == dst {
                direction = CD_RIGHTDOWN;
                if dst_rect.top > origin.y {
                    direction |= CD_UPWARDS;
                }
                if dst_rect.left > origin.x {
                    direction |= CD_LEFTWARDS;
                }
            }
        }
        let Some(rects) = clips(pdev, pco, dst_rect, direction) else {
            return false;
        };
        run(
            pdev,
            |command| {
                blt.encode(command)?;
                if let Some(pattern) = pattern {
                    command.push_all(&pattern.pixels)?;
                }
                command.push_all(table)
            },
            Blt::CLIP_COUNT,
            rects,
        )
    }
}

pub(crate) unsafe extern "system" fn drv_bit_blt(
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
) -> Bool {
    // SAFETY: GDI's arguments, which go to the engine unchanged when the page can't draw them
    unsafe {
        if mask.is_null()
            && bit_blt(
                trg, src, pco, pxlo, prcl_trg, pptl_src, pbo, pptl_brush, rop4,
            )
        {
            return TRUE;
        }
        EngBitBlt(
            trg, src, mask, pco, pxlo, prcl_trg, pptl_src, pptl_mask, pbo, pptl_brush, rop4,
        )
    }
}

pub(crate) unsafe extern "system" fn drv_copy_bits(
    dst: *mut SurfObj,
    src: *mut SurfObj,
    pco: *mut ClipObj,
    pxlo: *mut XlateObj,
    prcl_dst: *mut Rectl,
    pptl_src: *mut Pointl,
) -> Bool {
    const SRCCOPY4: u32 = 0xCCCC;
    // SAFETY: GDI's arguments, a blit without brush
    unsafe {
        if bit_blt(
            dst,
            src,
            pco,
            pxlo,
            prcl_dst,
            pptl_src,
            ptr::null_mut(),
            ptr::null_mut(),
            SRCCOPY4,
        ) {
            return TRUE;
        }
        EngCopyBits(dst, src, pco, pxlo, prcl_dst, pptl_src)
    }
}

/// Reads pixel `x` of a row of a 1 to 32 bpp bitmap
///
/// # Safety
///
/// The row holds pixel `x`.
unsafe fn pixel(row: *const u8, x: usize, bpp: u32) -> u32 {
    // SAFETY: The caller keeps x inside the row
    unsafe {
        match bpp {
            1 => u32::from(row.add(x / 8).read() >> (7 - x % 8)) & 1,
            4 => u32::from(row.add(x / 2).read() >> if x % 2 == 0 { 4 } else { 0 }) & 0xF,
            8 => u32::from(row.add(x).read()),
            16 => u32::from(row.add(x * 2).cast::<u16>().read_unaligned()),
            24 => {
                let at = row.add(x * 3);
                u32::from(at.read())
                    | u32::from(at.add(1).read()) << 8
                    | u32::from(at.add(2).read()) << 16
            }
            _ => row.add(x * 4).cast::<u32>().read_unaligned(),
        }
    }
}

/// Realizes 8x8 brushes in the target's format: the page draws patterns of 64 pixels
pub(crate) unsafe extern "system" fn drv_realize_brush(
    pbo: *mut BrushObj,
    target: *mut SurfObj,
    pattern: *mut SurfObj,
    mask: *mut SurfObj,
    pxlo: *mut XlateObj,
    _hatch: u32,
) -> Bool {
    // SAFETY: GDI passes the brush, the target, the pattern surface and its translation
    unsafe {
        let (Some(target), Some(pattern)) = (target.as_ref(), pattern.as_ref()) else {
            return 0;
        };
        let (Some(dst_bpp), Some(src_bpp)) =
            (bpp(target.i_bitmap_format), bpp(pattern.i_bitmap_format))
        else {
            return 0;
        };
        if !mask.is_null()
            || pattern.sizl_bitmap.cx != 8
            || pattern.sizl_bitmap.cy != 8
            || pattern.pv_scan0.is_null()
        {
            return 0;
        }
        let Some(xlate) = xlate(pxlo, src_bpp, dst_bpp) else {
            return 0;
        };
        let realized = BRUSHOBJ_pvAllocRbrush(pbo, size_of::<Rbrush>() as u32).cast::<Rbrush>();
        let Some(realized) = realized.as_mut() else {
            return 0;
        };
        for y in 0..8 {
            let row = pattern
                .pv_scan0
                .cast::<u8>()
                .offset(y as isize * pattern.l_delta as isize);
            for x in 0..8 {
                let value = pixel(row, x, src_bpp);
                realized.pixels[y * 8 + x] = match xlate {
                    Xlate::Identity => value,
                    Xlate::Table(table) => table.get(value as usize).copied().unwrap_or(0),
                };
            }
        }
        TRUE
    }
}

/// Visits the glyph batches of a string, stops when `visit` returns false
///
/// # Safety
///
/// The string object is valid.
unsafe fn glyph_batches(pstro: *mut StrObj, mut visit: impl FnMut(&[GlyphPos]) -> bool) -> bool {
    // SAFETY: GDI passes a valid string, its batches hold their counts
    unsafe {
        let string = &mut *pstro;
        if !string.pgp.is_null() {
            return visit(slice::from_raw_parts(string.pgp, string.c_glyphs as usize));
        }
        STROBJ_vEnumStart(pstro);
        loop {
            let (mut count, mut glyphs) = (0, ptr::null_mut());
            let more = STROBJ_bEnum(pstro, &mut count, &mut glyphs) != 0;
            if !glyphs.is_null() && !visit(slice::from_raw_parts(glyphs, count as usize)) {
                return false;
            }
            if !more {
                return true;
            }
        }
    }
}

/// The bitmap of a glyph, when the font has one
///
/// # Safety
///
/// The glyph position is valid.
unsafe fn glyph_bits<'a>(glyph: &GlyphPos) -> Option<&'a GlyphBits> {
    // SAFETY: GDI's glyph definitions point at their bitmaps
    unsafe { glyph.pgdf.as_ref()?.pgb.as_ref() }
}

/// # Safety
///
/// GDI's arguments of DrvTextOut.
#[allow(clippy::too_many_arguments)]
unsafe fn text_out(
    pso: *mut SurfObj,
    pstro: *mut StrObj,
    pfo: *mut FontObj,
    pco: *mut ClipObj,
    prcl_extra: *mut Rectl,
    prcl_opaque: *mut Rectl,
    pbo_fore: *mut BrushObj,
    pbo_opaque: *mut BrushObj,
    mix: u32,
) -> bool {
    // SAFETY: GDI passes valid arguments, the glyph bitmaps stay while the call lasts
    unsafe {
        let Some(pdev) = device(&[pso]) else {
            return false;
        };
        let Some(dst) = target(pdev, pso) else {
            return false;
        };
        let (Some(font), Some(fore), Some(string)) =
            (pfo.as_ref(), pbo_fore.as_ref(), pstro.as_ref())
        else {
            return false;
        };
        if font.fl_font_type & (FO_GRAY16 | FO_CLEARTYPE_X) != 0
            || !prcl_extra.is_null()
            || mix & 0xFF != R2_COPYPEN
            || fore.i_solid_color == SOLID_NONE
            || (string.ul_char_inc != 0 && string.fl_accel & (SO_VERTICAL | SO_REVERSED) != 0)
        {
            return false;
        }
        let mut text = Text {
            dst,
            fore: fore.i_solid_color,
            ..Text::default()
        };
        if let Some(opaque) = prcl_opaque.as_ref() {
            let Some(back) = pbo_opaque
                .as_ref()
                .filter(|brush| brush.i_solid_color != SOLID_NONE)
            else {
                return false;
            };
            text.flags = GPU_TEXT_OPAQUE;
            text.back = back.i_solid_color;
            text.opaque = rect(opaque);
        }
        // Every glyph needs its bitmap, checked before drawing any
        if !glyph_batches(pstro, |glyphs| {
            glyphs.iter().all(|glyph| glyph_bits(glyph).is_some())
        }) {
            return false;
        }
        let bounds = Rect::new(0, 0, dst.width as i32, dst.height as i32);
        let Some(clips) = clips(pdev, pco, bounds, CD_ANY) else {
            return false;
        };
        if clips.len() * RECT_WORDS + TEXT_HEADER_WORDS + GLYPH_WORDS > crate::COMMAND_WORDS {
            return false;
        }

        // Monospaced strings only position their first glyph
        let (step, mut origin) = (string.ul_char_inc as i32, None::<Pointl>);
        let mut index = 0;
        glyph_batches(pstro, |glyphs| {
            let mut command = Command::new(pdev.commands());
            if text.encode(&mut command).is_none() {
                return false;
            }
            let mut count = 0_u32;
            let room = ((command.remaining() - clips.len() * RECT_WORDS) / GLYPH_WORDS) as u32;
            for glyph in glyphs {
                let Some(bits) = glyph_bits(glyph) else {
                    return false;
                };
                let first = *origin.get_or_insert(glyph.ptl);
                let at = if step != 0 {
                    Pointl {
                        x: first.x + index * step,
                        y: first.y,
                    }
                } else {
                    glyph.ptl
                };
                index += 1;
                command.push_all(&[
                    (at.x + bits.ptl_origin.x) as u32,
                    (at.y + bits.ptl_origin.y) as u32,
                    bits.sizl_bitmap.cx as u32,
                    bits.sizl_bitmap.cy as u32,
                    addr_of!(bits.aj) as u32,
                ]);
                count += 1;
                if count == room {
                    // A long batch goes in several commands
                    let prefix = TEXT_HEADER_WORDS;
                    if !finish_text(&mut command, count, clips) {
                        return false;
                    }
                    command.truncate(prefix);
                    command.set(1, 0);
                    count = 0;
                }
            }
            if count > 0 && !finish_text(&mut command, count, clips) {
                return false;
            }
            // The opaque rectangle only goes with the first command
            text.flags = 0;
            true
        })
    }
}

/// Completes a text command with its glyph count and clips and executes it
///
/// # Safety
///
/// Kernel mode, the command is in the PDEV's buffer.
unsafe fn finish_text(command: &mut Command, count: u32, clips: &[Rect]) -> bool {
    command.set(Text::GLYPH_COUNT, count);
    command.set(Text::CLIP_COUNT, clips.len() as u32);
    if clips
        .iter()
        .try_for_each(|clip| command.push_rect(clip))
        .is_none()
    {
        return false;
    }
    // SAFETY: The command is in the PDEV's command buffer
    unsafe { submit(command) }
}

pub(crate) unsafe extern "system" fn drv_text_out(
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
) -> Bool {
    // SAFETY: GDI's arguments, which go to the engine unchanged when the page can't draw them
    unsafe {
        if text_out(
            pso,
            pstro,
            pfo,
            pco,
            prcl_extra,
            prcl_opaque,
            pbo_fore,
            pbo_opaque,
            mix,
        ) {
            return TRUE;
        }
        EngTextOut(
            pso,
            pstro,
            pfo,
            pco,
            prcl_extra,
            prcl_opaque,
            pbo_fore,
            pbo_opaque,
            pptl_org,
            mix,
        )
    }
}

/// Fills the segments, clipped, with a solid brush and a ROP2
///
/// # Safety
///
/// Kernel mode, the clip object is null or valid.
unsafe fn fill_segments(
    pdev: &mut Pdev,
    dst: Surface,
    pco: *mut ClipObj,
    color: u32,
    mix: u32,
    segments: &[Rect],
) -> bool {
    let Some(bounds) = segments.iter().copied().reduce(|a, b| {
        Rect::new(
            a.left.min(b.left),
            a.top.min(b.top),
            a.right.max(b.right),
            a.bottom.max(b.bottom),
        )
    }) else {
        return true;
    };
    // SAFETY: The clip object is valid
    let Some(clips) = (unsafe { clips(pdev, pco, bounds, CD_ANY) }) else {
        return false;
    };
    // SAFETY: The PDEV's rect buffer holds MAX_RECTS rectangles
    let out = unsafe { slice::from_raw_parts_mut(pdev.rects, MAX_RECTS) };
    let mut count = 0;
    for segment in segments {
        for clip in clips {
            let part = segment.intersect(clip);
            if part.is_empty() {
                continue;
            }
            let Some(slot) = out.get_mut(count) else {
                return false;
            };
            *slot = part;
            count += 1;
        }
    }
    let blt = Blt {
        rop: rop2_to_rop3(mix & 0xFF),
        dst,
        dst_rect: bounds,
        color,
        ..Blt::default()
    };
    // SAFETY: The rect buffer is separate from the command buffer
    let rects = unsafe { slice::from_raw_parts(pdev.rects, count) };
    // SAFETY: Kernel mode, the PDEV's buffers are allocated
    unsafe { run(pdev, |command| blt.encode(command), Blt::CLIP_COUNT, rects) }
}

/// The surface and color of a solid cosmetic line, when the page can draw it
///
/// # Safety
///
/// GDI's surface and brush.
unsafe fn line_target<'a>(
    pso: *mut SurfObj,
    pbo: *mut BrushObj,
    mix: u32,
) -> Option<(&'a mut Pdev, Surface, u32)> {
    // SAFETY: GDI passes valid arguments
    unsafe {
        let pdev = device(&[pso])?;
        let dst = target(pdev, pso)?;
        let color = pbo.as_ref()?.i_solid_color;
        // ROP2s are 1 to 16
        (color != SOLID_NONE && (1..=16).contains(&(mix & 0xFF))).then_some((pdev, dst, color))
    }
}

/// The segments of a path of axis aligned lines on whole pixels, in the PDEV's rect buffer
///
/// # Safety
///
/// The path object is valid.
unsafe fn path_segments<'a>(pdev: &Pdev, ppo: *mut PathObj) -> Option<&'a [Rect]> {
    // SAFETY: The PDEV's rect buffer holds MAX_RECTS rectangles, the path's records hold their
    // points
    unsafe {
        let out = slice::from_raw_parts_mut(pdev.rects, MAX_RECTS);
        let mut count = 0;
        let mut push = |from: Pointl, to: Pointl| -> Option<()> {
            let segment = line_rect(from.x, from.y, to.x, to.y)?;
            *out.get_mut(count)? = segment;
            count += 1;
            Some(())
        };
        let pixel = |point: &PointFix| {
            (point.x & 15 == 0 && point.y & 15 == 0).then_some(Pointl {
                x: point.x >> 4,
                y: point.y >> 4,
            })
        };
        PATHOBJ_vEnumStart(ppo);
        let (mut start, mut last) = (Pointl::default(), None::<Pointl>);
        loop {
            let mut data = PathData {
                flags: 0,
                count: 0,
                pptfx: ptr::null(),
            };
            let more = PATHOBJ_bEnum(ppo, &mut data) != 0;
            if data.flags & PD_BEZIERS != 0 {
                return None;
            }
            let points = if data.pptfx.is_null() {
                &[][..]
            } else {
                slice::from_raw_parts(data.pptfx, data.count as usize)
            };
            for (i, point) in points.iter().enumerate() {
                let point = pixel(point)?;
                if i == 0 && data.flags & PD_BEGINSUBPATH != 0 {
                    start = point;
                } else if let Some(from) = last {
                    push(from, point)?;
                }
                last = Some(point);
            }
            if data.flags & PD_CLOSEFIGURE != 0
                && let Some(from) = last
            {
                push(from, start)?;
            }
            if !more {
                break;
            }
        }
        Some(slice::from_raw_parts(pdev.rects, count))
    }
}

pub(crate) unsafe extern "system" fn drv_stroke_path(
    pso: *mut SurfObj,
    ppo: *mut PathObj,
    pco: *mut ClipObj,
    pxo: *mut XformObj,
    pbo: *mut BrushObj,
    pptl_brush_org: *mut Pointl,
    plineattrs: *mut LineAttrs,
    mix: u32,
) -> Bool {
    // SAFETY: GDI's arguments, which go to the engine unchanged when the page can't draw them
    unsafe {
        let cosmetic = plineattrs.as_ref().is_some_and(|attrs| {
            attrs.fl & (LA_GEOMETRIC | LA_STYLED) == 0 && attrs.pstyle.is_null()
        });
        if cosmetic
            && let Some((pdev, dst, color)) = line_target(pso, pbo, mix)
            && let Some(segments) = path_segments(pdev, ppo)
        {
            // The segments live in the rect buffer that filling reuses, so they are copied first
            let mut copy = [Rect::default(); 64];
            if segments.len() <= copy.len() {
                copy[..segments.len()].copy_from_slice(segments);
                if fill_segments(pdev, dst, pco, color, mix, &copy[..segments.len()]) {
                    return TRUE;
                }
            }
        }
        EngStrokePath(pso, ppo, pco, pxo, pbo, pptl_brush_org, plineattrs, mix)
    }
}

pub(crate) unsafe extern "system" fn drv_line_to(
    pso: *mut SurfObj,
    pco: *mut ClipObj,
    pbo: *mut BrushObj,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    prcl_bounds: *mut Rectl,
    mix: u32,
) -> Bool {
    // SAFETY: GDI's arguments, which go to the engine unchanged when the page can't draw them
    unsafe {
        if let Some((pdev, dst, color)) = line_target(pso, pbo, mix)
            && let Some(segment) = line_rect(x1, y1, x2, y2)
            && fill_segments(pdev, dst, pco, color, mix, &[segment])
        {
            return TRUE;
        }
        EngLineTo(pso, pco, pbo, x1, y1, x2, y2, prcl_bounds, mix)
    }
}

/// A copy from `src_rect` onto `dst_rect`, stretched, mirrored and color keyed as asked
///
/// # Safety
///
/// GDI's surfaces, clip, translation and rectangles.
#[allow(clippy::too_many_arguments)]
unsafe fn stretch(
    dst: *mut SurfObj,
    src: *mut SurfObj,
    pco: *mut ClipObj,
    pxlo: *mut XlateObj,
    prcl_dst: *mut Rectl,
    prcl_src: *mut Rectl,
    key: Option<u32>,
    shrink: bool,
) -> bool {
    // SAFETY: GDI passes valid arguments
    unsafe {
        let Some(pdev) = device(&[dst, src]) else {
            return false;
        };
        let (Some(dst_surface), Some(source), Some(dst_rect), Some(src_rect)) = (
            target(pdev, dst),
            src.as_ref(),
            prcl_dst.as_ref(),
            prcl_src.as_ref(),
        ) else {
            return false;
        };
        let Some(src_surface) = surface(pdev, source) else {
            return false;
        };
        let table = match xlate(pxlo, src_surface.bpp, dst_surface.bpp) {
            Some(Xlate::Identity) => &[][..],
            Some(Xlate::Table(table)) => table,
            None => return false,
        };
        let (dst_rect, src_rect) = (rect(dst_rect), rect(src_rect));
        let mut flags = GPU_BLT_SOURCE;
        if (dst_rect.left > dst_rect.right) != (src_rect.left > src_rect.right) {
            flags |= GPU_BLT_MIRROR_X;
        }
        if (dst_rect.top > dst_rect.bottom) != (src_rect.top > src_rect.bottom) {
            flags |= GPU_BLT_MIRROR_Y;
        }
        let (dst_rect, src_rect) = (dst_rect.ordered(), src_rect.ordered());
        if !shrink && (src_rect.width() > dst_rect.width() || src_rect.height() > dst_rect.height())
        {
            return false;
        }
        if key.is_some() {
            flags |= GPU_BLT_SOURCE_KEY;
        }
        let blt = Blt {
            flags,
            rop: ROP_SRCCOPY,
            dst: dst_surface,
            src: src_surface,
            dst_rect,
            src_rect,
            src_key: key.unwrap_or(0),
            xlate_count: table.len() as u32,
            ..Blt::default()
        };
        let Some(rects) = clips(pdev, pco, dst_rect, CD_ANY) else {
            return false;
        };
        // A stretch within one surface could read what it wrote
        if src_surface == dst_surface
            && !src_rect.intersect(&dst_rect).is_empty()
            && src_rect != dst_rect
        {
            return false;
        }
        run(
            pdev,
            |command| {
                blt.encode(command)?;
                command.push_all(table)
            },
            Blt::CLIP_COUNT,
            rects,
        )
    }
}

pub(crate) unsafe extern "system" fn drv_stretch_blt(
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
) -> Bool {
    // SAFETY: GDI's arguments, which go to the engine unchanged when the page can't draw them.
    // Every mode picks the nearest pixel when enlarging, only COLORONCOLOR does when shrinking.
    unsafe {
        if mask.is_null()
            && mode != HALFTONE
            && stretch(
                dst,
                src,
                pco,
                pxlo,
                prcl_dst,
                prcl_src,
                None,
                mode == COLORONCOLOR,
            )
        {
            return TRUE;
        }
        EngStretchBlt(
            dst,
            src,
            mask,
            pco,
            pxlo,
            pca,
            pptl_ht_org,
            prcl_dst,
            prcl_src,
            pptl_mask,
            mode,
        )
    }
}

pub(crate) unsafe extern "system" fn drv_transparent_blt(
    dst: *mut SurfObj,
    src: *mut SurfObj,
    pco: *mut ClipObj,
    pxlo: *mut XlateObj,
    prcl_dst: *mut Rectl,
    prcl_src: *mut Rectl,
    trans_color: u32,
    reserved: u32,
) -> Bool {
    // SAFETY: GDI's arguments, which go to the engine unchanged when the page can't draw them.
    // The key is in the source's format.
    unsafe {
        if stretch(
            dst,
            src,
            pco,
            pxlo,
            prcl_dst,
            prcl_src,
            Some(trans_color),
            true,
        ) {
            return TRUE;
        }
        EngTransparentBlt(
            dst,
            src,
            pco,
            pxlo,
            prcl_dst,
            prcl_src,
            trans_color,
            reserved,
        )
    }
}

/// # Safety
///
/// GDI's arguments of DrvAlphaBlend.
unsafe fn alpha_blend(
    dst: *mut SurfObj,
    src: *mut SurfObj,
    pco: *mut ClipObj,
    pxlo: *mut XlateObj,
    prcl_dst: *mut Rectl,
    prcl_src: *mut Rectl,
    blend: *mut BlendObj,
) -> bool {
    // SAFETY: GDI passes valid arguments
    unsafe {
        let Some(pdev) = device(&[dst, src]) else {
            return false;
        };
        let (Some(dst_surface), Some(source), Some(dst_rect), Some(src_rect), Some(blend)) = (
            target(pdev, dst),
            src.as_ref(),
            prcl_dst.as_ref(),
            prcl_src.as_ref(),
            blend.as_ref(),
        ) else {
            return false;
        };
        let Some(src_surface) = surface(pdev, source) else {
            return false;
        };
        let (dst_rect, src_rect) = (rect(dst_rect), rect(src_rect));
        if dst_surface.bpp != 32
            || src_surface.bpp != 32
            || blend.blend_op != AC_SRC_OVER
            || dst_rect.width() != src_rect.width()
            || dst_rect.height() != src_rect.height()
            || dst_rect.is_empty()
            || !matches!(xlate(pxlo, 32, 32), Some(Xlate::Identity))
        {
            return false;
        }
        let alpha = Alpha {
            flags: if blend.alpha_format & AC_SRC_ALPHA != 0 {
                GPU_ALPHA_PER_PIXEL
            } else {
                0
            },
            alpha: u32::from(blend.source_constant_alpha),
            dst: dst_surface,
            src: src_surface,
            dst_rect,
            src_rect,
        };
        let Some(rects) = clips(pdev, pco, dst_rect, CD_ANY) else {
            return false;
        };
        run(
            pdev,
            |command| alpha.encode(command),
            Alpha::CLIP_COUNT,
            rects,
        )
    }
}

pub(crate) unsafe extern "system" fn drv_alpha_blend(
    dst: *mut SurfObj,
    src: *mut SurfObj,
    pco: *mut ClipObj,
    pxlo: *mut XlateObj,
    prcl_dst: *mut Rectl,
    prcl_src: *mut Rectl,
    blend: *mut BlendObj,
) -> Bool {
    // SAFETY: GDI's arguments, which go to the engine unchanged when the page can't draw them
    unsafe {
        if alpha_blend(dst, src, pco, pxlo, prcl_dst, prcl_src, blend) {
            return TRUE;
        }
        EngAlphaBlend(dst, src, pco, pxlo, prcl_dst, prcl_src, blend)
    }
}

/// A DirectDraw blit between VRAM surfaces of the display format
///
/// # Safety
///
/// Kernel mode, the PDEV's buffers are allocated.
pub(crate) unsafe fn direct_draw_blt(pdev: &mut Pdev, blt: &Blt, clips: &[Rect]) -> bool {
    // SAFETY: The caller holds the PDEV
    unsafe { run(pdev, |command| blt.encode(command), Blt::CLIP_COUNT, clips) }
}

const _: () = assert!(size_of::<Rbrush>() == PATTERN_PIXELS * 4);
const _: () = assert!(rop2_to_rop3(R2_COPYPEN) == ROP_PATCOPY);
const _: () = assert!(rop2_to_rop3(1) == 0x00);
const _: () = assert!(rop2_to_rop3(16) == 0xFF);
// R2_NOT inverts the destination like DSTINVERT, R2_XORPEN is PATINVERT
const _: () = assert!(rop2_to_rop3(6) == 0x55);
const _: () = assert!(rop2_to_rop3(7) == 0x5A);
