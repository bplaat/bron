/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// The 2D engine of the display driver against straightforward reference drawing, the command decoding through
// guest page tables and the protocol constants against the Rust side

import assert from 'node:assert/strict';
import fs from 'node:fs';
import { describe, test } from 'node:test';
import { alpha, blt, combiner, rop3, text, type Blt, type Memory, type Rect, type Surface } from '../src/gpu/blit.ts';
import { GpuDevice, type GpuCpu } from '../src/gpu/device.ts';
import * as protocol from '../src/gpu/protocol.ts';
import { readLinear, translate } from '../src/guest-memory.ts';

const VRAM = 0;
const GUEST = 1;

// A pseudo random sequence, the same on every run
function random(seed: number) {
    let state = seed >>> 0 || 1;
    return () => {
        state ^= state << 13;
        state ^= state >>> 17;
        state ^= state << 5;
        return (state >>> 0) / 0x100000000;
    };
}

// VRAM and flat guest memory where linear addresses are physical
function machine(vramSize = 1 << 16, guestSize = 1 << 16) {
    const vram = new Uint8Array(vramSize);
    const guest = new Uint8Array(guestSize);
    const memory: Memory = {
        vram,
        read(address, length, out) {
            if (address < 0 || address + length > guest.length) return false;
            out.set(guest.subarray(address, address + length));
            return true;
        },
    };
    return { vram, guest, memory };
}

const surface = (
    kind: number,
    address: number,
    width: number,
    height: number,
    bpp: number,
    pitch?: number,
): Surface => ({
    kind,
    address,
    pitch: pitch ?? Math.ceil((width * bpp) / 32) * 4,
    bpp,
    width,
    height,
});
const rect = (left: number, top: number, right: number, bottom: number): Rect => ({ left, top, right, bottom });

function operation(op: Partial<Blt> & Pick<Blt, 'dst' | 'dstRect'>): Blt {
    return {
        rop: 0xcc,
        src: null,
        srcRect: op.dstRect,
        color: 0,
        pattern: null,
        patternX: 0,
        patternY: 0,
        xlate: null,
        srcKey: null,
        dstKey: null,
        mirrorX: false,
        mirrorY: false,
        clips: [op.dstRect],
        ...op,
    };
}

// Pixels of a surface as numbers, row after row
function pixels(bytes: Uint8Array, s: Surface) {
    const out: number[] = [];
    const view = new DataView(bytes.buffer, bytes.byteOffset);
    for (let y = 0; y < s.height; y++) {
        for (let x = 0; x < s.width; x++) {
            const at = s.address + y * s.pitch + (x * s.bpp) / 8;
            out.push(s.bpp === 32 ? view.getUint32(at, true) : s.bpp === 16 ? view.getUint16(at, true) : bytes[at]);
        }
    }
    return out;
}

function fillRandom(bytes: Uint8Array, seed: number) {
    const next = random(seed);
    for (let i = 0; i < bytes.length; i++) bytes[i] = next() * 256;
}

describe('protocol', () => {
    test('the constants match guest/src/gpu.rs', () => {
        const rust = fs.readFileSync(new URL('../../winxp-image/guest/src/gpu.rs', import.meta.url), 'utf8');
        const constants = [...rust.matchAll(/^pub const (\w+): (?:u32|usize) = ([0-9a-fx_]+);$/gim)];
        assert.deepEqual(constants.map(([, name]) => name).sort(), Object.keys(protocol).sort());
        for (const [, name, value] of constants) {
            const ts = (protocol as Record<string, number>)[name];
            assert.equal(ts >>> 0, Number(value.replaceAll('_', '')), name);
        }
    });
});

describe('ROP3', () => {
    test('the common ROPs equal the truth table', () => {
        const next = random(1);
        for (const rop of [0x00, 0x33, 0x55, 0x5a, 0x66, 0x88, 0xcc, 0xee, 0xf0, 0xff, 0xb8, 0xe2]) {
            for (let i = 0; i < 100; i++) {
                const [p, s, d] = [next() * 2 ** 32, next() * 2 ** 32, next() * 2 ** 32].map((v) => v >>> 0);
                assert.equal(combiner(rop)(p, s, d) >>> 0, rop3(rop, p, s, d), `rop ${rop}`);
            }
        }
    });

    test('each bit follows its pattern, source and destination bits', () => {
        for (let rop = 0; rop < 256; rop++) {
            for (let index = 0; index < 8; index++) {
                const bit = (v: number) => (v ? 1 : 0);
                const result = rop3(rop, bit(index & 4), bit(index & 2), bit(index & 1)) & 1;
                assert.equal(result, (rop >> index) & 1);
            }
        }
    });
});

describe('blits', () => {
    test('fills solid colors and patterns inside the clips', () => {
        const { vram, memory } = machine();
        const dst = surface(VRAM, 64, 16, 8, 32);
        blt(
            memory,
            operation({
                rop: 0xf0,
                dst,
                dstRect: rect(2, 1, 14, 7),
                color: 0xff0000,
                clips: [rect(0, 0, 8, 8), rect(10, 3, 20, 4)],
            }),
        );
        const out = pixels(vram, dst);
        for (let y = 0; y < 8; y++)
            for (let x = 0; x < 16; x++) {
                const inside = (x >= 2 && x < 8 && y >= 1 && y < 7) || (x >= 10 && x < 14 && y === 3);
                assert.equal(out[y * 16 + x], inside ? 0xff0000 : 0, `${x},${y}`);
            }

        const pattern = Uint32Array.from({ length: 64 }, (_, i) => i);
        blt(memory, operation({ rop: 0xf0, dst, dstRect: rect(0, 0, 16, 8), pattern, patternX: 3, patternY: 1 }));
        const patterned = pixels(vram, dst);
        for (let y = 0; y < 8; y++)
            for (let x = 0; x < 16; x++) assert.equal(patterned[y * 16 + x], ((y - 1) & 7) * 8 + ((x - 3) & 7));
    });

    test('combines pattern, source and destination with any ROP in 8, 16 and 32 bpp', () => {
        for (const bpp of [8, 16, 32]) {
            const { vram, guest, memory } = machine();
            fillRandom(vram, bpp);
            fillRandom(guest, bpp + 1);
            const dst = surface(VRAM, 0, 12, 6, bpp);
            const src = surface(GUEST, 0, 12, 6, bpp);
            const before = pixels(vram, dst);
            const source = pixels(guest, src);
            const pattern = Uint32Array.from({ length: 64 }, (_, i) => (i * 0x01030507) >>> 0);
            const rop = 0xb8;
            blt(memory, operation({ rop, dst, dstRect: rect(1, 1, 11, 5), src, srcRect: rect(2, 0, 12, 4), pattern }));
            const after = pixels(vram, dst);
            const max = bpp === 32 ? 0xffffffff : (1 << bpp) - 1;
            for (let y = 0; y < 6; y++)
                for (let x = 0; x < 12; x++) {
                    const i = y * 12 + x;
                    if (x < 1 || x >= 11 || y < 1 || y >= 5) {
                        assert.equal(after[i], before[i]);
                        continue;
                    }
                    const p = pattern[(y & 7) * 8 + (x & 7)];
                    const s = source[(y - 1) * 12 + x + 1];
                    assert.equal(after[i], (rop3(rop, p, s, before[i]) & max) >>> 0, `${bpp} bpp ${x},${y}`);
                }
        }
    });

    test('overlapping copies within a surface read before they write', () => {
        for (const [dx, dy] of [
            [3, 2],
            [-3, -2],
            [4, -1],
            [-2, 3],
            [0, 1],
            [1, 0],
        ]) {
            const { vram, memory } = machine();
            fillRandom(vram, 7);
            const dst = surface(VRAM, 0, 20, 12, 16);
            const before = pixels(vram, dst);
            const area = rect(4, 4, 14, 9);
            blt(memory, operation({ dst, dstRect: area, src: dst, srcRect: rect(4 - dx, 4 - dy, 14 - dx, 9 - dy) }));
            const after = pixels(vram, dst);
            for (let y = 0; y < 12; y++)
                for (let x = 0; x < 20; x++) {
                    const inside = x >= 4 && x < 14 && y >= 4 && y < 9;
                    assert.equal(
                        after[y * 20 + x],
                        inside ? before[(y - dy) * 20 + x - dx] : before[y * 20 + x],
                        `${dx},${dy}`,
                    );
                }
        }
    });

    test('translates 1, 4 and 8 bpp sources through tables and keeps 24 bpp as 32 bpp', () => {
        const { vram, guest, memory } = machine();
        // 1 bpp: alternating columns, rows of 4 bytes
        guest.set([0b10101010, 0, 0, 0, 0b01010101, 0, 0, 0], 0);
        const dst = surface(VRAM, 0, 8, 2, 32);
        blt(
            memory,
            operation({
                dst,
                dstRect: rect(0, 0, 8, 2),
                src: surface(GUEST, 0, 8, 2, 1),
                xlate: Uint32Array.of(0x111111, 0x222222),
            }),
        );
        assert.deepEqual(pixels(vram, dst).slice(0, 4), [0x222222, 0x111111, 0x222222, 0x111111]);
        assert.deepEqual(pixels(vram, dst).slice(8, 12), [0x111111, 0x222222, 0x111111, 0x222222]);

        guest.set([0x12, 0x34, 0, 0], 100);
        const table = Uint32Array.from({ length: 16 }, (_, i) => i * 0x10);
        blt(memory, operation({ dst, dstRect: rect(0, 0, 4, 1), src: surface(GUEST, 100, 4, 1, 4), xlate: table }));
        assert.deepEqual(pixels(vram, dst).slice(0, 4), [0x10, 0x20, 0x30, 0x40]);

        guest.set([5, 200], 200);
        blt(
            memory,
            operation({
                dst,
                dstRect: rect(0, 0, 2, 1),
                src: surface(GUEST, 200, 2, 1, 8),
                xlate: Uint32Array.from({ length: 256 }, (_, i) => i << 8),
            }),
        );
        assert.deepEqual(pixels(vram, dst).slice(0, 2), [0x500, 0xc800]);

        guest.set([1, 2, 3, 4, 5, 6], 300);
        blt(memory, operation({ dst, dstRect: rect(0, 0, 2, 1), src: surface(GUEST, 300, 2, 1, 24) }));
        assert.deepEqual(pixels(vram, dst).slice(0, 2), [0x030201, 0x060504]);
    });

    test('reads bottom up bitmaps through a negative pitch', () => {
        const { vram, guest, memory } = machine();
        // Row 0 is stored last
        new Uint32Array(guest.buffer, 0, 4).set([3, 4, 1, 2]);
        const dst = surface(VRAM, 0, 2, 2, 32);
        blt(memory, operation({ dst, dstRect: rect(0, 0, 2, 2), src: surface(GUEST, 8, 2, 2, 32, -8) }));
        assert.deepEqual(pixels(vram, dst), [1, 2, 3, 4]);
    });

    test('stretches to the nearest pixel and mirrors', () => {
        const { vram, guest, memory } = machine();
        new Uint32Array(guest.buffer, 0, 4).set([1, 2, 3, 4]);
        const src = surface(GUEST, 0, 2, 2, 32);
        const dst = surface(VRAM, 0, 4, 4, 32);
        blt(memory, operation({ dst, dstRect: rect(0, 0, 4, 4), src, srcRect: rect(0, 0, 2, 2) }));
        assert.deepEqual(pixels(vram, dst), [1, 1, 2, 2, 1, 1, 2, 2, 3, 3, 4, 4, 3, 3, 4, 4]);
        blt(
            memory,
            operation({
                dst,
                dstRect: rect(0, 0, 2, 2),
                src,
                srcRect: rect(0, 0, 2, 2),
                mirrorX: true,
                mirrorY: true,
                clips: [rect(0, 0, 2, 2)],
            }),
        );
        assert.deepEqual(pixels(vram, dst).slice(0, 2), [4, 3]);
        assert.deepEqual(pixels(vram, dst).slice(4, 6), [2, 1]);

        // Shrinking keeps every other pixel of a 4x1 row
        new Uint32Array(guest.buffer, 64, 4).set([10, 20, 30, 40]);
        blt(
            memory,
            operation({ dst, dstRect: rect(0, 3, 2, 4), src: surface(GUEST, 64, 4, 1, 32), srcRect: rect(0, 0, 4, 1) }),
        );
        assert.deepEqual(pixels(vram, dst).slice(12, 14), [20, 40]);
    });

    test('skips source and destination color keys', () => {
        const { vram, guest, memory } = machine();
        new Uint32Array(guest.buffer, 0, 4).set([7, 1, 7, 2]);
        new Uint32Array(vram.buffer, 0, 4).set([9, 9, 5, 5]);
        const dst = surface(VRAM, 0, 4, 1, 32);
        blt(memory, operation({ dst, dstRect: rect(0, 0, 4, 1), src: surface(GUEST, 0, 4, 1, 32), srcKey: 7 }));
        assert.deepEqual(pixels(vram, dst), [9, 1, 5, 2]);
        blt(memory, operation({ dst, dstRect: rect(0, 0, 4, 1), rop: 0xf0, color: 3, dstKey: 9 }));
        assert.deepEqual(pixels(vram, dst), [3, 1, 5, 2]);
    });

    test('declines without drawing when guest memory is missing', () => {
        const { vram, memory } = machine();
        const dst = surface(VRAM, 0, 4, 4, 32);
        vram.fill(0xaa);
        assert.throws(() =>
            blt(memory, operation({ dst, dstRect: rect(0, 0, 4, 4), src: surface(GUEST, 0xfff0, 4, 4, 32) })),
        );
        assert.ok(vram.every((byte) => byte === 0xaa));
    });
});

describe('text and alpha', () => {
    test('draws glyph bits inside the clips after the opaque rectangle', () => {
        const { vram, guest, memory } = machine();
        // A 3x2 glyph: 101 / 010
        guest.set([0b10100000, 0b01000000], 0);
        const dst = surface(VRAM, 0, 6, 4, 32);
        text(memory, {
            dst,
            fore: 0xff,
            back: 0x11,
            opaque: rect(0, 0, 6, 3),
            glyphs: [{ x: 1, y: 1, width: 3, height: 2, bits: 0 }],
            clips: [rect(0, 0, 3, 4)],
        });
        assert.deepEqual(
            pixels(vram, dst),
            [0x11, 0x11, 0x11, 0, 0, 0, 0x11, 0xff, 0x11, 0, 0, 0, 0x11, 0x11, 0xff, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        );
    });

    test('blends constant and premultiplied alpha', () => {
        const { vram, guest, memory } = machine();
        const dst = surface(VRAM, 0, 2, 1, 32);
        const src = surface(GUEST, 0, 2, 1, 32);
        new Uint32Array(vram.buffer, 0, 2).set([0x00ffffff, 0x00204080]);
        new Uint32Array(guest.buffer, 0, 2).set([0x80400000, 0xff102030]);
        alpha(memory, {
            dst,
            dstRect: rect(0, 0, 2, 1),
            src,
            srcRect: rect(0, 0, 2, 1),
            alpha: 255,
            perPixel: true,
            clips: [rect(0, 0, 2, 1)],
        });
        const [first, second] = pixels(vram, dst);
        // 0x40 + 0xff * (1 - 0x80 / 255) for red, the destination alone scaled for green and blue
        assert.equal((first >>> 16) & 0xff, 0x40 + Math.round((0xff * (255 - 0x80)) / 255));
        assert.equal(first & 0xff, Math.round((0xff * (255 - 0x80)) / 255));
        assert.equal(second & 0xffffff, 0x102030);

        new Uint32Array(vram.buffer, 0, 1).set([0x000000]);
        new Uint32Array(guest.buffer, 0, 1).set([0x00ff00ff]);
        alpha(memory, {
            dst,
            dstRect: rect(0, 0, 1, 1),
            src,
            srcRect: rect(0, 0, 1, 1),
            alpha: 51,
            perPixel: false,
            clips: [rect(0, 0, 1, 1)],
        });
        assert.equal(pixels(vram, dst)[0] & 0xffffff, 0x330033);
    });
});

describe('device', () => {
    // A machine without paging, so linear addresses are physical
    function cpu(): GpuCpu & { dirty: number } {
        const state = {
            mem8: new Uint8Array(1 << 20),
            cr: new Int32Array(8),
            devices: { vga: { svga_memory: new Uint8Array(1 << 16), svga_height: 768 } },
            dirty: 0,
            svga_mark_dirty() {
                state.dirty++;
            },
        };
        return state;
    }

    const surfaceWords = (s: Surface) => [s.kind, s.address, s.pitch >>> 0, s.bpp, s.width, s.height];
    const rectWords = (r: Rect) => [r.left, r.top, r.right, r.bottom].map((v) => v >>> 0);

    test('decodes a blit like the driver encodes it', () => {
        const machine = cpu();
        const dst = surface(VRAM, 0, 4, 2, 32);
        const src = surface(GUEST, 0x2000, 4, 2, 8);
        machine.mem8.set([0, 1, 2, 3, 3, 2, 1, 0], 0x2000);
        const xlate = [0x10, 0x20, 0x30, 0x40];
        const words = [
            protocol.GPU_OP_BLT,
            protocol.GPU_BLT_SOURCE,
            protocol.ROP_SRCCOPY,
            ...surfaceWords(dst),
            ...surfaceWords(src),
            ...rectWords(rect(0, 0, 4, 2)),
            ...rectWords(rect(0, 0, 4, 2)),
            0,
            0,
            0,
            0,
            0,
            xlate.length,
            1,
            ...xlate,
            ...rectWords(rect(0, 0, 4, 2)),
        ];
        const at = 0x1000;
        new Uint32Array(machine.mem8.buffer, at, words.length).set(words);
        const device = new GpuDevice(machine);
        assert.equal(device.execute(at, words.length * 4), protocol.V86_GPU_DONE);
        assert.deepEqual(
            pixels(machine.devices.vga.svga_memory, dst),
            [0x10, 0x20, 0x30, 0x40, 0x40, 0x30, 0x20, 0x10],
        );
        assert.equal(machine.dirty, 1);

        // A source the page tables don't reach is declined
        words[3 + 6 + 1] = 0x200000;
        new Uint32Array(machine.mem8.buffer, at, words.length).set(words);
        assert.equal(device.execute(at, words.length * 4), protocol.V86_GPU_PUNT);
        assert.equal(device.execute(at, 6), protocol.V86_GPU_INVALID);
    });

    test('keeps a 60 Hz timeline with the blank after the visible lines', () => {
        const device = new GpuDevice(cpu());
        const frame = 1000 / 60;
        assert.equal(device.scanline(0), 0);
        const middle = device.scanline(frame / 2);
        assert.ok(middle > 300 && middle < 768);
        assert.ok(device.scanline(frame * 0.99) & protocol.V86_GPU_VBLANK);
    });
});

describe('guest memory', () => {
    test('walks 32-bit and PAE page tables', () => {
        const mem8 = new Uint8Array(1 << 22);
        const view = new DataView(mem8.buffer);
        const cr = new Int32Array(8);
        // 32-bit: directory at 0x1000, table at 0x2000, linear 0x00403000 to physical 0x5000
        cr[0] = 0x80000001;
        cr[3] = 0x1000;
        view.setUint32(0x1000 + 1 * 4, 0x2000 | 1, true);
        view.setUint32(0x2000 + 3 * 4, 0x5000 | 1, true);
        mem8.set([1, 2, 3], 0x5ffe);
        assert.equal(translate({ mem8, cr }, 0x00403123), 0x5123);
        assert.equal(translate({ mem8, cr }, 0x00404000), -1);
        // Reads stop at pages that are not present
        assert.equal(readLinear({ mem8, cr }, 0x00403ffe, 4), null);

        // PAE: pointer table at 0x8000, directory at 0x9000, a 2 MiB page
        cr[4] = 0x20;
        cr[3] = 0x8000;
        view.setUint32(0x8000, 0x9000 | 1, true);
        view.setUint32(0x9000 + 2 * 8, 0x200000 | 0x81, true);
        assert.equal(translate({ mem8, cr }, 0x00412345), 0x212345);
    });
});
