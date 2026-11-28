/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The DirectDraw HAL: VRAM surfaces in the display format from a rectangular heap DirectDraw
//! manages, blits by the page, flips through the VBE's first row and the page's 60 Hz timeline for
//! vertical blanks. 8 bpp palettes reach the DAC through GDI's DrvSetPalette: DirectDraw refuses
//! the HAL of 8 bpp modes that claim palette support. 3D, texture, Z and overlay surfaces come with
//! the Direct3D DDI of a later phase.

use core::{ptr, slice};

use v86::ddi::*;
use v86::ddk::{
    IOCTL_VIDEO_SHARE_VIDEO_MEMORY, IOCTL_VIDEO_UNSHARE_VIDEO_MEMORY, VideoShareMemory,
    VideoShareMemoryInformation,
};
use v86::gpu::*;
use v86::{port, vbe};

use crate::{MAX_RECTS, Pdev, gdi, ioctl, pdev};

const TRUE: Bool = 1;
/// Spins of a vertical blank wait before giving up on the page
const MAX_SPINS: u32 = 1_000_000;
/// DirectDraw's flip chains start on a row, the VBE's Y offset is 16 bits
const MAX_ROWS: u32 = 0xFFFF;

/// # Safety
///
/// The DirectDraw global object is valid and belongs to the driver.
unsafe fn device<'a>(lp_dd: *mut DdDirectDrawGlobal) -> &'a mut Pdev {
    // SAFETY: DirectDraw passes the global object of this PDEV
    unsafe { pdev((*lp_dd).dhpdev) }
}

fn pixel_format(bpp: u32) -> DdPixelFormat {
    let (flags, red, green, blue) = match bpp {
        8 => (DDPF_RGB | DDPF_PALETTEINDEXED8, 0, 0, 0),
        16 => (DDPF_RGB, 0xF800, 0x07E0, 0x001F),
        _ => (DDPF_RGB, 0xFF_0000, 0xFF00, 0xFF),
    };
    DdPixelFormat {
        dw_size: size_of::<DdPixelFormat>() as u32,
        dw_flags: flags,
        dw_four_cc: 0,
        dw_rgb_bit_count: bpp,
        dw_r_bit_mask: red,
        dw_g_bit_mask: green,
        dw_b_bit_mask: blue,
        dw_rgb_alpha_bit_mask: 0,
    }
}

/// The rows below the primary, with room for the flips' 16 bit row
fn heap_rows(pdev: &Pdev) -> u32 {
    let mode = &pdev.mode;
    (pdev.vram / mode.pitch.max(1))
        .min(MAX_ROWS)
        .saturating_sub(mode.height)
}

pub(crate) unsafe extern "system" fn drv_get_direct_draw_info(
    dhpdev: Dhpdev,
    hal: *mut DdHalInfo,
    heap_count: *mut u32,
    heaps: *mut VideoMemory,
    four_cc_count: *mut u32,
    _four_cc: *mut u32,
) -> Bool {
    // SAFETY: GDI passes the PDEV and the output structures, the heap list only on the second call
    unsafe {
        let pdev = pdev(dhpdev);
        if !pdev.host || pdev.screen.is_null() {
            return 0;
        }
        let mode = pdev.mode;
        let rows = heap_rows(pdev);

        // SAFETY: The HAL info is plain data, zero is empty for every field
        let mut info: DdHalInfo = core::mem::zeroed();
        info.dw_size = size_of::<DdHalInfo>() as u32;
        info.vmi_data.fp_primary = 0;
        info.vmi_data.dw_display_width = mode.width;
        info.vmi_data.dw_display_height = mode.height;
        info.vmi_data.l_display_pitch = mode.pitch as i32;
        info.vmi_data.ddpf_display = pixel_format(mode.bpp);
        info.vmi_data.dw_offscreen_align = 4;
        info.vmi_data.pv_primary = pdev.screen.cast();

        let caps = &mut info.dd_caps;
        caps.dw_size = size_of::<DdNtCoreCaps>() as u32;
        caps.dw_caps = DDCAPS_BLT | DDCAPS_BLTCOLORFILL | DDCAPS_BLTSTRETCH | DDCAPS_COLORKEY;
        caps.dw_caps2 = DDCAPS2_WIDESURFACES | DDCAPS2_NOPAGELOCKREQUIRED | DDCAPS2_FLIPNOVSYNC;
        caps.dw_ckey_caps = DDCKEYCAPS_SRCBLT | DDCKEYCAPS_DESTBLT;
        caps.dw_fx_caps = DDFXCAPS_BLTSTRETCHX
            | DDFXCAPS_BLTSTRETCHXN
            | DDFXCAPS_BLTSTRETCHY
            | DDFXCAPS_BLTSTRETCHYN
            | DDFXCAPS_BLTSHRINKX
            | DDFXCAPS_BLTSHRINKXN
            | DDFXCAPS_BLTSHRINKY
            | DDFXCAPS_BLTSHRINKYN
            | DDFXCAPS_BLTMIRRORLEFTRIGHT
            | DDFXCAPS_BLTMIRRORUPDOWN;
        caps.dw_vid_mem_total = rows * mode.pitch;
        caps.dw_vid_mem_free = caps.dw_vid_mem_total;
        caps.dw_rops[(ROP_SRCCOPY / 32) as usize] |= 1 << (ROP_SRCCOPY % 32);
        caps.dds_caps = DDSCAPS_PRIMARYSURFACE
            | DDSCAPS_OFFSCREENPLAIN
            | DDSCAPS_FLIP
            | DDSCAPS_BACKBUFFER
            | DDSCAPS_FRONTBUFFER
            | DDSCAPS_COMPLEX
            | DDSCAPS_VIDEOMEMORY
            | DDSCAPS_LOCALVIDMEM;
        info.get_driver_info = Some(dd_get_driver_info);
        info.dw_flags = DDHALINFO_GETDRIVERINFOSET;
        hal.write(info);

        // One rectangular heap as wide as the primary, so full width surfaces start on a row
        *heap_count = u32::from(rows > 0);
        if !heaps.is_null() && rows > 0 {
            heaps.write(VideoMemory {
                dw_flags: VIDMEM_ISRECTANGULAR,
                fp_start: (mode.height * mode.pitch) as usize,
                fp_end_or_width: mode.pitch as usize,
                dds_caps: 0,
                dds_caps_alt: 0,
                height: rows as usize,
            });
        }
        *four_cc_count = 0;
        TRUE
    }
}

pub(crate) unsafe extern "system" fn drv_enable_direct_draw(
    _dhpdev: Dhpdev,
    callbacks: *mut DdCallbacks,
    surface_callbacks: *mut DdSurfaceCallbacks,
    palette_callbacks: *mut DdPaletteCallbacks,
) -> Bool {
    // SAFETY: GDI passes the callback tables to fill
    unsafe {
        callbacks.write(DdCallbacks {
            dw_size: size_of::<DdCallbacks>() as u32,
            dw_flags: DDHAL_CB32_WAITFORVERTICALBLANK
                | DDHAL_CB32_CANCREATESURFACE
                | DDHAL_CB32_GETSCANLINE
                | DDHAL_CB32_MAPMEMORY,
            destroy_driver: ptr::null(),
            create_surface: ptr::null(),
            set_color_key: ptr::null(),
            set_mode: ptr::null(),
            wait_for_vertical_blank: Some(dd_wait_for_vertical_blank),
            can_create_surface: Some(dd_can_create_surface),
            create_palette: ptr::null(),
            get_scan_line: Some(dd_get_scan_line),
            map_memory: Some(dd_map_memory),
        });
        surface_callbacks.write(DdSurfaceCallbacks {
            dw_size: size_of::<DdSurfaceCallbacks>() as u32,
            dw_flags: DDHAL_SURFCB32_FLIP
                | DDHAL_SURFCB32_BLT
                | DDHAL_SURFCB32_GETBLTSTATUS
                | DDHAL_SURFCB32_GETFLIPSTATUS,
            destroy_surface: ptr::null(),
            flip: Some(dd_flip),
            set_clip_list: ptr::null(),
            lock: ptr::null(),
            unlock: ptr::null(),
            blt: Some(dd_blt),
            set_color_key: ptr::null(),
            add_attached_surface: ptr::null(),
            get_blt_status: Some(dd_get_status),
            get_flip_status: Some(dd_get_status),
            update_overlay: ptr::null(),
            set_overlay_position: ptr::null(),
            reserved4: ptr::null(),
            set_palette: None,
        });
        palette_callbacks.write(DdPaletteCallbacks {
            dw_size: size_of::<DdPaletteCallbacks>() as u32,
            dw_flags: 0,
            destroy_palette: ptr::null(),
            set_entries: None,
        });
        TRUE
    }
}

pub(crate) unsafe extern "system" fn drv_disable_direct_draw(_dhpdev: Dhpdev) {
    // SAFETY: The VBE belongs to the adapter
    unsafe { vbe::write(vbe::Y_OFFSET, 0) };
}

unsafe extern "system" fn dd_get_driver_info(data: *mut DdGetDriverInfoData) -> u32 {
    // SAFETY: DirectDraw passes a request with a buffer of the expected size
    unsafe {
        let data = &mut *data;
        data.dd_r_val = DDERR_CURRENTLYNOTAVAIL;
        if data.guid_info == GUID_NT_CALLBACKS
            && data.dw_expected_size as usize >= size_of::<DdNtCallbacks>()
        {
            data.lpv_data.cast::<DdNtCallbacks>().write(DdNtCallbacks {
                dw_size: size_of::<DdNtCallbacks>() as u32,
                dw_flags: DDHAL_NTCB32_FLIPTOGDISURFACE,
                free_driver_memory: ptr::null(),
                set_exclusive_mode: ptr::null(),
                flip_to_gdi_surface: Some(dd_flip_to_gdi_surface),
            });
            data.dw_actual_size = size_of::<DdNtCallbacks>() as u32;
            data.dd_r_val = DD_OK;
        }
        DDHAL_DRIVER_HANDLED
    }
}

/// Maps the VRAM into the process that locks a surface, its address plus a surface's offset is the
/// surface
unsafe extern "system" fn dd_map_memory(data: *mut DdMapMemoryData) -> u32 {
    // SAFETY: DirectDraw passes the request of a process of this PDEV
    unsafe {
        let data = &mut *data;
        let pdev = device(data.lp_dd);
        let mut share = VideoShareMemory {
            process_handle: data.h_process,
            view_offset: 0,
            view_size: pdev.vram,
            requested_virtual_address: ptr::null_mut(),
        };
        let done = if data.b_map != 0 {
            let mut info = VideoShareMemoryInformation {
                shared_view_offset: 0,
                shared_view_size: 0,
                virtual_address: ptr::null_mut(),
            };
            let mapped = ioctl(
                pdev.driver,
                IOCTL_VIDEO_SHARE_VIDEO_MEMORY,
                (&raw const share).cast(),
                size_of::<VideoShareMemory>(),
                (&raw mut info).cast(),
                size_of::<VideoShareMemoryInformation>(),
            );
            data.fp_process = info.virtual_address as usize;
            mapped.is_some()
        } else {
            share.view_size = 0;
            share.requested_virtual_address = data.fp_process as *mut _;
            ioctl(
                pdev.driver,
                IOCTL_VIDEO_UNSHARE_VIDEO_MEMORY,
                (&raw const share).cast(),
                size_of::<VideoShareMemory>(),
                ptr::null_mut(),
                0,
            )
            .is_some()
        };
        data.dd_r_val = if done { DD_OK } else { DDERR_GENERIC };
        DDHAL_DRIVER_HANDLED
    }
}

unsafe extern "system" fn dd_can_create_surface(data: *mut DdCanCreateSurfaceData) -> u32 {
    const LATER: u32 = DDSCAPS_TEXTURE
        | DDSCAPS_3DDEVICE
        | DDSCAPS_ZBUFFER
        | DDSCAPS_OVERLAY
        | DDSCAPS_EXECUTEBUFFER;
    // SAFETY: DirectDraw passes the request with its surface description
    unsafe {
        let data = &mut *data;
        let caps = (*data.lp_dd_surface_desc).dds_caps;
        data.dd_r_val = if caps & DDSCAPS_SYSTEMMEMORY != 0 {
            DD_OK
        } else if caps & LATER != 0 {
            DDERR_UNSUPPORTED
        } else if data.b_is_different_pixel_format != 0 {
            DDERR_INVALIDPIXELFORMAT
        } else {
            DD_OK
        };
        DDHAL_DRIVER_HANDLED
    }
}

/// The page's scanline, with [`V86_GPU_VBLANK`] during the blank
///
/// # Safety
///
/// Kernel mode.
unsafe fn scanline() -> u32 {
    // SAFETY: Kernel mode, the backdoor is the page's
    unsafe { port::call(V86_CMD_GPU_SCANLINE, 0) }
}

/// Spins until the blank is in the given state
///
/// # Safety
///
/// Kernel mode.
unsafe fn wait_blank(blank: bool) {
    for _ in 0..MAX_SPINS {
        // SAFETY: Kernel mode
        if (unsafe { scanline() } & V86_GPU_VBLANK != 0) == blank {
            return;
        }
    }
}

unsafe extern "system" fn dd_wait_for_vertical_blank(data: *mut DdWaitForVerticalBlankData) -> u32 {
    // SAFETY: DirectDraw passes the request
    unsafe {
        let data = &mut *data;
        match data.dw_flags {
            DDWAITVB_I_TESTVB => data.b_is_in_vb = u32::from(scanline() & V86_GPU_VBLANK != 0),
            DDWAITVB_BLOCKBEGIN => {
                wait_blank(false);
                wait_blank(true);
            }
            DDWAITVB_BLOCKEND => {
                wait_blank(true);
                wait_blank(false);
            }
            _ => {}
        }
        data.dd_r_val = DD_OK;
        DDHAL_DRIVER_HANDLED
    }
}

unsafe extern "system" fn dd_get_scan_line(data: *mut DdGetScanLineData) -> u32 {
    // SAFETY: DirectDraw passes the request
    unsafe {
        let data = &mut *data;
        let line = scanline();
        if line & V86_GPU_VBLANK != 0 {
            data.dd_r_val = DDERR_VERTICALBLANKINPROGRESS;
        } else {
            data.dw_scan_line = line & 0xFFFF;
            data.dd_r_val = DD_OK;
        }
        DDHAL_DRIVER_HANDLED
    }
}

/// The page draws synchronously, every blit and flip is done when its call returns
unsafe extern "system" fn dd_get_status(data: *mut DdGetStatusData) -> u32 {
    // SAFETY: DirectDraw passes the request
    unsafe { (*data).dd_r_val = DD_OK };
    DDHAL_DRIVER_HANDLED
}

/// A VRAM surface of the display format
///
/// # Safety
///
/// The surface is valid.
unsafe fn surface(pdev: &Pdev, local: *mut DdSurfaceLocal) -> Option<Surface> {
    // SAFETY: DirectDraw passes valid surfaces with their global parts
    unsafe {
        let local = local.as_ref()?;
        let global = local.lp_gbl.as_ref()?;
        (local.dds_caps & DDSCAPS_SYSTEMMEMORY == 0 && global.l_pitch > 0).then_some(Surface {
            kind: GPU_SURFACE_VRAM,
            address: global.fp_vid_mem as u32,
            pitch: global.l_pitch,
            bpp: pdev.mode.bpp,
            width: global.w_width,
            height: global.w_height,
        })
    }
}

/// # Safety
///
/// DirectDraw's blit request.
unsafe fn blt(pdev: &mut Pdev, data: &DdBltData) -> Option<()> {
    // SAFETY: DirectDraw passes the surfaces of the blit and its clip list
    unsafe {
        let flags = data.dw_flags;
        let fx = &data.blt_fx;
        let mut blt = Blt {
            dst: surface(pdev, data.lp_dd_dest_surface)?,
            ..Blt::default()
        };
        let rop = if flags & DDBLT_ROP != 0 {
            (fx.dw_rop >> 16) & 0xFF
        } else {
            ROP_SRCCOPY
        };
        if flags & DDBLT_COLORFILL != 0 {
            blt.rop = ROP_PATCOPY;
            blt.color = fx.dw_fill_color;
        } else if rop_uses_pattern(rop) {
            return None;
        } else if rop_uses_source(rop) {
            let source = data.lp_dd_src_surface;
            blt.src = surface(pdev, source)?;
            blt.flags |= GPU_BLT_SOURCE;
            blt.rop = rop;
            if flags & DDBLT_KEYSRCOVERRIDE != 0 {
                blt.flags |= GPU_BLT_SOURCE_KEY;
                blt.src_key = fx.ddck_src_colorkey.low;
            } else if flags & DDBLT_KEYSRC != 0 {
                blt.flags |= GPU_BLT_SOURCE_KEY;
                blt.src_key = (*source).ddck_ck_src_blt.low;
            }
        } else {
            blt.rop = rop;
        }
        if flags & DDBLT_KEYDESTOVERRIDE != 0 {
            blt.flags |= GPU_BLT_DEST_KEY;
            blt.dst_key = fx.ddck_dest_colorkey.low;
        } else if flags & DDBLT_KEYDEST != 0 {
            blt.flags |= GPU_BLT_DEST_KEY;
            blt.dst_key = (*data.lp_dd_dest_surface).ddck_ck_dest_blt.low;
        }
        if flags & DDBLT_DDFX != 0 {
            if fx.dw_ddfx & !(DDBLTFX_MIRRORLEFTRIGHT | DDBLTFX_MIRRORUPDOWN) != 0 {
                return None;
            }
            if fx.dw_ddfx & DDBLTFX_MIRRORLEFTRIGHT != 0 {
                blt.flags |= GPU_BLT_MIRROR_X;
            }
            if fx.dw_ddfx & DDBLTFX_MIRRORUPDOWN != 0 {
                blt.flags |= GPU_BLT_MIRROR_Y;
            }
        }
        let supported = DDBLT_COLORFILL
            | DDBLT_ROP
            | DDBLT_KEYSRC
            | DDBLT_KEYSRCOVERRIDE
            | DDBLT_KEYDEST
            | DDBLT_KEYDESTOVERRIDE
            | DDBLT_DDFX
            | 0x0100_0000 // DDBLT_WAIT
            | 0x0800_0000 // DDBLT_DONOTWAIT
            | 0x200; // DDBLT_ASYNC
        if flags & !supported != 0 {
            return None;
        }

        // NT clips in the runtime, a clipped blit has its whole rectangles and a clip list
        let (dst_rect, src_rect) = if data.is_clipped != 0 {
            (data.r_orig_dest, data.r_orig_src)
        } else {
            (data.r_dest, data.r_src)
        };
        blt.dst_rect = Rect::new(dst_rect.left, dst_rect.top, dst_rect.right, dst_rect.bottom);
        blt.src_rect = Rect::new(src_rect.left, src_rect.top, src_rect.right, src_rect.bottom);
        let out = slice::from_raw_parts_mut(pdev.clips, MAX_RECTS);
        let count = if data.is_clipped != 0 && !data.pr_dest_rects.is_null() {
            let rects = slice::from_raw_parts(data.pr_dest_rects, data.dw_rect_cnt as usize);
            if rects.len() > out.len() {
                return None;
            }
            for (slot, clip) in out.iter_mut().zip(rects) {
                *slot = Rect::new(clip.left, clip.top, clip.right, clip.bottom)
                    .intersect(&blt.dst_rect);
            }
            rects.len()
        } else {
            out[0] = Rect::new(
                data.r_dest.left,
                data.r_dest.top,
                data.r_dest.right,
                data.r_dest.bottom,
            );
            1
        };
        let clips = slice::from_raw_parts(pdev.clips, count);
        gdi::direct_draw_blt(pdev, &blt, clips).then_some(())
    }
}

unsafe extern "system" fn dd_blt(data: *mut DdBltData) -> u32 {
    // SAFETY: DirectDraw passes the request of this PDEV
    unsafe {
        let data = &mut *data;
        let pdev = device(data.lp_dd);
        if blt(pdev, data).is_none() {
            return DDHAL_DRIVER_NOTHANDLED;
        }
        data.dd_r_val = DD_OK;
        DDHAL_DRIVER_HANDLED
    }
}

/// Shows the target from its first row, the page draws the new row from the next frame on
unsafe extern "system" fn dd_flip(data: *mut DdFlipData) -> u32 {
    // SAFETY: DirectDraw passes the request with the target of this PDEV, the VBE belongs to the
    // adapter
    unsafe {
        let data = &mut *data;
        let pdev = device(data.lp_dd);
        let pitch = pdev.mode.pitch as usize;
        let offset = (*(*data.lp_surf_targ).lp_gbl).fp_vid_mem;
        let row = offset / pitch.max(1);
        if !offset.is_multiple_of(pitch.max(1)) || row > MAX_ROWS as usize {
            data.dd_r_val = DDERR_GENERIC;
            return DDHAL_DRIVER_HANDLED;
        }
        vbe::write(vbe::Y_OFFSET, row as u16);
        data.dd_r_val = DD_OK;
        DDHAL_DRIVER_HANDLED
    }
}

unsafe extern "system" fn dd_flip_to_gdi_surface(data: *mut DdFlipToGdiSurfaceData) -> u32 {
    // SAFETY: DirectDraw passes the request, the VBE belongs to the adapter
    unsafe {
        vbe::write(vbe::Y_OFFSET, 0);
        (*data).dd_r_val = DD_OK;
    }
    DDHAL_DRIVER_HANDLED
}
