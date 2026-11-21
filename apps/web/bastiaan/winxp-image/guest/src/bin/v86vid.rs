/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! v86vid.sys - video miniport for the Bochs VBE (dispi) adapter v86 emulates.
//!
//! Pairs with the stock framebuf.dll of Windows XP. Common sizes in 8 bit (palette), 16 and 32
//! bit colour, so DirectDraw games find their fullscreen modes, plus one extra 32 bit mode: the
//! size the page currently wants (asked over the backdoor), which is how the desktop follows the
//! browser window.
//!
//! Hardware pointer: framebuf.dll hands monochrome pointers of up to 32x32 to the miniport, which
//! passes them to the page; the page shows them as the browser's own cursor, so it moves at
//! native speed and Windows no longer draws it. Colour pointers and shapes the page declines stay
//! drawn by Windows.

#![no_std]
#![no_main]

use core::ffi::c_void;
use core::panic::PanicInfo;
use core::ptr;

use v86::ddk::*;
use v86::port;
use v86::protocol::*;

const DISPI_INDEX: u16 = 0x1CE;
const DISPI_DATA: u16 = 0x1CF;
const DISPI_ID: u16 = 0;
const DISPI_XRES: u16 = 1;
const DISPI_YRES: u16 = 2;
const DISPI_BPP: u16 = 3;
const DISPI_ENABLE: u16 = 4;
const DISPI_VIDEO_MEMORY_64K: u16 = 0xA;
const DISPI_ENABLED: u16 = 0x01;
const DISPI_8BIT_DAC: u16 = 0x20;
const DISPI_LFB: u16 = 0x40;
const VGA_DAC_WRITE: u16 = 0x3C8;
const VGA_DAC_DATA: u16 = 0x3C9;

const MAX_MODES: usize = 64;
const POINTER_SIZE: u32 = 32;
/// The AND then the XOR mask, 1 bit per pixel
const POINTER_MASKS: usize = 2 * (POINTER_SIZE as usize / 8) * POINTER_SIZE as usize;

const STANDARD_SIZES: [(u32, u32); 16] = [
    (640, 480),
    (800, 600),
    (1024, 768),
    (1152, 864),
    (1280, 720),
    (1280, 800),
    (1280, 1024),
    (1366, 768),
    (1440, 900),
    (1600, 900),
    (1600, 1200),
    (1680, 1050),
    (1920, 1080),
    (1920, 1200),
    (2560, 1440),
    (2560, 1600),
];
const DEPTHS: [u32; 3] = [32, 16, 8];

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Mode {
    width: u32,
    height: u32,
    bpp: u32,
}

impl Mode {
    const fn bytes(&self) -> u32 {
        self.width * self.height * (self.bpp / 8)
    }
}

/// The pointer shape the page reads from the device extension, which is nonpaged
#[repr(C, align(4))]
struct Pointer([u8; VIDEO_POINTER_ATTRIBUTES_SIZE + POINTER_MASKS]);

/// The device extension, the video port zeroes it
#[repr(C)]
struct Device {
    lfb: i64,
    vram: u32,
    modes: [Mode; MAX_MODES],
    mode_count: usize,
    current: Mode,
    /// The page answers the v86 backdoor commands
    host: bool,
    /// The page accepted the last shape, kept for when it asks again
    pointer_shown: bool,
    pointer: Pointer,
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    bug_check()
}

/// # Safety
///
/// Kernel mode, the adapter is the Bochs VBE.
unsafe fn dispi_write(index: u16, value: u16) {
    // SAFETY: The ports belong to the adapter
    unsafe {
        port::write_u16(DISPI_INDEX, index);
        port::write_u16(DISPI_DATA, value);
    }
}

/// # Safety
///
/// Kernel mode, the adapter is the Bochs VBE.
unsafe fn dispi_read(index: u16) -> u16 {
    // SAFETY: The ports belong to the adapter
    unsafe {
        port::write_u16(DISPI_INDEX, index);
        port::read_u16(DISPI_DATA)
    }
}

const fn pointer_position(column: i16, row: i16) -> u32 {
    ((column as u16 as u32) << 16) | row as u16 as u32
}

impl Device {
    fn modes(&self) -> &[Mode] {
        &self.modes[..self.mode_count]
    }

    /// Tells the page the shape, which reads it from the device extension
    ///
    /// # Safety
    ///
    /// Kernel mode, the device extension is nonpaged.
    unsafe fn pointer_shape(&mut self) -> bool {
        let shape = self.pointer.0.as_ptr() as u32;
        // SAFETY: The caller runs in kernel mode, the shape is nonpaged
        self.pointer_shown = unsafe { port::call(V86_CMD_POINTER_SHAPE, shape) } == 1;
        self.pointer_shown
    }

    /// # Safety
    ///
    /// Like [`Self::pointer_shape`].
    unsafe fn pointer_move(&mut self, position: u32) {
        // SAFETY: The caller runs in kernel mode
        unsafe {
            if port::call(V86_CMD_POINTER_MOVE, position) == V86_POINTER_RESEND
                && self.pointer_shown
            {
                self.pointer_shape();
            }
        }
    }

    fn add_mode(&mut self, mode: Mode) {
        let fits = (320..=2560).contains(&mode.width)
            && (240..=1600).contains(&mode.height)
            && mode.bytes() <= self.vram;
        if fits && self.mode_count < MAX_MODES && !self.modes().contains(&mode) {
            self.modes[self.mode_count] = mode;
            self.mode_count += 1;
        }
    }

    /// Rebuilt on every mode query, so the current size of the host is always offered
    ///
    /// # Safety
    ///
    /// Kernel mode.
    unsafe fn build_modes(&mut self) {
        self.mode_count = 0;
        for bpp in DEPTHS {
            for (width, height) in STANDARD_SIZES {
                self.add_mode(Mode { width, height, bpp });
            }
        }
        if self.host {
            // SAFETY: The caller runs in kernel mode
            let size = unsafe { port::call(V86_CMD_GETSIZE, 0) };
            if size != 0 {
                self.add_mode(Mode {
                    width: size >> 16,
                    height: size & 0xFFFF,
                    bpp: 32,
                });
            }
        }
        if self.current.width != 0 {
            self.add_mode(self.current);
        }
    }
}

fn mode_information(index: usize, mode: &Mode) -> VideoModeInformation {
    let mut info = VideoModeInformation {
        length: size_of::<VideoModeInformation>() as u32,
        mode_index: index as u32,
        vis_screen_width: mode.width,
        vis_screen_height: mode.height,
        screen_stride: mode.width * (mode.bpp / 8),
        number_of_planes: 1,
        bits_per_plane: mode.bpp,
        frequency: 60,
        x_millimeter: 320,
        y_millimeter: 240,
        attribute_flags: VIDEO_MODE_COLOR | VIDEO_MODE_GRAPHICS,
        video_memory_bitmap_width: mode.width,
        video_memory_bitmap_height: mode.height,
        ..Default::default()
    };
    match mode.bpp {
        8 => {
            // The VGA DAC holds the palette, switched to 8 bits per channel
            (
                info.number_red_bits,
                info.number_green_bits,
                info.number_blue_bits,
            ) = (8, 8, 8);
            info.attribute_flags |= VIDEO_MODE_PALETTE_DRIVEN | VIDEO_MODE_MANAGED_PALETTE;
        }
        16 => {
            (
                info.number_red_bits,
                info.number_green_bits,
                info.number_blue_bits,
            ) = (5, 6, 5);
            (info.red_mask, info.green_mask, info.blue_mask) = (0xF800, 0x07E0, 0x001F);
        }
        _ => {
            (
                info.number_red_bits,
                info.number_green_bits,
                info.number_blue_bits,
            ) = (8, 8, 8);
            (info.red_mask, info.green_mask, info.blue_mask) = (0xFF0000, 0x00FF00, 0x0000FF);
        }
    }
    info
}

/// # Safety
///
/// Kernel mode, the adapter is the Bochs VBE.
unsafe fn set_mode(mode: &Mode) {
    let dac = if mode.bpp == 8 { DISPI_8BIT_DAC } else { 0 };
    // SAFETY: The caller runs in kernel mode with the adapter
    unsafe {
        dispi_write(DISPI_ENABLE, 0);
        dispi_write(DISPI_BPP, mode.bpp as u16);
        dispi_write(DISPI_XRES, mode.width as u16);
        dispi_write(DISPI_YRES, mode.height as u16);
        dispi_write(DISPI_ENABLE, DISPI_ENABLED | DISPI_LFB | dac);
    }
}

unsafe extern "system" fn find_adapter(
    extension: *mut c_void,
    _context: *mut c_void,
    _arguments: *mut u16,
    config: *mut VideoPortConfigInfo,
    again: *mut u8,
) -> VpStatus {
    // SAFETY: The video port passes the zeroed device extension, the configuration and the
    // again flag
    unsafe {
        let device = &mut *extension.cast::<Device>();
        *again = 0;
        if !(0xB0C0..=0xB0C5).contains(&dispi_read(DISPI_ID)) {
            return ERROR_DEV_NOT_EXIST;
        }

        let mut ranges = [VideoAccessRange::default(); 4];
        let (mut vendor, mut device_id, mut slot) = (0x1234_u32, 0x1111_u32, 0_u32);
        device.lfb = 0xE000_0000;
        if VideoPortGetAccessRanges(
            extension,
            0,
            ptr::null_mut(),
            ranges.len() as u32,
            ranges.as_mut_ptr(),
            (&raw mut vendor).cast(),
            (&raw mut device_id).cast(),
            &mut slot,
        ) == NO_ERROR
            && let Some(range) = ranges
                .iter()
                .find(|range| range.range_length != 0 && range.range_in_io_space == 0)
        {
            device.lfb = range.range_start;
        }
        device.vram = (u32::from(dispi_read(DISPI_VIDEO_MEMORY_64K)) * 0x10000).max(0x400000);
        device.host = port::host_present() && port::call(V86_CMD_HELLO, 0) == V86_HELLO_REPLY;

        let config = &mut *config;
        config.num_emulator_access_entries = 0;
        config.emulator_access_entries = ptr::null_mut();
        config.vdm_physical_video_memory_address = 0;
        config.vdm_physical_video_memory_length = 0;
        config.hardware_state_size = 0;
        NO_ERROR
    }
}

unsafe extern "system" fn initialize(extension: *mut c_void) -> u8 {
    // SAFETY: The video port passes the device extension
    unsafe { (*extension.cast::<Device>()).build_modes() };
    1
}

/// Handles a request, returns its status and the length of its output
///
/// # Safety
///
/// Kernel mode, the buffers of the packet have their given lengths.
unsafe fn request(
    device: &mut Device,
    extension: *mut c_void,
    packet: &VideoRequestPacket,
) -> (VpStatus, usize) {
    let input = packet.input_buffer;
    let output = packet.output_buffer;
    let in_len = packet.input_buffer_length as usize;
    let out_len = packet.output_buffer_length as usize;
    // SAFETY: Every buffer access is checked against its length first
    unsafe {
        match packet.io_control_code {
            IOCTL_VIDEO_QUERY_NUM_AVAIL_MODES => {
                if out_len < size_of::<VideoNumModes>() {
                    return (ERROR_INSUFFICIENT_BUFFER, 0);
                }
                device.build_modes();
                output.cast::<VideoNumModes>().write(VideoNumModes {
                    num_modes: device.mode_count as u32,
                    mode_information_length: size_of::<VideoModeInformation>() as u32,
                });
                (NO_ERROR, size_of::<VideoNumModes>())
            }
            IOCTL_VIDEO_QUERY_AVAIL_MODES => {
                let need = device.mode_count * size_of::<VideoModeInformation>();
                if out_len < need {
                    return (ERROR_INSUFFICIENT_BUFFER, 0);
                }
                let info = output.cast::<VideoModeInformation>();
                for (i, mode) in device.modes().iter().enumerate() {
                    info.add(i).write(mode_information(i, mode));
                }
                (NO_ERROR, need)
            }
            IOCTL_VIDEO_SET_CURRENT_MODE => {
                if in_len < size_of::<u32>() {
                    return (ERROR_INSUFFICIENT_BUFFER, 0);
                }
                let index = (input.cast::<u32>().read() & 0x0FFF_FFFF) as usize;
                let Some(&mode) = device.modes().get(index) else {
                    return (ERROR_INVALID_PARAMETER, 0);
                };
                device.current = mode;
                set_mode(&mode);
                (NO_ERROR, 0)
            }
            IOCTL_VIDEO_QUERY_CURRENT_MODE => {
                if out_len < size_of::<VideoModeInformation>() {
                    return (ERROR_INSUFFICIENT_BUFFER, 0);
                }
                let index = device
                    .modes()
                    .iter()
                    .rposition(|mode| *mode == device.current)
                    .unwrap_or(0);
                output
                    .cast::<VideoModeInformation>()
                    .write(mode_information(index, &device.current));
                (NO_ERROR, size_of::<VideoModeInformation>())
            }
            IOCTL_VIDEO_MAP_VIDEO_MEMORY => {
                if in_len < size_of::<*mut c_void>()
                    || out_len < size_of::<VideoMemoryInformation>()
                {
                    return (ERROR_INSUFFICIENT_BUFFER, 0);
                }
                let mut base = input.cast::<*mut c_void>().read();
                let (mut length, mut in_io_space) = (device.vram, VIDEO_MEMORY_SPACE_MEMORY);
                let status = VideoPortMapMemory(
                    extension,
                    device.lfb,
                    &mut length,
                    &mut in_io_space,
                    &mut base,
                );
                if status != NO_ERROR {
                    return (status, 0);
                }
                output
                    .cast::<VideoMemoryInformation>()
                    .write(VideoMemoryInformation {
                        video_ram_base: base,
                        video_ram_length: length,
                        frame_buffer_base: base,
                        frame_buffer_length: device.current.bytes(),
                    });
                (NO_ERROR, size_of::<VideoMemoryInformation>())
            }
            IOCTL_VIDEO_UNMAP_VIDEO_MEMORY => {
                if in_len < size_of::<*mut c_void>() {
                    return (ERROR_INSUFFICIENT_BUFFER, 0);
                }
                let base = input.cast::<*mut c_void>().read();
                (VideoPortUnmapMemory(extension, base, ptr::null_mut()), 0)
            }
            IOCTL_VIDEO_SET_COLOR_REGISTERS => {
                let header = size_of::<VideoClut>();
                if in_len < header {
                    return (ERROR_INVALID_PARAMETER, 0);
                }
                let clut = &*input.cast::<VideoClut>();
                let (first, count) = (usize::from(clut.first_entry), usize::from(clut.num_entries));
                if in_len < header + count * 4 || first + count > 256 {
                    return (ERROR_INVALID_PARAMETER, 0);
                }
                let colors = core::slice::from_raw_parts(input.cast::<u8>().add(header), count * 4);
                for (i, color) in colors.as_chunks::<4>().0.iter().enumerate() {
                    port::write_u8(VGA_DAC_WRITE, (first + i) as u8);
                    for &channel in &color[..3] {
                        port::write_u8(VGA_DAC_DATA, channel);
                    }
                }
                (NO_ERROR, 0)
            }
            IOCTL_VIDEO_QUERY_POINTER_CAPABILITIES if device.host => {
                if out_len < size_of::<VideoPointerCapabilities>() {
                    return (ERROR_INSUFFICIENT_BUFFER, 0);
                }
                output
                    .cast::<VideoPointerCapabilities>()
                    .write(VideoPointerCapabilities {
                        flags: VIDEO_MODE_ASYNC_POINTER | VIDEO_MODE_MONO_POINTER,
                        max_width: POINTER_SIZE,
                        max_height: POINTER_SIZE,
                        hw_ptr_bitmap_start: u32::MAX,
                        hw_ptr_bitmap_end: u32::MAX,
                    });
                (NO_ERROR, size_of::<VideoPointerCapabilities>())
            }
            IOCTL_VIDEO_SET_POINTER_ATTR => {
                if !device.host || in_len < VIDEO_POINTER_ATTRIBUTES_SIZE {
                    return (ERROR_INVALID_PARAMETER, 0);
                }
                let attributes = input.cast::<VideoPointerAttributes>().read();
                let size = VIDEO_POINTER_ATTRIBUTES_SIZE
                    + 2 * attributes.width_in_bytes as usize * attributes.height as usize;
                if attributes.width > POINTER_SIZE
                    || attributes.height > POINTER_SIZE
                    || attributes.width_in_bytes != POINTER_SIZE / 8
                    || in_len < size
                    || attributes.flags & VIDEO_MODE_COLOR_POINTER != 0
                {
                    return (ERROR_INVALID_PARAMETER, 0);
                }
                device.pointer.0.fill(0);
                ptr::copy_nonoverlapping(input.cast::<u8>(), device.pointer.0.as_mut_ptr(), size);
                if !device.pointer_shape() {
                    return (ERROR_INVALID_PARAMETER, 0);
                }
                let visible =
                    attributes.enable != 0 && (attributes.column != -1 || attributes.row != -1);
                device.pointer_move(if visible {
                    pointer_position(attributes.column, attributes.row)
                } else {
                    V86_POINTER_HIDDEN
                });
                (NO_ERROR, 0)
            }
            IOCTL_VIDEO_SET_POINTER_POSITION => {
                if in_len < size_of::<VideoPointerPosition>() {
                    return (ERROR_INSUFFICIENT_BUFFER, 0);
                }
                let position = &*input.cast::<VideoPointerPosition>();
                device.pointer_move(pointer_position(position.column, position.row));
                (NO_ERROR, 0)
            }
            IOCTL_VIDEO_DISABLE_POINTER => {
                device.pointer_move(V86_POINTER_HIDDEN);
                (NO_ERROR, 0)
            }
            // Shown again by the next position
            IOCTL_VIDEO_ENABLE_POINTER => (NO_ERROR, 0),
            IOCTL_VIDEO_RESET_DEVICE => {
                dispi_write(DISPI_ENABLE, 0);
                (NO_ERROR, 0)
            }
            _ => (ERROR_INVALID_FUNCTION, 0),
        }
    }
}

unsafe extern "system" fn start_io(extension: *mut c_void, packet: *mut VideoRequestPacket) -> u8 {
    // SAFETY: The video port passes the device extension and a request with its status block
    unsafe {
        let packet = &*packet;
        let (status, information) = request(&mut *extension.cast::<Device>(), extension, packet);
        let block = &mut *packet.status_block;
        block.status = status;
        block.information = information;
    }
    1
}

unsafe extern "system" fn reset_hw(_extension: *mut c_void, _columns: u32, _rows: u32) -> u8 {
    // SAFETY: The video port resets the adapter of the driver
    unsafe { dispi_write(DISPI_ENABLE, 0) };
    // Lets the HAL finish with an int 10h text mode set
    0
}

unsafe extern "system" fn power_state(
    _extension: *mut c_void,
    _id: u32,
    _power: *mut c_void,
) -> VpStatus {
    NO_ERROR
}

/// # Safety
///
/// Called once by the kernel with its two arguments for the video port.
#[unsafe(no_mangle)]
unsafe extern "system" fn DriverEntry(argument1: *mut c_void, argument2: *mut c_void) -> u32 {
    let mut init = VideoHwInitializationData {
        hw_init_data_size: size_of::<VideoHwInitializationData>() as u32,
        adapter_interface_type: PCI_BUS,
        hw_find_adapter: Some(find_adapter),
        hw_initialize: Some(initialize),
        hw_interrupt: ptr::null_mut(),
        hw_start_io: Some(start_io),
        hw_device_extension_size: size_of::<Device>() as u32,
        starting_device_number: 0,
        hw_reset_hw: Some(reset_hw),
        hw_timer: ptr::null_mut(),
        hw_start_dma: ptr::null_mut(),
        hw_set_power_state: Some(power_state),
        hw_get_power_state: Some(power_state),
        reserved: [0; 8],
    };
    // SAFETY: The arguments come from the kernel, the video port copies the init data
    unsafe { VideoPortInitialize(argument1, argument2, &mut init, ptr::null_mut()) }
}
