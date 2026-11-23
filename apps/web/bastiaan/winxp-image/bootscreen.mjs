#!/usr/bin/env node
// Extracts the Windows XP boot screen from the kernel for the site's loading screen: the 640x480 logo (bitmap 1) and
// the progress bar blocks (bitmap 8). Both are 4-bit bitmaps with an empty palette, the kernel holds the real one.
// Usage: bootscreen.mjs <disk.img> <output dir>

import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { encodePng } from './lib.mjs';

const [image, outDir] = process.argv.slice(2);
const part = Buffer.alloc(4);
fs.readSync(fs.openSync(image, 'r'), part, 0, 4, 454);
const kernelFile = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'winxp-')), 'ntoskrnl.exe');
execFileSync(
    'mcopy',
    ['-i', `${image}@@${part.readUInt32LE(0) * 512}`, '::/WINDOWS/system32/ntoskrnl.exe', kernelFile],
    {
        env: { ...process.env, MTOOLS_SKIP_CHECK: '1' },
    },
);
const pe = fs.readFileSync(kernelFile);
fs.rmSync(path.dirname(kernelFile), { recursive: true });

// PE resources: type -> name -> language -> data
const header = pe.readUInt32LE(0x3c);
const optional = header + 24;
const sections = Array.from({ length: pe.readUInt16LE(header + 6) }, (_, i) => {
    const s = optional + pe.readUInt16LE(header + 20) + i * 40;
    return { va: pe.readUInt32LE(s + 12), size: pe.readUInt32LE(s + 16), raw: pe.readUInt32LE(s + 20) };
});
const offset = (rva) => {
    const s = sections.find((s) => rva >= s.va && rva < s.va + s.size);
    return rva - s.va + s.raw;
};
const resources = offset(pe.readUInt32LE(optional + 96 + 2 * 8));
const entry = (dir, id) => {
    const count = pe.readUInt16LE(dir + 12) + pe.readUInt16LE(dir + 14);
    for (let i = 0; i < count; i++) {
        if (pe.readUInt32LE(dir + 16 + i * 8) === id || id === undefined) {
            return resources + (pe.readUInt32LE(dir + 20 + i * 8) & 0x7fffffff);
        }
    }
    throw new Error(`no resource ${id}`);
};
function bitmap(id) {
    const data = entry(entry(entry(resources, 2 /* RT_BITMAP */), id));
    const start = offset(pe.readUInt32LE(data));
    return pe.subarray(start, start + pe.readUInt32LE(data + 4));
}

// The boot palette: bitmap 5 carries most of it, shifted by one entry
const reference = bitmap(5).subarray(40 + 2 * 4, 40 + 16 * 4);
const paletteAt = pe.indexOf(reference) - 4;
if (paletteAt < 0) throw new Error('boot palette not found');
const palette = pe.subarray(paletteAt, paletteAt + 16 * 4);

// 4-bit DIB, uncompressed or RLE4, bottom up
function decode(dib) {
    const width = dib.readInt32LE(4);
    const height = dib.readInt32LE(8);
    const compression = dib.readUInt32LE(16);
    const bits = dib.subarray(dib.readUInt32LE(0) + (dib.readUInt32LE(32) || 16) * 4);
    const index = new Uint8Array(width * height);
    if (compression === 0) {
        const stride = ((width + 7) >> 3) * 4;
        for (let y = 0; y < height; y++)
            for (let x = 0; x < width; x++)
                index[y * width + x] = (bits[y * stride + (x >> 1)] >> (x & 1 ? 0 : 4)) & 15;
    } else {
        let x = 0;
        let y = 0;
        for (let i = 0; i < bits.length;) {
            const count = bits[i++];
            const value = bits[i++];
            if (count) {
                for (let n = 0; n < count; n++) index[y * width + x++] = n & 1 ? value & 15 : value >> 4;
            } else if (value === 0) {
                x = 0;
                y++;
            } else if (value === 1) {
                break;
            } else if (value === 2) {
                x += bits[i++];
                y += bits[i++];
            } else {
                for (let n = 0; n < value; n++)
                    index[y * width + x++] = n & 1 ? bits[i + (n >> 1)] & 15 : bits[i + (n >> 1)] >> 4;
                i += ((value + 3) >> 2) << 1;
            }
        }
    }
    const rgb = Buffer.alloc(width * height * 3);
    for (let y = 0; y < height; y++) {
        for (let x = 0; x < width; x++) {
            const color = index[(height - 1 - y) * width + x] * 4;
            rgb.set([palette[color + 2], palette[color + 1], palette[color]], (y * width + x) * 3);
        }
    }
    return encodePng(width, height, rgb);
}

fs.mkdirSync(outDir, { recursive: true });
fs.writeFileSync(path.join(outDir, 'boot.png'), decode(bitmap(1)));
fs.writeFileSync(path.join(outDir, 'boot-bar.png'), decode(bitmap(8)));
console.log('boot screen extracted');
