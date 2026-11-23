// Helpers for running the XP disk headless in v86

import fs from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';
import zlib from 'node:zlib';
import { GuestHost } from '../web/src/guest-host.ts';
import { MACHINE, patchNetworkCard } from '../web/src/machine.ts';

const require = createRequire(new URL('../web/package.json', import.meta.url));
const { V86 } = require('v86');

const root = path.dirname(new URL(import.meta.url).pathname);
const file = (name) => new Uint8Array(fs.readFileSync(path.join(root, 'bios', name))).buffer;
export const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

export async function boot(hda, options = {}) {
    const emulator = new V86({
        ...MACHINE,
        ...options,
        wasm_path: require.resolve('v86/build/v86.wasm'),
        bios: { buffer: file('seabios.bin') },
        vga_bios: { buffer: file('vgabios.bin') },
        hda,
        autostart: true,
    });
    await new Promise((resolve) => emulator.add_listener('emulator-loaded', resolve));
    patchNetworkCard(emulator);
    return { emulator, host: new GuestHost(emulator) };
}

// Prints the "LOG <text>" lines guest scripts write to COM1, "ERROR <text>" stops the build
export function logSerial(emulator) {
    let line = '';
    emulator.add_listener('serial0-output-byte', (byte) => {
        if (byte !== 10 && byte !== 13) return void (line += String.fromCharCode(byte));
        const text = line.trim();
        line = '';
        if (text.startsWith('LOG ')) console.log(`guest: ${text.slice(4)}`);
        if (text.startsWith('ERROR ')) {
            console.error(`guest: ${text.slice(6)}`);
            process.exit(1);
        }
    });
}

export const disk = (emulator) => emulator.v86.cpu.devices.ide.primary.master.buffer;

export async function waitFor(emulator, what, check, minutes) {
    const start = Date.now();
    for (let tick = 0; !check(); tick++) {
        if (tick % 15 === 0) screenshot(emulator, path.join(root, 'out/progress.png'));
        if (Date.now() - start > minutes * 60 * 1000) {
            screenshot(emulator, path.join(root, 'out/timeout.png'));
            throw new Error(`timed out waiting for ${what}, see out/timeout.png`);
        }
        await sleep(1000);
    }
    console.log(`${what} after ${Math.round((Date.now() - start) / 1000)}s`);
}

// Waits until the guest stops writing to the disk, after it has powered off
export async function waitIdle(emulator, seconds) {
    const buffer = disk(emulator);
    let writes = -1;
    for (let quiet = 0; quiet < seconds;) {
        await sleep(1000);
        quiet = buffer.block_cache_is_write.size === writes ? quiet + 1 : 0;
        writes = buffer.block_cache_is_write.size;
    }
}

// Saves the VGA framebuffer as PNG, for debugging the headless runs
export function screenshot(emulator, file) {
    const vga = emulator.v86.cpu.devices.vga;
    if (!vga.graphical_mode) {
        // Text mode: dump the 80x25 screen next to it
        // Character and attribute pairs
        const lines = [];
        for (let y = 0; y < 25; y++) {
            let line = '';
            for (let x = 0; x < 80; x++) line += String.fromCharCode(vga.vga_memory[(y * 80 + x) * 2] || 32);
            lines.push(line.trimEnd());
        }
        fs.writeFileSync(file.replace(/\.png$/, '.txt'), lines.join('\n') + '\n');
        return;
    }
    const [width, height] = [vga.svga_width || 1024, vga.svga_height || 768];
    const rgb = Buffer.alloc(width * height * 3);
    for (let i = 0; i < width * height; i++) {
        rgb[i * 3] = vga.svga_memory[i * 4 + 2];
        rgb[i * 3 + 1] = vga.svga_memory[i * 4 + 1];
        rgb[i * 3 + 2] = vga.svga_memory[i * 4];
    }
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, encodePng(width, height, rgb));
}

// Encodes 8-bit RGB pixels as PNG
export function encodePng(width, height, rgb) {
    const raw = Buffer.alloc((width * 3 + 1) * height);
    for (let y = 0; y < height; y++) rgb.copy(raw, y * (width * 3 + 1) + 1, y * width * 3, (y + 1) * width * 3);
    const chunk = (type, data) => {
        const body = Buffer.concat([Buffer.from(type), data]);
        const length = Buffer.alloc(4);
        length.writeUInt32BE(data.length);
        const crc = Buffer.alloc(4);
        crc.writeUInt32BE(zlib.crc32(body));
        return Buffer.concat([length, body, crc]);
    };
    const header = Buffer.alloc(13);
    header.writeUInt32BE(width, 0);
    header.writeUInt32BE(height, 4);
    header.set([8, 2, 0, 0, 0], 8);
    return Buffer.concat([
        Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
        chunk('IHDR', header),
        chunk('IDAT', zlib.deflateSync(raw, { level: 9 })),
        chunk('IEND', Buffer.alloc(0)),
    ]);
}
