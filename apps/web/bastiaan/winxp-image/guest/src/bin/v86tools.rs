/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! v86tools.sys - host channel for the user mode guest agent.
//!
//! Exposes `\\.\V86Tools` for display size, clock and message transfers. Ring 3 code can't touch
//! the backdoor port under the IOPL of Windows XP, so IOCTLs proxy it here.

#![no_std]
#![no_main]

use core::ffi::c_void;
use core::panic::PanicInfo;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};

use v86::ddk::*;
use v86::port::{self, Regs};
use v86::protocol::*;
use v86::wide;

const BUFFER_SIZE: usize = V86_BUFFER_SIZE as usize;
const DEVICE_NAME: UnicodeString = UnicodeString::new(wide!(V86_DEVICE_NAME));
const DOS_NAME: UnicodeString = UnicodeString::new(wide!(V86_DOS_NAME));

static HOST: AtomicBool = AtomicBool::new(false);
static BUFFER: AtomicPtr<u8> = AtomicPtr::new(ptr::null_mut());
static BUFFER_PHYSICAL: AtomicU32 = AtomicU32::new(0);
static LOCK: KernelMutex = KernelMutex::new();

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    bug_check()
}

/// The control device: the window of the agent onto the host. Returns the status and the length
/// of the output.
///
/// # Safety
///
/// The system buffer holds the larger of the input and output lengths.
unsafe fn control(code: u32, buffer: *mut u8, in_len: usize, out_len: usize) -> (NtStatus, usize) {
    const WORD: usize = size_of::<u32>();
    if !HOST.load(Ordering::Relaxed) {
        return (STATUS_DEVICE_NOT_CONNECTED, 0);
    }
    let words = buffer.cast::<u32>();
    let shared = BUFFER.load(Ordering::Relaxed);
    let physical = BUFFER_PHYSICAL.load(Ordering::Relaxed);
    // SAFETY: The lengths are checked against the system buffer before each access, the shared
    // buffer holds BUFFER_SIZE bytes and the lock keeps it for one transfer at a time
    unsafe {
        match code {
            IOCTL_V86_GETSIZE if out_len >= WORD => {
                words.write_unaligned(port::call(V86_CMD_GETSIZE, 0));
                (STATUS_SUCCESS, WORD)
            }
            IOCTL_V86_GETTIME if out_len >= 3 * WORD => {
                let regs = port::backdoor(Regs::command(VMWARE_CMD_GETTIME, 0));
                words.write_unaligned(regs.eax);
                words.add(1).write_unaligned(regs.ebx);
                words.add(2).write_unaligned(regs.edx);
                (STATUS_SUCCESS, 3 * WORD)
            }
            IOCTL_V86_RECV if out_len >= 2 * WORD && !shared.is_null() => {
                let capacity = (out_len - 2 * WORD).min(BUFFER_SIZE);
                let _guard = LOCK.lock();
                let regs = port::backdoor(Regs {
                    ebx: physical,
                    ecx: V86_CMD_RECV,
                    esi: capacity as u32,
                    ..Regs::default()
                });
                let len = if regs.eax == V86_MSG_NONE {
                    0
                } else {
                    (regs.ebx as usize).min(capacity)
                };
                words.write_unaligned(regs.eax);
                words.add(1).write_unaligned(len as u32);
                ptr::copy_nonoverlapping(shared, buffer.add(2 * WORD), len);
                (STATUS_SUCCESS, 2 * WORD + len)
            }
            IOCTL_V86_SEND if in_len >= WORD && !shared.is_null() => {
                let len = (in_len - WORD).min(BUFFER_SIZE);
                let _guard = LOCK.lock();
                ptr::copy_nonoverlapping(buffer.add(WORD), shared, len);
                port::backdoor(Regs {
                    ebx: physical,
                    ecx: V86_CMD_SEND,
                    esi: len as u32,
                    edi: words.read_unaligned(),
                    ..Regs::default()
                });
                (STATUS_SUCCESS, 0)
            }
            IOCTL_V86_GETSIZE | IOCTL_V86_GETTIME | IOCTL_V86_RECV => (STATUS_BUFFER_TOO_SMALL, 0),
            IOCTL_V86_SEND => (STATUS_INVALID_PARAMETER, 0),
            _ => (STATUS_INVALID_DEVICE_REQUEST, 0),
        }
    }
}

unsafe extern "system" fn dispatch(_device: *mut DeviceObject, irp: *mut Irp) -> NtStatus {
    // SAFETY: The I/O manager passes a valid IRP for this device, buffered I/O gives it a
    // system buffer for the control codes
    unsafe {
        let stack = &*IoGetCurrentIrpStackLocation(irp);
        (*irp).io_status.information = 0;
        let status = match stack.major_function {
            IRP_MJ_CREATE | IRP_MJ_CLOSE | IRP_MJ_CLEANUP => STATUS_SUCCESS,
            IRP_MJ_DEVICE_CONTROL => {
                let (status, information) = control(
                    stack.io_control_code(),
                    (*irp).system_buffer.cast(),
                    stack.input_buffer_length() as usize,
                    stack.output_buffer_length() as usize,
                );
                (*irp).io_status.information = information;
                status
            }
            _ => STATUS_INVALID_DEVICE_REQUEST,
        };
        complete_request(irp, status)
    }
}

unsafe extern "system" fn unload(driver: *mut DriverObject) {
    // SAFETY: The driver made the link, the device and the buffer in DriverEntry
    unsafe {
        IoDeleteSymbolicLink(&DOS_NAME);
        IoDeleteDevice((*driver).device_object);
        MmFreeContiguousMemory(BUFFER.load(Ordering::Relaxed).cast());
    }
}

/// # Safety
///
/// Called once by the kernel with the driver object.
#[unsafe(no_mangle)]
unsafe extern "system" fn DriverEntry(driver: *mut DriverObject, _path: *mut c_void) -> NtStatus {
    // SAFETY: The kernel calls this once at PASSIVE_LEVEL with a valid driver object
    unsafe {
        HOST.store(port::host_present(), Ordering::Relaxed);
        (*driver).major_function = [Some(dispatch as DriverDispatch); IRP_MJ_COUNT];
        (*driver).driver_unload = Some(unload);
        LOCK.init();

        let buffer = MmAllocateContiguousMemory(BUFFER_SIZE, 0x7FFF_FFFF);
        if buffer.is_null() {
            return STATUS_INSUFFICIENT_RESOURCES;
        }
        BUFFER.store(buffer.cast(), Ordering::Relaxed);
        BUFFER_PHYSICAL.store(MmGetPhysicalAddress(buffer) as u32, Ordering::Relaxed);

        let mut device = ptr::null_mut();
        let mut status = IoCreateDevice(
            driver,
            0,
            &DEVICE_NAME,
            FILE_DEVICE_UNKNOWN,
            0,
            0,
            &mut device,
        );
        if nt_success(status) {
            status = IoCreateSymbolicLink(&DOS_NAME, &DEVICE_NAME);
            if !nt_success(status) {
                IoDeleteDevice(device);
            }
        }
        if !nt_success(status) {
            MmFreeContiguousMemory(buffer);
            return status;
        }
        (*device).flags |= DO_BUFFERED_IO;
        (*device).flags &= !DO_DEVICE_INITIALIZING;
        STATUS_SUCCESS
    }
}
