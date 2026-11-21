/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The parts of the Windows XP DDK the drivers use, for 32-bit x86. The structures only name the
//! fields the drivers touch, their offsets are checked against the mingw-w64 headers. Imports are
//! raw-dylib, so the drivers don't need import libraries.

#![allow(non_snake_case, missing_docs)]

use core::cell::UnsafeCell;
use core::ffi::c_void;
use core::mem::{offset_of, size_of};
use core::ptr;

pub type NtStatus = i32;
pub const STATUS_SUCCESS: NtStatus = 0;
pub const STATUS_CONTINUE_COMPLETION: NtStatus = 0;
pub const STATUS_BUFFER_TOO_SMALL: NtStatus = 0xC0000023_u32 as i32;
pub const STATUS_INVALID_PARAMETER: NtStatus = 0xC000000D_u32 as i32;
pub const STATUS_INVALID_DEVICE_REQUEST: NtStatus = 0xC0000010_u32 as i32;
pub const STATUS_INSUFFICIENT_RESOURCES: NtStatus = 0xC000009A_u32 as i32;
pub const STATUS_DEVICE_NOT_CONNECTED: NtStatus = 0xC000009D_u32 as i32;

pub const fn nt_success(status: NtStatus) -> bool {
    status >= 0
}

pub const IRP_MJ_CREATE: u8 = 0;
pub const IRP_MJ_CLOSE: u8 = 2;
pub const IRP_MJ_READ: u8 = 3;
pub const IRP_MJ_DEVICE_CONTROL: u8 = 14;
pub const IRP_MJ_INTERNAL_DEVICE_CONTROL: u8 = 15;
pub const IRP_MJ_CLEANUP: u8 = 18;
pub const IRP_MJ_POWER: u8 = 22;
pub const IRP_MJ_PNP: u8 = 27;
pub const IRP_MJ_COUNT: usize = 28;
pub const IRP_MN_REMOVE_DEVICE: u8 = 2;

pub const DO_BUFFERED_IO: u32 = 0x4;
pub const DO_DIRECT_IO: u32 = 0x10;
pub const DO_DEVICE_INITIALIZING: u32 = 0x80;
pub const DO_POWER_PAGABLE: u32 = 0x2000;
pub const FILE_DEVICE_MOUSE: u32 = 0xF;
pub const FILE_DEVICE_UNKNOWN: u32 = 0x22;

const SL_PENDING_RETURNED: u8 = 0x1;
const SL_INVOKE_ON_CANCEL: u8 = 0x20;
const SL_INVOKE_ON_SUCCESS: u8 = 0x40;
const SL_INVOKE_ON_ERROR: u8 = 0x80;
const SYNCHRONIZATION_EVENT: i32 = 1;

pub type DriverDispatch = unsafe extern "system" fn(*mut DeviceObject, *mut Irp) -> NtStatus;
pub type DriverUnload = unsafe extern "system" fn(*mut DriverObject);
pub type DriverAddDevice =
    unsafe extern "system" fn(*mut DriverObject, *mut DeviceObject) -> NtStatus;
pub type IoCompletionRoutine =
    unsafe extern "system" fn(*mut DeviceObject, *mut Irp, *mut c_void) -> NtStatus;

#[repr(C)]
pub struct UnicodeString {
    pub length: u16,
    pub maximum_length: u16,
    pub buffer: *const u16,
}

impl UnicodeString {
    pub const fn new(text: &'static [u16]) -> Self {
        let bytes = (text.len() * 2) as u16;
        Self {
            length: bytes,
            maximum_length: bytes,
            buffer: text.as_ptr(),
        }
    }
}

/// UTF-16 of an ASCII string at compile time, for [`UnicodeString`]
#[macro_export]
macro_rules! wide {
    ($text:expr) => {{
        const TEXT: &str = $text;
        const WIDE: [u16; TEXT.len()] = $crate::ddk::ascii_to_utf16(TEXT);
        &WIDE
    }};
}

pub const fn ascii_to_utf16<const N: usize>(text: &str) -> [u16; N] {
    let bytes = text.as_bytes();
    let mut wide = [0; N];
    let mut i = 0;
    while i < N {
        assert!(bytes[i].is_ascii());
        wide[i] = bytes[i] as u16;
        i += 1;
    }
    wide
}

#[repr(C)]
pub struct IoStatusBlock {
    pub status: NtStatus,
    pub information: usize,
}

#[repr(C)]
pub struct Irp {
    _header: [u8; 12],
    pub system_buffer: *mut c_void,
    _thread_list_entry: [u8; 8],
    pub io_status: IoStatusBlock,
    _requestor_mode: u8,
    pub pending_returned: u8,
    _stack_count: i8,
    current_location: i8,
    _middle: [u8; 60],
    current_stack_location: *mut IoStackLocation,
    _tail: [u8; 12],
}
const _: () = assert!(size_of::<Irp>() == 112);
const _: () = assert!(offset_of!(Irp, system_buffer) == 12);
const _: () = assert!(offset_of!(Irp, io_status) == 24);
const _: () = assert!(offset_of!(Irp, pending_returned) == 33);
const _: () = assert!(offset_of!(Irp, current_location) == 35);
const _: () = assert!(offset_of!(Irp, current_stack_location) == 96);

#[repr(C)]
pub struct IoStackLocation {
    pub major_function: u8,
    pub minor_function: u8,
    _flags: u8,
    control: u8,
    parameters: [u32; 4],
    _device_object: *mut DeviceObject,
    _file_object: *mut c_void,
    completion_routine: Option<IoCompletionRoutine>,
    context: *mut c_void,
}
const _: () = assert!(size_of::<IoStackLocation>() == 36);
const _: () = assert!(offset_of!(IoStackLocation, control) == 3);
const _: () = assert!(offset_of!(IoStackLocation, parameters) == 4);
const _: () = assert!(offset_of!(IoStackLocation, completion_routine) == 28);
const _: () = assert!(offset_of!(IoStackLocation, context) == 32);

impl IoStackLocation {
    /// `Parameters.DeviceIoControl.OutputBufferLength`, and `Parameters.Read.Length`
    pub const fn output_buffer_length(&self) -> u32 {
        self.parameters[0]
    }
    pub const fn read_length(&self) -> u32 {
        self.parameters[0]
    }
    pub const fn input_buffer_length(&self) -> u32 {
        self.parameters[1]
    }
    pub const fn io_control_code(&self) -> u32 {
        self.parameters[2]
    }
}

#[repr(C)]
pub struct DriverObject {
    _header: [u8; 4],
    pub device_object: *mut DeviceObject,
    _middle: [u8; 16],
    pub driver_extension: *mut DriverExtension,
    _middle2: [u8; 24],
    pub driver_unload: Option<DriverUnload>,
    pub major_function: [Option<DriverDispatch>; IRP_MJ_COUNT],
}
const _: () = assert!(size_of::<DriverObject>() == 168);
const _: () = assert!(offset_of!(DriverObject, device_object) == 4);
const _: () = assert!(offset_of!(DriverObject, driver_extension) == 24);
const _: () = assert!(offset_of!(DriverObject, driver_unload) == 52);
const _: () = assert!(offset_of!(DriverObject, major_function) == 56);

#[repr(C)]
pub struct DriverExtension {
    _driver_object: *mut DriverObject,
    pub add_device: Option<DriverAddDevice>,
}

/// Only the head, device objects are made by the kernel
#[repr(C)]
pub struct DeviceObject {
    _header: [u8; 28],
    pub flags: u32,
    _middle: [u8; 8],
    pub device_extension: *mut c_void,
}
const _: () = assert!(offset_of!(DeviceObject, flags) == 28);
const _: () = assert!(offset_of!(DeviceObject, device_extension) == 40);

#[repr(C)]
struct FastMutex {
    count: i32,
    owner: *mut c_void,
    contention: u32,
    event: [u32; 4],
    old_irql: u32,
}
const _: () = assert!(size_of::<FastMutex>() == 32);
const _: () = assert!(offset_of!(FastMutex, event) == 12);

/// A `FAST_MUTEX` for a static, which is in nonpaged memory
pub struct KernelMutex(UnsafeCell<FastMutex>);

// SAFETY: The kernel serializes the users of a fast mutex
unsafe impl Sync for KernelMutex {}

impl KernelMutex {
    pub const fn new() -> Self {
        Self(UnsafeCell::new(FastMutex {
            count: 0,
            owner: ptr::null_mut(),
            contention: 0,
            event: [0; 4],
            old_irql: 0,
        }))
    }

    /// `ExInitializeFastMutex`
    ///
    /// # Safety
    ///
    /// Once, before the mutex is used.
    pub unsafe fn init(&self) {
        let mutex = self.0.get();
        // SAFETY: Nobody uses the mutex yet
        unsafe {
            (*mutex).count = 1;
            (*mutex).owner = ptr::null_mut();
            (*mutex).contention = 0;
            KeInitializeEvent((&raw mut (*mutex).event).cast(), SYNCHRONIZATION_EVENT, 0);
        }
    }

    /// # Safety
    ///
    /// The mutex is initialized and the IRQL is at most APC_LEVEL.
    pub unsafe fn lock(&self) -> KernelMutexGuard<'_> {
        // SAFETY: The caller initialized the mutex
        unsafe { ExAcquireFastMutex(self.0.get()) };
        KernelMutexGuard(self)
    }
}

impl Default for KernelMutex {
    fn default() -> Self {
        Self::new()
    }
}

pub struct KernelMutexGuard<'a>(&'a KernelMutex);

impl Drop for KernelMutexGuard<'_> {
    fn drop(&mut self) {
        // SAFETY: The guard holds the mutex
        unsafe { ExReleaseFastMutex(self.0.0.get()) };
    }
}

/// Stops the machine with MANUALLY_INITIATED_CRASH, for panics, which drivers can't unwind
pub fn bug_check() -> ! {
    // SAFETY: KeBugCheck doesn't return
    unsafe { KeBugCheck(0xE2) }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MouseInputData {
    pub unit_id: u16,
    pub flags: u16,
    pub button_flags: u16,
    pub button_data: u16,
    pub raw_buttons: u32,
    pub last_x: i32,
    pub last_y: i32,
    pub extra_information: u32,
}
const _: () = assert!(size_of::<MouseInputData>() == 24);
const _: () = assert!(offset_of!(MouseInputData, last_x) == 12);

#[repr(C)]
pub struct MouseAttributes {
    pub mouse_identifier: u16,
    pub number_of_buttons: u16,
    pub sample_rate: u16,
    pub input_data_queue_length: u32,
}
const _: () = assert!(size_of::<MouseAttributes>() == 12);

pub const IOCTL_MOUSE_QUERY_ATTRIBUTES: u32 = 0xF0000;
pub const MOUSE_MOVE_RELATIVE: u16 = 0;
pub const MOUSE_MOVE_ABSOLUTE: u16 = 1;
pub const MOUSE_LEFT_BUTTON_DOWN: u16 = 0x1;
pub const MOUSE_LEFT_BUTTON_UP: u16 = 0x2;
pub const MOUSE_RIGHT_BUTTON_DOWN: u16 = 0x4;
pub const MOUSE_RIGHT_BUTTON_UP: u16 = 0x8;
pub const MOUSE_MIDDLE_BUTTON_DOWN: u16 = 0x10;
pub const MOUSE_MIDDLE_BUTTON_UP: u16 = 0x20;
pub const MOUSE_WHEEL: u16 = 0x400;
pub const WHEELMOUSE_I8042_HARDWARE: u16 = 0x20;

#[link(
    name = "ntoskrnl.exe",
    kind = "raw-dylib",
    modifiers = "+verbatim",
    import_name_type = "undecorated"
)]
unsafe extern "system" {
    pub fn IoCreateDevice(
        driver: *mut DriverObject,
        extension_size: u32,
        name: *const UnicodeString,
        device_type: u32,
        characteristics: u32,
        exclusive: u8,
        device: *mut *mut DeviceObject,
    ) -> NtStatus;
    pub fn IoDeleteDevice(device: *mut DeviceObject);
    pub fn IoCreateSymbolicLink(link: *const UnicodeString, name: *const UnicodeString)
    -> NtStatus;
    pub fn IoDeleteSymbolicLink(link: *const UnicodeString) -> NtStatus;
    pub fn IoAttachDeviceToDeviceStack(
        source: *mut DeviceObject,
        target: *mut DeviceObject,
    ) -> *mut DeviceObject;
    pub fn IoDetachDevice(target: *mut DeviceObject);
    pub fn MmAllocateContiguousMemory(size: usize, highest: i64) -> *mut c_void;
    pub fn MmFreeContiguousMemory(base: *mut c_void);
    pub fn MmGetPhysicalAddress(base: *mut c_void) -> i64;
    pub fn PoStartNextPowerIrp(irp: *mut Irp);
    pub fn PoCallDriver(device: *mut DeviceObject, irp: *mut Irp) -> NtStatus;
    fn KeInitializeEvent(event: *mut c_void, kind: i32, state: u8);
    fn KeBugCheck(code: u32) -> !;
}

#[link(
    name = "ntoskrnl.exe",
    kind = "raw-dylib",
    modifiers = "+verbatim",
    import_name_type = "undecorated"
)]
unsafe extern "fastcall" {
    fn IofCallDriver(device: *mut DeviceObject, irp: *mut Irp) -> NtStatus;
    fn IofCompleteRequest(irp: *mut Irp, priority_boost: i8);
}

#[link(
    name = "hal.dll",
    kind = "raw-dylib",
    modifiers = "+verbatim",
    import_name_type = "undecorated"
)]
unsafe extern "fastcall" {
    fn ExAcquireFastMutex(mutex: *mut FastMutex);
    fn ExReleaseFastMutex(mutex: *mut FastMutex);
}

// The inline functions of wdm.h. Safety: the IRP must be valid and owned by the driver at its
// current stack location.

/// # Safety
///
/// See above.
pub unsafe fn IoGetCurrentIrpStackLocation(irp: *mut Irp) -> *mut IoStackLocation {
    // SAFETY: The caller gives a valid IRP
    unsafe { (*irp).current_stack_location }
}

/// # Safety
///
/// See above, the IRP must have a next stack location.
unsafe fn IoGetNextIrpStackLocation(irp: *mut Irp) -> *mut IoStackLocation {
    // SAFETY: The caller gives a valid IRP with a next location below the current one
    unsafe { (*irp).current_stack_location.sub(1) }
}

/// # Safety
///
/// See above.
pub unsafe fn IoSkipCurrentIrpStackLocation(irp: *mut Irp) {
    // SAFETY: The caller gives a valid IRP, the lower driver gets the current location
    unsafe {
        (*irp).current_location += 1;
        (*irp).current_stack_location = (*irp).current_stack_location.add(1);
    }
}

/// # Safety
///
/// See above, the IRP must have a next stack location.
pub unsafe fn IoCopyCurrentIrpStackLocationToNext(irp: *mut Irp) {
    // SAFETY: The caller gives a valid IRP with a next location, which gets everything up to
    // the completion routine
    unsafe {
        let next = IoGetNextIrpStackLocation(irp);
        ptr::copy_nonoverlapping(
            IoGetCurrentIrpStackLocation(irp).cast::<u8>(),
            next.cast::<u8>(),
            offset_of!(IoStackLocation, completion_routine),
        );
        (*next).control = 0;
    }
}

/// `IoSetCompletionRoutine`, on success, error and cancel
///
/// # Safety
///
/// See above, the IRP must have a next stack location.
pub unsafe fn IoSetCompletionRoutine(irp: *mut Irp, routine: IoCompletionRoutine) {
    // SAFETY: The caller gives a valid IRP with a next location
    unsafe {
        let next = IoGetNextIrpStackLocation(irp);
        (*next).completion_routine = Some(routine);
        (*next).context = ptr::null_mut();
        (*next).control = SL_INVOKE_ON_SUCCESS | SL_INVOKE_ON_ERROR | SL_INVOKE_ON_CANCEL;
    }
}

/// # Safety
///
/// See above.
pub unsafe fn IoMarkIrpPending(irp: *mut Irp) {
    // SAFETY: The caller gives a valid IRP
    unsafe { (*IoGetCurrentIrpStackLocation(irp)).control |= SL_PENDING_RETURNED };
}

/// # Safety
///
/// See above, the IRP belongs to the lower driver after the call.
pub unsafe fn IoCallDriver(device: *mut DeviceObject, irp: *mut Irp) -> NtStatus {
    // SAFETY: The caller passes a valid device and IRP
    unsafe { IofCallDriver(device, irp) }
}

/// Completes the IRP with the status, without a priority boost
///
/// # Safety
///
/// See above, the IRP is gone after the call.
pub unsafe fn complete_request(irp: *mut Irp, status: NtStatus) -> NtStatus {
    // SAFETY: The caller gives a valid IRP it owns
    unsafe {
        (*irp).io_status.status = status;
        IofCompleteRequest(irp, 0);
    }
    status
}

// Video miniports

pub type VpStatus = u32;
pub const NO_ERROR: VpStatus = 0;
pub const ERROR_INVALID_FUNCTION: VpStatus = 1;
pub const ERROR_DEV_NOT_EXIST: VpStatus = 55;
pub const ERROR_INVALID_PARAMETER: VpStatus = 87;
pub const ERROR_INSUFFICIENT_BUFFER: VpStatus = 122;

pub type HwFindAdapter = unsafe extern "system" fn(
    *mut c_void,
    *mut c_void,
    *mut u16,
    *mut VideoPortConfigInfo,
    *mut u8,
) -> VpStatus;
pub type HwInitialize = unsafe extern "system" fn(*mut c_void) -> u8;
pub type HwStartIo = unsafe extern "system" fn(*mut c_void, *mut VideoRequestPacket) -> u8;
pub type HwResetHw = unsafe extern "system" fn(*mut c_void, u32, u32) -> u8;
pub type HwPowerState = unsafe extern "system" fn(*mut c_void, u32, *mut c_void) -> VpStatus;

pub const PCI_BUS: i32 = 5;

#[repr(C)]
pub struct VideoHwInitializationData {
    pub hw_init_data_size: u32,
    pub adapter_interface_type: i32,
    pub hw_find_adapter: Option<HwFindAdapter>,
    pub hw_initialize: Option<HwInitialize>,
    pub hw_interrupt: *mut c_void,
    pub hw_start_io: Option<HwStartIo>,
    pub hw_device_extension_size: u32,
    pub starting_device_number: u32,
    pub hw_reset_hw: Option<HwResetHw>,
    pub hw_timer: *mut c_void,
    pub hw_start_dma: *mut c_void,
    pub hw_set_power_state: Option<HwPowerState>,
    pub hw_get_power_state: Option<HwPowerState>,
    pub reserved: [u32; 8],
}
const _: () = assert!(size_of::<VideoHwInitializationData>() == 84);
const _: () = assert!(offset_of!(VideoHwInitializationData, hw_start_io) == 20);
const _: () = assert!(offset_of!(VideoHwInitializationData, hw_reset_hw) == 32);
const _: () = assert!(offset_of!(VideoHwInitializationData, hw_get_power_state) == 48);

#[repr(C)]
pub struct VideoPortConfigInfo {
    _header: [u8; 24],
    pub num_emulator_access_entries: u32,
    pub emulator_access_entries: *mut c_void,
    _middle: [u8; 8],
    pub vdm_physical_video_memory_address: i64,
    pub vdm_physical_video_memory_length: u32,
    pub hardware_state_size: u32,
    _tail: [u8; 56],
}
const _: () = assert!(size_of::<VideoPortConfigInfo>() == 112);
const _: () = assert!(offset_of!(VideoPortConfigInfo, num_emulator_access_entries) == 24);
const _: () = assert!(offset_of!(VideoPortConfigInfo, vdm_physical_video_memory_address) == 40);
const _: () = assert!(offset_of!(VideoPortConfigInfo, hardware_state_size) == 52);

#[repr(C)]
pub struct StatusBlock {
    pub status: VpStatus,
    pub information: usize,
}

#[repr(C)]
pub struct VideoRequestPacket {
    pub io_control_code: u32,
    pub status_block: *mut StatusBlock,
    pub input_buffer: *mut c_void,
    pub input_buffer_length: u32,
    pub output_buffer: *mut c_void,
    pub output_buffer_length: u32,
}
const _: () = assert!(size_of::<VideoRequestPacket>() == 24);

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct VideoAccessRange {
    pub range_start: i64,
    pub range_length: u32,
    pub range_in_io_space: u8,
    pub range_visible: u8,
    pub range_shareable: u8,
    pub range_passive: u8,
}
const _: () = assert!(size_of::<VideoAccessRange>() == 16);

#[repr(C)]
#[derive(Default)]
pub struct VideoModeInformation {
    pub length: u32,
    pub mode_index: u32,
    pub vis_screen_width: u32,
    pub vis_screen_height: u32,
    pub screen_stride: u32,
    pub number_of_planes: u32,
    pub bits_per_plane: u32,
    pub frequency: u32,
    pub x_millimeter: u32,
    pub y_millimeter: u32,
    pub number_red_bits: u32,
    pub number_green_bits: u32,
    pub number_blue_bits: u32,
    pub red_mask: u32,
    pub green_mask: u32,
    pub blue_mask: u32,
    pub attribute_flags: u32,
    pub video_memory_bitmap_width: u32,
    pub video_memory_bitmap_height: u32,
    pub driver_specific_attribute_flags: u32,
}
const _: () = assert!(size_of::<VideoModeInformation>() == 80);

#[repr(C)]
pub struct VideoNumModes {
    pub num_modes: u32,
    pub mode_information_length: u32,
}

#[repr(C)]
pub struct VideoMemoryInformation {
    pub video_ram_base: *mut c_void,
    pub video_ram_length: u32,
    pub frame_buffer_base: *mut c_void,
    pub frame_buffer_length: u32,
}

#[repr(C)]
pub struct VideoPointerCapabilities {
    pub flags: u32,
    pub max_width: u32,
    pub max_height: u32,
    pub hw_ptr_bitmap_start: u32,
    pub hw_ptr_bitmap_end: u32,
}

/// Followed by the pixels, at the size of the C structure, which counts its first pixel byte
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VideoPointerAttributes {
    pub flags: u32,
    pub width: u32,
    pub height: u32,
    pub width_in_bytes: u32,
    pub enable: u32,
    pub column: i16,
    pub row: i16,
}
pub const VIDEO_POINTER_ATTRIBUTES_SIZE: usize = 28;

#[repr(C)]
pub struct VideoPointerPosition {
    pub column: i16,
    pub row: i16,
}

/// Followed by `num_entries` red, green, blue and unused bytes
#[repr(C)]
pub struct VideoClut {
    pub num_entries: u16,
    pub first_entry: u16,
}

pub const IOCTL_VIDEO_QUERY_AVAIL_MODES: u32 = 0x230400;
pub const IOCTL_VIDEO_QUERY_NUM_AVAIL_MODES: u32 = 0x230404;
pub const IOCTL_VIDEO_QUERY_CURRENT_MODE: u32 = 0x230408;
pub const IOCTL_VIDEO_SET_CURRENT_MODE: u32 = 0x23040C;
pub const IOCTL_VIDEO_RESET_DEVICE: u32 = 0x230410;
pub const IOCTL_VIDEO_SET_COLOR_REGISTERS: u32 = 0x23041C;
pub const IOCTL_VIDEO_ENABLE_POINTER: u32 = 0x230438;
pub const IOCTL_VIDEO_DISABLE_POINTER: u32 = 0x23043C;
pub const IOCTL_VIDEO_SET_POINTER_ATTR: u32 = 0x230440;
pub const IOCTL_VIDEO_SET_POINTER_POSITION: u32 = 0x230448;
pub const IOCTL_VIDEO_QUERY_POINTER_CAPABILITIES: u32 = 0x230450;
pub const IOCTL_VIDEO_MAP_VIDEO_MEMORY: u32 = 0x230458;
pub const IOCTL_VIDEO_UNMAP_VIDEO_MEMORY: u32 = 0x23045C;

pub const VIDEO_MODE_COLOR: u32 = 1;
pub const VIDEO_MODE_GRAPHICS: u32 = 2;
pub const VIDEO_MODE_PALETTE_DRIVEN: u32 = 4;
pub const VIDEO_MODE_MANAGED_PALETTE: u32 = 8;
pub const VIDEO_MODE_ASYNC_POINTER: u32 = 1;
pub const VIDEO_MODE_MONO_POINTER: u32 = 2;
pub const VIDEO_MODE_COLOR_POINTER: u32 = 4;
pub const VIDEO_MEMORY_SPACE_MEMORY: u32 = 0;

#[link(
    name = "videoprt.sys",
    kind = "raw-dylib",
    modifiers = "+verbatim",
    import_name_type = "undecorated"
)]
unsafe extern "system" {
    pub fn VideoPortInitialize(
        argument1: *mut c_void,
        argument2: *mut c_void,
        init: *mut VideoHwInitializationData,
        context: *mut c_void,
    ) -> u32;
    pub fn VideoPortGetAccessRanges(
        extension: *mut c_void,
        num_requested: u32,
        requested: *mut c_void,
        num_ranges: u32,
        ranges: *mut VideoAccessRange,
        vendor_id: *mut c_void,
        device_id: *mut c_void,
        slot: *mut u32,
    ) -> VpStatus;
    pub fn VideoPortMapMemory(
        extension: *mut c_void,
        physical: i64,
        length: *mut u32,
        in_io_space: *mut u32,
        virtual_address: *mut *mut c_void,
    ) -> VpStatus;
    pub fn VideoPortUnmapMemory(
        extension: *mut c_void,
        virtual_address: *mut c_void,
        process: *mut c_void,
    ) -> VpStatus;
}
