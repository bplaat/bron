/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// Guest memory at linear addresses, through the page tables of the code that is running: the drivers hand the page
// pointers into their own address space. Pages that are not present make a read fail, the image has no pagefile.

// The parts of the CPU this needs, so tests can stand in for v86
export interface GuestCpu {
    mem8: Uint8Array;
    cr: Int32Array;
}

const u32 = (mem: Uint8Array, address: number) =>
    (mem[address] | (mem[address + 1] << 8) | (mem[address + 2] << 16) | (mem[address + 3] << 24)) >>> 0;

// The physical address of a linear address, -1 when its page is not present
export function translate(cpu: GuestCpu, linear: number) {
    const mem = cpu.mem8;
    if (!(cpu.cr[0] & 0x80000000)) return linear;
    if (cpu.cr[4] & 0x20) {
        // PAE: 64-bit entries, the guest has less than 4 GB so the low halves are enough
        const pdpte = u32(mem, (cpu.cr[3] & 0xffffffe0) + (linear >>> 30) * 8);
        if (!(pdpte & 1)) return -1;
        const pde = u32(mem, (pdpte & 0xfffff000) + ((linear >>> 21) & 0x1ff) * 8);
        if (!(pde & 1)) return -1;
        if (pde & 0x80) return ((pde & 0xffe00000) | (linear & 0x1fffff)) >>> 0;
        const pte = u32(mem, (pde & 0xfffff000) + ((linear >>> 12) & 0x1ff) * 8);
        return pte & 1 ? ((pte & 0xfffff000) | (linear & 0xfff)) >>> 0 : -1;
    }
    const pde = u32(mem, (cpu.cr[3] & 0xfffff000) + (linear >>> 22) * 4);
    if (!(pde & 1)) return -1;
    if (pde & 0x80 && cpu.cr[4] & 0x10) return ((pde & 0xffc00000) | (linear & 0x3fffff)) >>> 0;
    const pte = u32(mem, (pde & 0xfffff000) + ((linear >>> 12) & 0x3ff) * 4);
    return pte & 1 ? ((pte & 0xfffff000) | (linear & 0xfff)) >>> 0 : -1;
}

// Copies guest memory into `out` at `offset`, false when a page is not present
export function readInto(cpu: GuestCpu, address: number, length: number, out: Uint8Array, offset = 0) {
    for (let done = 0; done < length;) {
        const linear = (address + done) >>> 0;
        const physical = translate(cpu, linear);
        const count = Math.min(length - done, 0x1000 - (linear & 0xfff));
        if (physical < 0 || physical + count > cpu.mem8.length) return false;
        out.set(cpu.mem8.subarray(physical, physical + count), offset + done);
        done += count;
    }
    return true;
}

export function readLinear(cpu: GuestCpu, address: number, length: number) {
    const out = new Uint8Array(length);
    return readInto(cpu, address, length, out) ? out : null;
}
