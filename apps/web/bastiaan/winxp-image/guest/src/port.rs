/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! x86 port I/O for the drivers: the VMware backdoor of v86 and the registers of the emulated
//! hardware. Only kernel mode can touch ports under the IOPL of Windows XP.

use core::arch::asm;

use crate::protocol::{VMWARE_CMD_GETVERSION, VMWARE_MAGIC, VMWARE_PORT};

/// The registers of a backdoor call: commands go in ECX, arguments in EBX, ESI and EDI
#[derive(Clone, Copy, Default)]
pub struct Regs {
    pub eax: u32,
    pub ebx: u32,
    pub ecx: u32,
    pub edx: u32,
    pub esi: u32,
    pub edi: u32,
}

impl Regs {
    pub const fn command(command: u32, ebx: u32) -> Self {
        Self {
            eax: 0,
            ebx,
            ecx: command,
            edx: 0,
            esi: 0,
            edi: 0,
        }
    }
}

/// Calls the backdoor with all registers
///
/// # Safety
///
/// Must run in kernel mode. Commands that pass buffers need them physically contiguous and
/// nonpaged for as long as the call.
pub unsafe fn backdoor(regs: Regs) -> Regs {
    // LLVM reserves ESI, so ESI and EDI travel through memory that EDI points to
    let mut esi_edi = [regs.esi, regs.edi];
    let (eax, ebx, ecx, edx);
    // SAFETY: The caller runs in kernel mode, the port is the backdoor and ESI is restored
    unsafe {
        asm!(
            "push esi",
            "push edi",
            "mov esi, [edi]",
            "mov edi, [edi + 4]",
            "in eax, dx",
            "xchg edi, [esp]",
            "mov [edi], esi",
            "pop dword ptr [edi + 4]",
            "pop esi",
            inout("eax") VMWARE_MAGIC => eax,
            inout("ebx") regs.ebx => ebx,
            inout("ecx") regs.ecx => ecx,
            inout("edx") u32::from(VMWARE_PORT) => edx,
            inout("edi") esi_edi.as_mut_ptr() => _,
        );
    }
    Regs {
        eax,
        ebx,
        ecx,
        edx,
        esi: esi_edi[0],
        edi: esi_edi[1],
    }
}

/// Calls a backdoor command with one argument, returns EAX
///
/// # Safety
///
/// Like [`backdoor`].
pub unsafe fn call(command: u32, ebx: u32) -> u32 {
    // SAFETY: The caller upholds the contract of backdoor
    unsafe { backdoor(Regs::command(command, ebx)) }.eax
}

/// Whether the backdoor answers, so the machine is v86 or VMware
///
/// # Safety
///
/// Must run in kernel mode.
pub unsafe fn host_present() -> bool {
    // SAFETY: The caller runs in kernel mode
    unsafe { backdoor(Regs::command(VMWARE_CMD_GETVERSION, 0)) }.ebx == VMWARE_MAGIC
}

/// # Safety
///
/// Must run in kernel mode, the port must belong to the driver.
pub unsafe fn write_u8(port: u16, value: u8) {
    // SAFETY: The caller owns the port
    unsafe { asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack)) };
}

/// # Safety
///
/// Must run in kernel mode, the port must belong to the driver.
pub unsafe fn write_u16(port: u16, value: u16) {
    // SAFETY: The caller owns the port
    unsafe { asm!("out dx, ax", in("dx") port, in("ax") value, options(nomem, nostack)) };
}

/// # Safety
///
/// Must run in kernel mode, the port must belong to the driver.
pub unsafe fn read_u16(port: u16) -> u16 {
    let value;
    // SAFETY: The caller owns the port
    unsafe { asm!("in ax, dx", in("dx") port, out("ax") value, options(nomem, nostack)) };
    value
}
