/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! v86mou.sys - absolute mouse filter for v86.
//!
//! Sits above mouclass, so i8042prt keeps its stock stack. While the VMware absolute pointer is
//! enabled, v86 still raises PS/2 packets for host events; each mouse read they complete is
//! rewritten with the queued backdoor packets.

#![no_std]
#![no_main]

use core::ffi::c_void;
use core::panic::PanicInfo;
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};

use v86::ddk::*;
use v86::port::{self, Regs};
use v86::protocol::*;

const BUTTONS: u32 = VMWARE_BUTTON_LEFT | VMWARE_BUTTON_RIGHT | VMWARE_BUTTON_MIDDLE;
/// Button mask with its down and up flags
const BUTTON_FLAGS: [(u32, u16, u16); 3] = [
    (
        VMWARE_BUTTON_LEFT,
        MOUSE_LEFT_BUTTON_DOWN,
        MOUSE_LEFT_BUTTON_UP,
    ),
    (
        VMWARE_BUTTON_RIGHT,
        MOUSE_RIGHT_BUTTON_DOWN,
        MOUSE_RIGHT_BUTTON_UP,
    ),
    (
        VMWARE_BUTTON_MIDDLE,
        MOUSE_MIDDLE_BUTTON_DOWN,
        MOUSE_MIDDLE_BUTTON_UP,
    ),
];

static HOST: AtomicBool = AtomicBool::new(false);

/// The device extension
#[repr(C)]
struct Filter {
    lower: *mut DeviceObject,
    x: i32,
    y: i32,
    buttons: u32,
    started: bool,
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    bug_check()
}

/// # Safety
///
/// Kernel mode.
unsafe fn enable_absolute() {
    // SAFETY: The caller runs in kernel mode
    unsafe {
        port::call(VMWARE_CMD_ABSPOINTER_COMMAND, VMWARE_ABSPOINTER_ENABLE);
        if port::call(VMWARE_CMD_ABSPOINTER_STATUS, 0) & 0xFFFF >= 1 {
            // Consume the READ_ID
            port::call(VMWARE_CMD_ABSPOINTER_DATA, 1);
        }
        port::call(VMWARE_CMD_ABSPOINTER_COMMAND, VMWARE_ABSPOINTER_ABSOLUTE);
    }
}

fn button_flags(old: u32, now: u32) -> u16 {
    BUTTON_FLAGS
        .iter()
        .filter(|(mask, _, _)| (old ^ now) & mask != 0)
        .fold(0, |flags, &(mask, down, up)| {
            flags | if now & mask != 0 { down } else { up }
        })
}

impl Filter {
    /// Replaces one PS/2 record with the next backdoor packet, or a no-op at the current position
    ///
    /// # Safety
    ///
    /// Kernel mode, with a packet queued when `packet` is set.
    unsafe fn rewrite(&mut self, input: &mut MouseInputData, packet: bool) {
        input.flags = MOUSE_MOVE_ABSOLUTE;
        input.button_flags = 0;
        input.button_data = 0;
        input.raw_buttons = 0;
        if packet {
            // SAFETY: The caller runs in kernel mode with a packet queued
            let regs = unsafe { port::backdoor(Regs::command(VMWARE_CMD_ABSPOINTER_DATA, 4)) };
            let buttons = regs.eax & BUTTONS;
            input.button_flags = button_flags(self.buttons, buttons);
            self.buttons = buttons;
            let wheel = i32::from(regs.edx as u8 as i8);
            if wheel != 0 {
                input.button_flags |= MOUSE_WHEEL;
                input.button_data = (-wheel * 120) as u16;
            }
            if regs.eax & VMWARE_RELATIVE_PACKET != 0 {
                // The host pointer is locked: relative motion, positive y is up
                input.flags = MOUSE_MOVE_RELATIVE;
                input.last_x = regs.ebx as i32;
                input.last_y = (regs.ecx as i32).wrapping_neg();
                return;
            }
            self.x = regs.ebx as i32;
            self.y = regs.ecx as i32;
        }
        input.last_x = self.x;
        input.last_y = self.y;
    }
}

unsafe extern "system" fn read_done(
    device: *mut DeviceObject,
    irp: *mut Irp,
    _context: *mut c_void,
) -> NtStatus {
    // SAFETY: The I/O manager completes a read of this device, mouclass gave a system buffer
    // of the read length
    unsafe {
        if (*irp).pending_returned != 0 {
            IoMarkIrpPending(irp);
        }
        if !HOST.load(Ordering::Relaxed) || !nt_success((*irp).io_status.status) {
            return STATUS_CONTINUE_COMPLETION;
        }

        let status = port::call(VMWARE_CMD_ABSPOINTER_STATUS, 0);
        if status & 0xFFFF0000 == 0xFFFF0000 {
            // Absolute mode is off (or the host dropped it): pass PS/2 through
            enable_absolute();
            return STATUS_CONTINUE_COMPLETION;
        }
        // Emits every queued packet (v86 may merge PS/2 packets), padding with no-ops
        let filter = &mut *(*device).device_extension.cast::<Filter>();
        let record = size_of::<MouseInputData>();
        let count = (*irp).io_status.information / record;
        let capacity = (*IoGetCurrentIrpStackLocation(irp)).read_length() as usize / record;
        let packets = (status & 0xFFFF) as usize / 4;
        let total = count.max(packets).min(capacity);
        if total == 0 {
            return STATUS_CONTINUE_COMPLETION;
        }
        let data =
            core::slice::from_raw_parts_mut((*irp).system_buffer.cast::<MouseInputData>(), total);
        let first = data[0];
        data[count.min(total)..].fill(first);
        for (i, input) in data.iter_mut().enumerate() {
            filter.rewrite(input, i < packets);
        }
        (*irp).io_status.information = total * record;
        STATUS_CONTINUE_COMPLETION
    }
}

unsafe extern "system" fn attributes_done(
    _device: *mut DeviceObject,
    irp: *mut Irp,
    _context: *mut c_void,
) -> NtStatus {
    // SAFETY: The I/O manager completes the attributes query, its buffer holds the attributes
    // when the information says so
    unsafe {
        if (*irp).pending_returned != 0 {
            IoMarkIrpPending(irp);
        }
        if nt_success((*irp).io_status.status)
            && (*irp).io_status.information >= size_of::<MouseAttributes>()
        {
            // Advertises a wheel: the backdoor reports it even though PS/2 runs 3-byte
            let attributes = &mut *(*irp).system_buffer.cast::<MouseAttributes>();
            attributes.mouse_identifier = WHEELMOUSE_I8042_HARDWARE;
            attributes.number_of_buttons = attributes.number_of_buttons.max(3);
        }
        STATUS_CONTINUE_COMPLETION
    }
}

/// # Safety
///
/// The IRP belongs to the filter and has a next stack location.
unsafe fn forward(lower: *mut DeviceObject, irp: *mut Irp, done: IoCompletionRoutine) -> NtStatus {
    // SAFETY: The caller owns the IRP, which goes to the lower driver
    unsafe {
        IoCopyCurrentIrpStackLocationToNext(irp);
        IoSetCompletionRoutine(irp, done);
        IoCallDriver(lower, irp)
    }
}

unsafe extern "system" fn dispatch(device: *mut DeviceObject, irp: *mut Irp) -> NtStatus {
    // SAFETY: The I/O manager passes a valid IRP for a device of the filter, which has a lower
    // device and stack locations for it
    unsafe {
        let stack = &*IoGetCurrentIrpStackLocation(irp);
        // Through the pointer, the read completion can use the extension at the same time
        let filter = (*device).device_extension.cast::<Filter>();
        let lower = (*filter).lower;
        match stack.major_function {
            IRP_MJ_READ => {
                // Switches to absolute before the first event, so none arrives relative
                if HOST.load(Ordering::Relaxed) && !(*filter).started {
                    enable_absolute();
                }
                (*filter).started = true;
                return forward(lower, irp, read_done);
            }
            IRP_MJ_DEVICE_CONTROL | IRP_MJ_INTERNAL_DEVICE_CONTROL
                if stack.io_control_code() == IOCTL_MOUSE_QUERY_ATTRIBUTES =>
            {
                return forward(lower, irp, attributes_done);
            }
            IRP_MJ_POWER => {
                PoStartNextPowerIrp(irp);
                IoSkipCurrentIrpStackLocation(irp);
                return PoCallDriver(lower, irp);
            }
            IRP_MJ_PNP if stack.minor_function == IRP_MN_REMOVE_DEVICE => {
                IoSkipCurrentIrpStackLocation(irp);
                let status = IoCallDriver(lower, irp);
                IoDetachDevice(lower);
                IoDeleteDevice(device);
                return status;
            }
            _ => {}
        }
        IoSkipCurrentIrpStackLocation(irp);
        IoCallDriver(lower, irp)
    }
}

unsafe extern "system" fn add_device(
    driver: *mut DriverObject,
    pdo: *mut DeviceObject,
) -> NtStatus {
    // SAFETY: The PnP manager passes the driver and the device to filter
    unsafe {
        let mut device = ptr::null_mut();
        let status = IoCreateDevice(
            driver,
            size_of::<Filter>() as u32,
            ptr::null(),
            FILE_DEVICE_MOUSE,
            0,
            0,
            &mut device,
        );
        if !nt_success(status) {
            return status;
        }
        let lower = IoAttachDeviceToDeviceStack(device, pdo);
        if lower.is_null() {
            IoDeleteDevice(device);
            return STATUS_DEVICE_NOT_CONNECTED;
        }
        (*device).device_extension.cast::<Filter>().write(Filter {
            lower,
            x: 0,
            y: 0,
            buttons: 0,
            started: false,
        });
        (*device).flags |= (*lower).flags & (DO_BUFFERED_IO | DO_DIRECT_IO | DO_POWER_PAGABLE);
        (*device).flags &= !DO_DEVICE_INITIALIZING;
        STATUS_SUCCESS
    }
}

/// # Safety
///
/// Called once by the kernel with the driver object.
#[unsafe(no_mangle)]
unsafe extern "system" fn DriverEntry(driver: *mut DriverObject, _path: *mut c_void) -> NtStatus {
    // SAFETY: The kernel calls this once with a valid driver object
    unsafe {
        HOST.store(port::host_present(), Ordering::Relaxed);
        (*driver).major_function = [Some(dispatch as DriverDispatch); IRP_MJ_COUNT];
        (*(*driver).driver_extension).add_device = Some(add_device);
    }
    STATUS_SUCCESS
}
