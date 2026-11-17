/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

import type { V86, V86Options } from 'v86';

// The emulated machine, shared by the site and the winxp-image/ build: a snapshot only restores on the same machine
export const DISK_CHUNK_SIZE = 1024 * 1024;

export const MACHINE = {
    memory_size: 256 * 1024 * 1024,
    vga_memory_size: 16 * 1024 * 1024,
    acpi: false, // XP runs the Standard PC HAL
    cpuid_level: 2,
    net_device: { type: 'ne2k' }, // XP's in-box RTL8029 driver, the site connects it to the server's LAN relay
} satisfies V86Options;

// The parts of v86's internals the site and the build use, its types only declare the public API
export interface AsyncDisk {
    block_cache: Map<number, Uint8Array>;
    block_cache_is_write: Set<number>;
    set(offset: number, data: Uint8Array, done: () => void): void;
    get_state(): unknown;
}

export interface NetworkCard {
    port: number;
    cr: number;
    pstop: number;
    mac: Uint8Array;
    mac_address_in_state?: Uint8Array;
}

export interface Cpu {
    mem8: Uint8Array;
    reg32: Int32Array;
    cr: Int32Array;
    io: {
        ports: { write8: (this: unknown, value: number) => void }[];
        register_read(
            port: number,
            device: object,
            read8: () => number,
            read16: () => number,
            read32: () => number,
        ): void;
    };
    devices: {
        net?: NetworkCard;
        vmware: { port_read32(): number; queue: unknown[] };
        vga: { svga_width: number; svga_height: number };
        ide: { primary: { master: { buffer: AsyncDisk } } };
    };
    write_blob(data: Uint8Array, address: number): void;
}

export const cpu = (emulator: V86): Cpu => (emulator as unknown as { v86: { cpu: Cpu } }).v86.cpu;

// XP's RTL8029 driver selects register page 3 (Realtek's own, which v86 does not have) and then writes 0xFF, landing
// in PSTART on page 0: a receive ring that starts past its end (PSTOP) drops every packet, keep the one set up before
export function patchNetworkCard(emulator: V86) {
    const { devices, io } = cpu(emulator);
    const net = devices.net;
    if (!net) return;
    const port = io.ports[net.port + 1];
    const write = port.write8;
    port.write8 = function (value) {
        if ((net.cr & 0xc0) === 0 && value >= net.pstop) return;
        write.call(this, value);
    };
}
