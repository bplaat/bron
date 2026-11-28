/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! v86disp.dll - the display driver of the paravirtualized graphics adapter.
//!
//! Replaces XP's framebuf.dll on top of the miniport v86vid.sys. The primary surface is the mapped
//! VRAM, GDI draws into it directly, the hooked calls are executed by the page over the backdoor
//! (`gdi.rs`) and fall back to the engine when the page declines. DirectDraw gets a HAL with VRAM
//! surfaces, blits and flips (`ddraw.rs`). Display drivers may only import from win32k.sys.

#![no_std]
#![no_main]

mod ddraw;
mod gdi;

use core::ffi::c_void;
use core::panic::PanicInfo;
use core::{ptr, slice};

use v86::ddi::*;
use v86::ddk::{
    IOCTL_VIDEO_DISABLE_POINTER, IOCTL_VIDEO_MAP_VIDEO_MEMORY, IOCTL_VIDEO_QUERY_AVAIL_MODES,
    IOCTL_VIDEO_QUERY_NUM_AVAIL_MODES, IOCTL_VIDEO_QUERY_POINTER_CAPABILITIES,
    IOCTL_VIDEO_RESET_DEVICE, IOCTL_VIDEO_SET_COLOR_REGISTERS, IOCTL_VIDEO_SET_CURRENT_MODE,
    IOCTL_VIDEO_SET_POINTER_ATTR, IOCTL_VIDEO_SET_POINTER_POSITION, IOCTL_VIDEO_UNMAP_VIDEO_MEMORY,
    VIDEO_MODE_COLOR_POINTER, VIDEO_MODE_GRAPHICS, VIDEO_MODE_MONO_POINTER,
    VIDEO_POINTER_ATTRIBUTES_SIZE, VideoMemoryInformation, VideoModeInformation, VideoNumModes,
    VideoPointerCapabilities, VideoPointerPosition,
};
use v86::gpu::Rect;
use v86::protocol::{V86_CMD_HELLO, V86_HELLO_REPLY};
use v86::{port, vbe};

/// 'v86d', the pool tag of the driver's memory
const TAG: u32 = u32::from_le_bytes(*b"v86d");
/// 64 KiB, the largest command
pub(crate) const COMMAND_WORDS: usize = 16 * 1024;
/// Clip rectangles of one call, more are left to the engine
pub(crate) const MAX_RECTS: usize = 1024;
const POINTER_SIZE: usize = 32;
const POINTER_STRIDE: usize = POINTER_SIZE / 8;
/// The AND then the XOR mask after the attributes, which end where the C structure has its first
/// pixel byte
const POINTER_PIXELS: usize = 24;
const POINTER_BUFFER: usize = VIDEO_POINTER_ATTRIBUTES_SIZE + 2 * POINTER_STRIDE * POINTER_SIZE;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Mode {
    pub index: u32,
    pub width: u32,
    pub height: u32,
    pub bpp: u32,
    pub pitch: u32,
    pub frequency: u32,
    pub red: u32,
    pub green: u32,
    pub blue: u32,
    pub x_mm: u32,
    pub y_mm: u32,
}

impl Mode {
    fn from_info(info: &VideoModeInformation) -> Self {
        Self {
            index: info.mode_index,
            width: info.vis_screen_width,
            height: info.vis_screen_height,
            bpp: info.bits_per_plane * info.number_of_planes,
            pitch: info.screen_stride,
            frequency: info.frequency,
            red: info.red_mask,
            green: info.green_mask,
            blue: info.blue_mask,
            x_mm: info.x_millimeter,
            y_mm: info.y_millimeter,
        }
    }

    const fn format(&self) -> u32 {
        match self.bpp {
            8 => BMF_8BPP,
            16 => BMF_16BPP,
            _ => BMF_32BPP,
        }
    }

    const fn same_size(&self, other: &Self) -> bool {
        self.width == other.width && self.height == other.height && self.bpp == other.bpp
    }
}

/// The physical device, in zeroed nonpaged memory
pub(crate) struct Pdev {
    /// The miniport
    pub driver: Handle,
    pub hdev: Hdev,
    pub hsurf: Hsurf,
    pub palette: Hpalette,
    pub mode: Mode,
    /// The mapped VRAM while the surface is enabled, the primary at its start
    pub screen: *mut u8,
    pub vram: u32,
    /// The page answers the backdoor
    pub host: bool,
    pub hardware_pointer: bool,
    pub pointer_hot: Pointl,
    pub commands: *mut u32,
    pub clips: *mut Rect,
    pub rects: *mut Rect,
}

impl Pdev {
    /// # Safety
    ///
    /// The PDEV's buffers are allocated.
    pub(crate) unsafe fn commands(&mut self) -> &mut [u32] {
        // SAFETY: The buffer holds COMMAND_WORDS words and belongs to the PDEV
        unsafe { slice::from_raw_parts_mut(self.commands, COMMAND_WORDS) }
    }

    /// The primary surface's pixel offset in VRAM of a linear address in the mapped VRAM
    pub(crate) fn vram_offset(&self, address: *const c_void) -> Option<u32> {
        let offset = (address as usize).checked_sub(self.screen as usize)?;
        (!self.screen.is_null() && offset < self.vram as usize).then_some(offset as u32)
    }
}

/// # Safety
///
/// `dhpdev` comes from [`drv_enable_pdev`].
pub(crate) unsafe fn pdev<'a>(dhpdev: Dhpdev) -> &'a mut Pdev {
    // SAFETY: GDI passes back the PDEV of the driver
    unsafe { &mut *dhpdev.cast::<Pdev>() }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    // SAFETY: Stops the machine with MANUALLY_INITIATED_CRASH, display drivers can't unwind
    unsafe { EngBugCheckEx(0xE2, 0, 0, 0, 0) }
}

/// # Safety
///
/// The buffers have their sizes, `driver` is the miniport.
pub(crate) unsafe fn ioctl(
    driver: Handle,
    code: u32,
    input: *const c_void,
    input_size: usize,
    output: *mut c_void,
    output_size: usize,
) -> Option<usize> {
    let mut returned = 0;
    // SAFETY: The caller passes valid buffers of the given sizes
    let status = unsafe {
        EngDeviceIoControl(
            driver,
            code,
            input.cast_mut(),
            input_size as u32,
            output,
            output_size as u32,
            &mut returned,
        )
    };
    (status == 0).then_some(returned as usize)
}

/// # Safety
///
/// Kernel mode.
unsafe fn allocate<T>(count: usize) -> *mut T {
    // SAFETY: Zeroed nonpaged pool, freed with EngFreeMem
    unsafe {
        EngAllocMem(
            FL_ZERO_MEMORY | FL_NONPAGED_MEMORY,
            (count * size_of::<T>()) as u32,
            TAG,
        )
        .cast()
    }
}

/// The graphics modes of the miniport, freed on drop
struct Modes {
    info: *mut VideoModeInformation,
    count: usize,
}

impl Modes {
    /// # Safety
    ///
    /// `driver` is the miniport.
    unsafe fn query(driver: Handle) -> Option<Self> {
        let mut num = VideoNumModes {
            num_modes: 0,
            mode_information_length: 0,
        };
        // SAFETY: The buffers have their sizes, the mode list fits the allocation
        unsafe {
            ioctl(
                driver,
                IOCTL_VIDEO_QUERY_NUM_AVAIL_MODES,
                ptr::null(),
                0,
                (&raw mut num).cast(),
                size_of::<VideoNumModes>(),
            )?;
            if num.mode_information_length as usize != size_of::<VideoModeInformation>() {
                return None;
            }
            let count = num.num_modes as usize;
            let info = allocate::<VideoModeInformation>(count.max(1));
            let modes = Self { info, count };
            if info.is_null() {
                return None;
            }
            ioctl(
                driver,
                IOCTL_VIDEO_QUERY_AVAIL_MODES,
                ptr::null(),
                0,
                info.cast(),
                count * size_of::<VideoModeInformation>(),
            )?;
            Some(modes)
        }
    }

    /// Packed pixel graphics modes in the depths GDI gets from the driver
    fn iter(&self) -> impl Iterator<Item = Mode> + '_ {
        // SAFETY: The miniport filled `count` entries
        unsafe { slice::from_raw_parts(self.info, self.count) }
            .iter()
            .filter(|info| {
                info.number_of_planes == 1
                    && info.attribute_flags & VIDEO_MODE_GRAPHICS != 0
                    && matches!(info.bits_per_plane, 8 | 16 | 32)
            })
            .map(Mode::from_info)
    }
}

impl Drop for Modes {
    fn drop(&mut self) {
        // SAFETY: The list was allocated with EngAllocMem
        unsafe { EngFreeMem(self.info.cast()) };
    }
}

/// The mode a DEVMODE asks for, the first mode when it leaves the fields empty
fn select_mode(modes: &Modes, devmode: &DevModeW) -> Option<Mode> {
    let unset = devmode.dm_pels_width == 0 && devmode.dm_bits_per_pel == 0;
    modes.iter().find(|mode| {
        unset
            || (mode.width == devmode.dm_pels_width
                && mode.height == devmode.dm_pels_height
                && mode.bpp == devmode.dm_bits_per_pel
                && (devmode.dm_display_frequency <= 1
                    || mode.frequency == devmode.dm_display_frequency))
    })
}

const fn face_name(name: &str) -> [u16; 32] {
    let bytes = name.as_bytes();
    let mut face = [0; 32];
    let mut i = 0;
    while i < bytes.len() && i < 31 {
        face[i] = bytes[i] as u16;
        i += 1;
    }
    face
}

const fn log_font(
    height: i32,
    width: i32,
    weight: i32,
    pitch: u8,
    clip: u8,
    quality: u8,
    name: &str,
) -> LogFontW {
    LogFontW {
        lf_height: height,
        lf_width: width,
        lf_escapement: 0,
        lf_orientation: 0,
        lf_weight: weight,
        lf_italic: 0,
        lf_underline: 0,
        lf_strike_out: 0,
        lf_char_set: 0,
        lf_out_precision: 0,
        lf_clip_precision: clip,
        lf_quality: quality,
        lf_pitch_and_family: pitch,
        lf_face_name: face_name(name),
    }
}

// The default fonts of the DDK's display samples: variable pitch System, MS Sans Serif and fixed
// pitch Courier, the last two with stroke clipping and proof quality
const SYSTEM_FONT: LogFontW = log_font(16, 7, 700, 2, 0, 0, "System");
const SANS_FONT: LogFontW = log_font(12, 9, 400, 2, 2, 2, "MS Sans Serif");
const FIXED_FONT: LogFontW = log_font(12, 9, 400, 1, 2, 2, "Courier");

fn gdi_info(mode: &Mode) -> GdiInfo {
    let palette = mode.bpp == 8;
    let (dac_red, dac_green, dac_blue) = if mode.bpp == 16 { (5, 6, 5) } else { (8, 8, 8) };
    let chroma = |x, y| CieChroma {
        x,
        y,
        y_luminance: 0,
    };
    GdiInfo {
        ul_version: GDI_DRIVER_VERSION,
        ul_technology: DT_RASDISPLAY,
        ul_horz_size: mode.x_mm,
        ul_vert_size: mode.y_mm,
        ul_horz_res: mode.width,
        ul_vert_res: mode.height,
        c_bits_pixel: mode.bpp,
        c_planes: 1,
        ul_num_colors: if palette { 20 } else { u32::MAX },
        ul_log_pixels_x: 96,
        ul_log_pixels_y: 96,
        fl_text_caps: TC_RA_ABLE,
        ul_dac_red: dac_red,
        ul_dac_green: dac_green,
        ul_dac_blue: dac_blue,
        ul_aspect_x: 36,
        ul_aspect_y: 36,
        ul_aspect_xy: 51,
        x_style_step: 1,
        y_style_step: 1,
        den_style_step: 3,
        ul_num_pal_reg: if palette { 256 } else { 0 },
        // The sRGB like defaults of the DDK's display samples
        ci_device: ColorInfo {
            red: chroma(6700, 3300),
            green: chroma(2100, 7100),
            blue: chroma(1400, 800),
            cyan: chroma(1750, 3950),
            magenta: chroma(4050, 2050),
            yellow: chroma(4400, 5200),
            alignment_white: chroma(3127, 3290),
            red_gamma: 20000,
            green_gamma: 20000,
            blue_gamma: 20000,
            dyes: [0; 6],
        },
        ul_primary_order: PRIMARY_ORDER_CBA,
        ul_ht_pattern_size: HT_PATSIZE_4X4_M,
        ul_ht_output_format: match mode.bpp {
            8 => HT_FORMAT_8BPP,
            16 => HT_FORMAT_16BPP,
            _ => HT_FORMAT_32BPP,
        },
        fl_ht_flags: HT_FLAG_ADDITIVE_PRIMS,
        ul_v_refresh: mode.frequency,
        ..GdiInfo::default()
    }
}

fn dev_info(mode: &Mode, palette: Hpalette) -> DevInfo {
    let mut caps = GCAPS_OPAQUERECT
        | GCAPS_ALTERNATEFILL
        | GCAPS_WINDINGFILL
        | GCAPS_MONO_DITHER
        | GCAPS_ASYNCMOVE
        | GCAPS_DIRECTDRAW;
    if mode.bpp == 8 {
        caps |= GCAPS_PALMANAGED | GCAPS_COLOR_DITHER;
    }
    DevInfo {
        fl_graphics_caps: caps,
        lf_default_font: SYSTEM_FONT,
        lf_ansi_var_font: SANS_FONT,
        lf_ansi_fix_font: FIXED_FONT,
        c_fonts: 0,
        i_dither_format: mode.format(),
        cx_dither: 8,
        cy_dither: 8,
        hpal_default: palette,
        fl_graphics_caps2: 0,
    }
}

/// The 20 static colors at both ends, a 6x6x6 color cube and grays in between, as 0x00BBGGRR
fn default_palette() -> [u32; 256] {
    const STATIC: [u32; 20] = [
        0x000000, 0x000080, 0x008000, 0x008080, 0x800000, 0x800080, 0x808000, 0xC0C0C0, 0xC0DCC0,
        0xF0CAA6, 0xF0FBFF, 0xA4A0A0, 0x808080, 0x0000FF, 0x00FF00, 0x00FFFF, 0xFF0000, 0xFF00FF,
        0xFFFF00, 0xFFFFFF,
    ];
    let mut colors = [0; 256];
    colors[..10].copy_from_slice(&STATIC[..10]);
    colors[246..].copy_from_slice(&STATIC[10..]);
    for (i, color) in colors[10..226].iter_mut().enumerate() {
        let level = |value: usize| (value as u32) * 51;
        *color = level(i % 6) | level(i / 6 % 6) << 8 | level(i / 36) << 16;
    }
    for (i, color) in colors[226..246].iter_mut().enumerate() {
        let gray = 12 + i as u32 * 12;
        *color = gray | gray << 8 | gray << 16;
    }
    colors
}

/// Loads palette entries (0x00BBGGRR) into the DAC through the miniport
///
/// # Safety
///
/// `driver` is the miniport.
unsafe fn set_palette_entries(driver: Handle, first: u32, colors: &[u32]) -> bool {
    let count = colors.len().min(256 - first.min(256) as usize);
    // VIDEO_CLUT: the entry count and the first entry, then the entries with a zero unused byte
    let mut clut = [0_u32; 257];
    clut[0] = count as u32 | first << 16;
    for (entry, color) in clut[1..].iter_mut().zip(&colors[..count]) {
        *entry = color & 0x00FF_FFFF;
    }
    // SAFETY: The table holds the header and `count` entries
    unsafe {
        ioctl(
            driver,
            IOCTL_VIDEO_SET_COLOR_REGISTERS,
            clut.as_ptr().cast(),
            (1 + count) * size_of::<u32>(),
            ptr::null_mut(),
            0,
        )
    }
    .is_some()
}

/// # Safety
///
/// The PDEV is valid.
unsafe fn set_mode(pdev: &mut Pdev) -> bool {
    // The miniport adds the page's size to its list, look the mode up again by its size
    // SAFETY: The driver handle is the miniport
    let Some(mode) = (unsafe { Modes::query(pdev.driver) })
        .and_then(|modes| modes.iter().find(|mode| mode.same_size(&pdev.mode)))
    else {
        return false;
    };
    pdev.mode.index = mode.index;
    // SAFETY: The mode index is a u32, the miniport takes it as input
    let set = unsafe {
        ioctl(
            pdev.driver,
            IOCTL_VIDEO_SET_CURRENT_MODE,
            (&raw const mode.index).cast(),
            size_of::<u32>(),
            ptr::null_mut(),
            0,
        )
    }
    .is_some();
    if set && pdev.mode.bpp == 8 {
        // SAFETY: The driver handle is the miniport
        unsafe { set_palette_entries(pdev.driver, 0, &default_palette()) };
    }
    set
}

unsafe extern "system" fn drv_enable_pdev(
    devmode: *mut DevModeW,
    _log_address: *mut u16,
    _pattern_count: u32,
    _patterns: *mut Hsurf,
    caps_size: u32,
    caps: *mut u32,
    dev_info_size: u32,
    info: *mut DevInfo,
    _hdev: Hdev,
    _device_name: *mut u16,
    driver: Handle,
) -> Dhpdev {
    // SAFETY: GDI passes the DEVMODE, the capability buffers of the given sizes and the miniport
    unsafe {
        let Some(mode) = Modes::query(driver).and_then(|modes| select_mode(&modes, &*devmode))
        else {
            return ptr::null_mut();
        };
        let pdev = allocate::<Pdev>(1);
        if pdev.is_null() {
            return ptr::null_mut();
        }
        let palette = if mode.bpp == 8 {
            EngCreatePalette(PAL_INDEXED, 256, default_palette().as_ptr(), 0, 0, 0)
        } else {
            EngCreatePalette(
                PAL_BITFIELDS,
                0,
                ptr::null(),
                mode.red,
                mode.green,
                mode.blue,
            )
        };
        let commands = allocate::<u32>(COMMAND_WORDS);
        let clips = allocate::<Rect>(MAX_RECTS);
        let rects = allocate::<Rect>(MAX_RECTS);
        let mut pointer = VideoPointerCapabilities {
            flags: 0,
            max_width: 0,
            max_height: 0,
            hw_ptr_bitmap_start: 0,
            hw_ptr_bitmap_end: 0,
        };
        let hardware_pointer = ioctl(
            driver,
            IOCTL_VIDEO_QUERY_POINTER_CAPABILITIES,
            ptr::null(),
            0,
            (&raw mut pointer).cast(),
            size_of::<VideoPointerCapabilities>(),
        )
        .is_some()
            && pointer.flags & VIDEO_MODE_MONO_POINTER != 0
            && pointer.max_width as usize >= POINTER_SIZE;
        // Field by field into the zeroed allocation, the PDEV is too large for the kernel stack
        let device = &mut *pdev;
        device.driver = driver;
        device.palette = palette;
        device.mode = mode;
        device.host = port::host_present() && port::call(V86_CMD_HELLO, 0) == V86_HELLO_REPLY;
        device.hardware_pointer = hardware_pointer;
        device.commands = commands;
        device.clips = clips;
        device.rects = rects;
        if palette.is_null() || commands.is_null() || clips.is_null() || rects.is_null() {
            drv_disable_pdev(pdev.cast());
            return ptr::null_mut();
        }

        let gdi = gdi_info(&mode);
        let size = (caps_size as usize).min(size_of::<GdiInfo>());
        ptr::copy_nonoverlapping((&raw const gdi).cast::<u8>(), caps.cast::<u8>(), size);
        let dev = dev_info(&mode, palette);
        let size = (dev_info_size as usize).min(size_of::<DevInfo>());
        ptr::copy_nonoverlapping((&raw const dev).cast::<u8>(), info.cast::<u8>(), size);
        pdev.cast()
    }
}

unsafe extern "system" fn drv_complete_pdev(dhpdev: Dhpdev, hdev: Hdev) {
    // SAFETY: GDI passes the PDEV of the driver
    unsafe { pdev(dhpdev).hdev = hdev };
}

unsafe extern "system" fn drv_disable_pdev(dhpdev: Dhpdev) {
    // SAFETY: GDI passes the PDEV of the driver, which owns its palette and buffers
    unsafe {
        let pdev = pdev(dhpdev);
        if !pdev.palette.is_null() {
            EngDeletePalette(pdev.palette);
        }
        for buffer in [
            pdev.commands.cast::<c_void>(),
            pdev.clips.cast(),
            pdev.rects.cast(),
        ] {
            if !buffer.is_null() {
                EngFreeMem(buffer);
            }
        }
        EngFreeMem(dhpdev);
    }
}

unsafe extern "system" fn drv_enable_surface(dhpdev: Dhpdev) -> Hsurf {
    // SAFETY: GDI passes the PDEV of the driver, the miniport maps the VRAM
    unsafe {
        let pdev = pdev(dhpdev);
        if !set_mode(pdev) {
            return ptr::null_mut();
        }
        let mut base: *mut c_void = ptr::null_mut();
        let mut memory = VideoMemoryInformation {
            video_ram_base: ptr::null_mut(),
            video_ram_length: 0,
            frame_buffer_base: ptr::null_mut(),
            frame_buffer_length: 0,
        };
        if ioctl(
            pdev.driver,
            IOCTL_VIDEO_MAP_VIDEO_MEMORY,
            (&raw mut base).cast(),
            size_of::<*mut c_void>(),
            (&raw mut memory).cast(),
            size_of::<VideoMemoryInformation>(),
        )
        .is_none()
        {
            return ptr::null_mut();
        }
        pdev.screen = memory.frame_buffer_base.cast();
        pdev.vram = memory.video_ram_length;

        let size = Sizel {
            cx: pdev.mode.width as i32,
            cy: pdev.mode.height as i32,
        };
        let hsurf = EngCreateDeviceSurface(dhpdev, size, pdev.mode.format());
        if hsurf.is_null() {
            drv_disable_surface(dhpdev);
            return ptr::null_mut();
        }
        let hooks = HOOK_BITBLT
            | HOOK_COPYBITS
            | HOOK_STRETCHBLT
            | HOOK_TEXTOUT
            | HOOK_STROKEPATH
            | HOOK_LINETO
            | HOOK_ALPHABLEND
            | HOOK_TRANSPARENTBLT;
        // GDI draws into the mapped VRAM itself where the page doesn't; XP refuses the surface when
        // it is marked as not in system memory
        if EngModifySurface(
            hsurf,
            pdev.hdev,
            if pdev.host { hooks } else { 0 },
            0,
            dhpdev,
            pdev.screen.cast(),
            pdev.mode.pitch as i32,
            ptr::null_mut(),
        ) == 0
        {
            EngDeleteSurface(hsurf);
            drv_disable_surface(dhpdev);
            return ptr::null_mut();
        }
        pdev.hsurf = hsurf;
        hsurf
    }
}

unsafe extern "system" fn drv_disable_surface(dhpdev: Dhpdev) {
    // SAFETY: GDI passes the PDEV of the driver, the miniport unmaps the VRAM
    unsafe {
        let pdev = pdev(dhpdev);
        if !pdev.hsurf.is_null() {
            EngDeleteSurface(pdev.hsurf);
            pdev.hsurf = ptr::null_mut();
        }
        if !pdev.screen.is_null() {
            let mut base = pdev.screen.cast::<c_void>();
            ioctl(
                pdev.driver,
                IOCTL_VIDEO_UNMAP_VIDEO_MEMORY,
                (&raw mut base).cast(),
                size_of::<*mut c_void>(),
                ptr::null_mut(),
                0,
            );
            pdev.screen = ptr::null_mut();
        }
    }
}

unsafe extern "system" fn drv_assert_mode(dhpdev: Dhpdev, enable: Bool) -> Bool {
    // SAFETY: GDI passes the PDEV of the driver
    unsafe {
        let pdev = pdev(dhpdev);
        if enable == 0 {
            vbe::write(vbe::Y_OFFSET, 0);
            return ioctl(
                pdev.driver,
                IOCTL_VIDEO_RESET_DEVICE,
                ptr::null(),
                0,
                ptr::null_mut(),
                0,
            )
            .is_some() as Bool;
        }
        set_mode(pdev) as Bool
    }
}

unsafe extern "system" fn drv_disable_driver() {}

unsafe extern "system" fn drv_get_modes(driver: Handle, size: u32, devmodes: *mut DevModeW) -> u32 {
    // SAFETY: GDI passes the miniport and a buffer of `size` bytes, or none to ask for the size
    unsafe {
        let Some(modes) = Modes::query(driver) else {
            return 0;
        };
        let count = modes.iter().count();
        if devmodes.is_null() {
            return (count * size_of::<DevModeW>()) as u32;
        }
        let fit = (size as usize / size_of::<DevModeW>()).min(count);
        for (i, mode) in modes.iter().take(fit).enumerate() {
            devmodes.add(i).write(DevModeW {
                dm_device_name: face_name("v86disp"),
                dm_spec_version: DM_SPECVERSION,
                dm_driver_version: DM_SPECVERSION,
                dm_size: size_of::<DevModeW>() as u16,
                dm_driver_extra: 0,
                dm_fields: DM_BITSPERPEL
                    | DM_PELSWIDTH
                    | DM_PELSHEIGHT
                    | DM_DISPLAYFREQUENCY
                    | DM_DISPLAYFLAGS,
                dm_position: Pointl::default(),
                dm_display_orientation: 0,
                dm_display_fixed_output: 0,
                dm_color: 0,
                dm_duplex: 0,
                dm_y_resolution: 0,
                dm_tt_option: 0,
                dm_collate: 0,
                dm_form_name: [0; 32],
                dm_log_pixels: 0,
                dm_bits_per_pel: mode.bpp,
                dm_pels_width: mode.width,
                dm_pels_height: mode.height,
                dm_display_flags: 0,
                dm_display_frequency: mode.frequency,
                dm_icm_method: 0,
                dm_icm_intent: 0,
                dm_media_type: 0,
                dm_dither_type: 0,
                dm_reserved1: 0,
                dm_reserved2: 0,
                dm_panning_width: 0,
                dm_panning_height: 0,
            });
        }
        (fit * size_of::<DevModeW>()) as u32
    }
}

unsafe extern "system" fn drv_set_palette(
    dhpdev: Dhpdev,
    palette: *mut PalObj,
    _flags: u32,
    start: u32,
    count: u32,
) -> Bool {
    let mut colors = [0_u32; 256];
    let count = count.min(256 - start.min(256));
    // SAFETY: GDI passes the PDEV and the palette, the colors fit the table
    unsafe {
        let got = PALOBJ_cGetColors(palette, start, count, colors.as_mut_ptr());
        set_palette_entries(pdev(dhpdev).driver, start, &colors[..got as usize]) as Bool
    }
}

/// # Safety
///
/// The PDEV is valid.
unsafe fn hide_pointer(pdev: &Pdev) {
    // SAFETY: The driver handle is the miniport
    unsafe {
        ioctl(
            pdev.driver,
            IOCTL_VIDEO_DISABLE_POINTER,
            ptr::null(),
            0,
            ptr::null_mut(),
            0,
        )
    };
}

/// Monochrome pointers go to the miniport, which shows them as the browser's cursor. Color and
/// alpha pointers are declined, GDI draws them.
unsafe extern "system" fn drv_set_pointer_shape(
    pso: *mut SurfObj,
    mask: *mut SurfObj,
    color: *mut SurfObj,
    _xlate: *mut XlateObj,
    x_hot: i32,
    y_hot: i32,
    x: i32,
    y: i32,
    _bounds: *mut Rectl,
    flags: u32,
) -> u32 {
    const SPS_ALPHA: u32 = 0x10;
    // SAFETY: GDI passes the screen surface of the driver and the mask surface
    unsafe {
        let pdev = pdev((*pso).dhpdev);
        if !pdev.hardware_pointer || mask.is_null() || !color.is_null() || flags & SPS_ALPHA != 0 {
            hide_pointer(pdev);
            return SPS_DECLINE;
        }
        let mask = &*mask;
        let (width, height) = (
            mask.sizl_bitmap.cx as usize,
            mask.sizl_bitmap.cy as usize / 2,
        );
        if width > POINTER_SIZE || height > POINTER_SIZE {
            hide_pointer(pdev);
            return SPS_DECLINE;
        }

        // Transparent where the shape is smaller: AND 1, XOR 0
        let mut buffer = [0_u8; POINTER_BUFFER];
        let (and, xor) = buffer[POINTER_PIXELS..].split_at_mut(POINTER_STRIDE * POINTER_SIZE);
        and.fill(0xFF);
        let row_bytes = width.div_ceil(8);
        for row in 0..height {
            for (half, out) in [(0, &mut *and), (height, &mut *xor)] {
                let source = mask
                    .pv_scan0
                    .cast::<u8>()
                    .offset(((row + half) as isize) * mask.l_delta as isize);
                for byte in 0..row_bytes {
                    // The bits past the width keep the transparent default
                    let keep = if (byte + 1) * 8 > width {
                        0xFF_u8 >> (width - byte * 8)
                    } else {
                        0
                    };
                    let index = row * POINTER_STRIDE + byte;
                    out[index] = (out[index] & keep) | (source.add(byte).read() & !keep);
                }
            }
        }
        pdev.pointer_hot = Pointl { x: x_hot, y: y_hot };
        let hidden = x == -1;
        let header: [u32; 5] = [
            VIDEO_MODE_MONO_POINTER,
            POINTER_SIZE as u32,
            POINTER_SIZE as u32,
            POINTER_STRIDE as u32,
            1,
        ];
        for (i, word) in header.iter().enumerate() {
            buffer[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        let (column, row) = if hidden {
            (-1_i16, -1_i16)
        } else {
            ((x - x_hot) as i16, (y - y_hot) as i16)
        };
        buffer[20..22].copy_from_slice(&column.to_le_bytes());
        buffer[22..24].copy_from_slice(&row.to_le_bytes());
        if ioctl(
            pdev.driver,
            IOCTL_VIDEO_SET_POINTER_ATTR,
            buffer.as_ptr().cast(),
            buffer.len(),
            ptr::null_mut(),
            0,
        )
        .is_none()
        {
            hide_pointer(pdev);
            return SPS_DECLINE;
        }
        SPS_ACCEPT_NOEXCLUDE
    }
}

unsafe extern "system" fn drv_move_pointer(pso: *mut SurfObj, x: i32, y: i32, _bounds: *mut Rectl) {
    // SAFETY: GDI passes the screen surface of the driver
    unsafe {
        let pdev = pdev((*pso).dhpdev);
        if x == -1 {
            hide_pointer(pdev);
            return;
        }
        let position = VideoPointerPosition {
            column: (x - pdev.pointer_hot.x) as i16,
            row: (y - pdev.pointer_hot.y) as i16,
        };
        ioctl(
            pdev.driver,
            IOCTL_VIDEO_SET_POINTER_POSITION,
            (&raw const position).cast(),
            size_of::<VideoPointerPosition>(),
            ptr::null_mut(),
            0,
        );
    }
}

const _: () = assert!(VIDEO_MODE_COLOR_POINTER != VIDEO_MODE_MONO_POINTER);

macro_rules! function {
    ($index:expr, $function:expr) => {
        DrvFn {
            i_func: $index,
            pfn: $function as *const c_void,
        }
    };
}

struct Functions([DrvFn; 23]);
// SAFETY: The table only holds function pointers, which GDI reads
unsafe impl Sync for Functions {}

static FUNCTIONS: Functions = Functions([
    function!(INDEX_DRV_ENABLE_PDEV, drv_enable_pdev),
    function!(INDEX_DRV_COMPLETE_PDEV, drv_complete_pdev),
    function!(INDEX_DRV_DISABLE_PDEV, drv_disable_pdev),
    function!(INDEX_DRV_ENABLE_SURFACE, drv_enable_surface),
    function!(INDEX_DRV_DISABLE_SURFACE, drv_disable_surface),
    function!(INDEX_DRV_ASSERT_MODE, drv_assert_mode),
    function!(INDEX_DRV_DISABLE_DRIVER, drv_disable_driver),
    function!(INDEX_DRV_GET_MODES, drv_get_modes),
    function!(INDEX_DRV_SET_PALETTE, drv_set_palette),
    function!(INDEX_DRV_SET_POINTER_SHAPE, drv_set_pointer_shape),
    function!(INDEX_DRV_MOVE_POINTER, drv_move_pointer),
    function!(INDEX_DRV_REALIZE_BRUSH, gdi::drv_realize_brush),
    function!(INDEX_DRV_BIT_BLT, gdi::drv_bit_blt),
    function!(INDEX_DRV_COPY_BITS, gdi::drv_copy_bits),
    function!(INDEX_DRV_STRETCH_BLT, gdi::drv_stretch_blt),
    function!(INDEX_DRV_TEXT_OUT, gdi::drv_text_out),
    function!(INDEX_DRV_STROKE_PATH, gdi::drv_stroke_path),
    function!(INDEX_DRV_LINE_TO, gdi::drv_line_to),
    function!(INDEX_DRV_ALPHA_BLEND, gdi::drv_alpha_blend),
    function!(INDEX_DRV_TRANSPARENT_BLT, gdi::drv_transparent_blt),
    function!(
        INDEX_DRV_GET_DIRECT_DRAW_INFO,
        ddraw::drv_get_direct_draw_info
    ),
    function!(INDEX_DRV_ENABLE_DIRECT_DRAW, ddraw::drv_enable_direct_draw),
    function!(
        INDEX_DRV_DISABLE_DIRECT_DRAW,
        ddraw::drv_disable_direct_draw
    ),
]);

/// The entry point GDI calls after loading the driver
///
/// # Safety
///
/// Called by GDI with a DRVENABLEDATA of `size` bytes.
#[unsafe(no_mangle)]
unsafe extern "system" fn DrvEnableDriver(
    engine_version: u32,
    size: u32,
    data: *mut DrvEnableData,
) -> Bool {
    if engine_version < DDI_DRIVER_VERSION_NT5_01 || (size as usize) < size_of::<DrvEnableData>() {
        return 0;
    }
    // SAFETY: GDI passes a DRVENABLEDATA that fits
    unsafe {
        data.write(DrvEnableData {
            i_driver_version: DDI_DRIVER_VERSION_NT5_01,
            c: FUNCTIONS.0.len() as u32,
            pdrvfn: FUNCTIONS.0.as_ptr(),
        });
    }
    1
}
