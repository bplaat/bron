/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// The 2D engine of the display driver's commands, drawing into the VRAM: blits with every ROP3, patterns,
// translation tables, stretching, mirroring and color keys, monochrome glyphs and alpha blending. Every operation
// first reads what it needs and checks its surfaces, so a declined command leaves the VRAM as it was.

import { GPU_SURFACE_GUEST, GPU_SURFACE_VRAM } from './protocol.ts';

export interface Surface {
    kind: number;
    address: number;
    pitch: number;
    bpp: number;
    width: number;
    height: number;
}

export interface Rect {
    left: number;
    top: number;
    right: number;
    bottom: number;
}

// Where the engine draws and reads: the VRAM, and guest memory at linear addresses
export interface Memory {
    vram: Uint8Array;
    read(address: number, length: number, out: Uint8Array): boolean;
}

export interface Blt {
    rop: number;
    dst: Surface;
    dstRect: Rect;
    // Without a source the ROP may not read one
    src: Surface | null;
    srcRect: Rect;
    // The solid brush, or the 8x8 pattern at its origin
    color: number;
    pattern: Uint32Array | null;
    patternX: number;
    patternY: number;
    // Source index to destination pixel, null when the pixels stay as they are
    xlate: Uint32Array | null;
    srcKey: number | null;
    dstKey: number | null;
    mirrorX: boolean;
    mirrorY: boolean;
    clips: Rect[];
}

export interface Glyph {
    x: number;
    y: number;
    width: number;
    height: number;
    // Linear address of the rows, one bit per pixel, rows padded to bytes
    bits: number;
}

export interface Text {
    dst: Surface;
    fore: number;
    back: number;
    opaque: Rect | null;
    glyphs: Glyph[];
    clips: Rect[];
}

export interface Alpha {
    dst: Surface;
    dstRect: Rect;
    src: Surface;
    srcRect: Rect;
    alpha: number;
    perPixel: boolean;
    clips: Rect[];
}

// The page can't do it (memory not present, a format it doesn't draw): the driver lets the engine draw instead
export class Punt extends Error {}
// The command is malformed
export class Invalid extends Error {}

const intersect = (a: Rect, b: Rect): Rect => ({
    left: Math.max(a.left, b.left),
    top: Math.max(a.top, b.top),
    right: Math.min(a.right, b.right),
    bottom: Math.min(a.bottom, b.bottom),
});
const isEmpty = (r: Rect) => r.left >= r.right || r.top >= r.bottom;
const bounds = (s: Surface): Rect => ({ left: 0, top: 0, right: s.width, bottom: s.height });

const MAX_SIZE = 8192;
const mask = (bpp: number) => (bpp === 32 ? 0xffffffff : (1 << bpp) - 1);

// A VRAM surface the engine writes, as pixel indices into a typed view of the VRAM
class Target {
    pixels: Uint8Array | Uint16Array | Uint32Array;
    base: number;
    stride: number;
    mask: number;

    constructor(memory: Memory, s: Surface) {
        const bytes = s.bpp / 8;
        if (s.kind !== GPU_SURFACE_VRAM || ![8, 16, 32].includes(s.bpp)) throw new Invalid('target');
        if (s.width > MAX_SIZE || s.height > MAX_SIZE || s.pitch < s.width * bytes) throw new Invalid('target size');
        if (s.address % bytes || s.pitch % bytes) throw new Invalid('target alignment');
        if (s.height && s.address + (s.height - 1) * s.pitch + s.width * bytes > memory.vram.length)
            throw new Invalid('target outside VRAM');
        const { buffer, byteOffset, byteLength } = memory.vram;
        if (byteOffset % 4) throw new Punt('VRAM alignment');
        this.pixels =
            s.bpp === 32
                ? new Uint32Array(buffer, byteOffset, byteLength >> 2)
                : s.bpp === 16
                  ? new Uint16Array(buffer, byteOffset, byteLength >> 1)
                  : memory.vram;
        this.base = s.address / bytes;
        this.stride = s.pitch / bytes;
        this.mask = mask(s.bpp);
    }

    index(x: number, y: number) {
        return this.base + y * this.stride + x;
    }
}

// A rectangle of a source surface, copied out before anything is drawn: guest pages can be missing, and a copy
// within one surface must read what was there before
class Source {
    surface: Surface;
    rect: Rect;
    data: Uint8Array;
    rowBytes: number;
    firstByte: number;

    constructor(memory: Memory, surface: Surface, rect: Rect) {
        const s = surface;
        this.surface = surface;
        this.rect = rect;
        if (![1, 4, 8, 16, 24, 32].includes(s.bpp) || s.width > MAX_SIZE || s.height > MAX_SIZE)
            throw new Invalid('source');
        if (rect.left < 0 || rect.top < 0 || rect.right > s.width || rect.bottom > s.height) throw new Punt('source');
        this.firstByte = Math.floor((rect.left * s.bpp) / 8);
        this.rowBytes = Math.ceil((rect.right * s.bpp) / 8) - this.firstByte;
        const rows = rect.bottom - rect.top;
        this.data = new Uint8Array(this.rowBytes * rows);
        for (let row = 0; row < rows; row++) {
            const at = s.address + (rect.top + row) * s.pitch + this.firstByte;
            const out = this.data.subarray(row * this.rowBytes, (row + 1) * this.rowBytes);
            if (s.kind === GPU_SURFACE_VRAM) {
                if (s.pitch <= 0 || at < 0 || at + this.rowBytes > memory.vram.length) throw new Invalid('source VRAM');
                out.set(memory.vram.subarray(at, at + this.rowBytes));
            } else if (s.kind === GPU_SURFACE_GUEST) {
                if (!memory.read(at >>> 0, this.rowBytes, out)) throw new Punt('source memory');
            } else {
                throw new Invalid('source kind');
            }
        }
    }

    // The bytes of row y from pixel x, for byte aligned formats
    bytes(y: number, x: number, count: number) {
        const bytes = this.surface.bpp / 8;
        const start = (y - this.rect.top) * this.rowBytes + x * bytes - this.firstByte;
        return this.data.subarray(start, start + count * bytes);
    }

    // The raw pixels of row y from x, as stored in the surface
    row(y: number, x: number, count: number, out: Uint32Array) {
        const { bpp } = this.surface;
        const data = this.data;
        const start = (y - this.rect.top) * this.rowBytes;
        const bit = x * bpp - this.firstByte * 8;
        for (let i = 0; i < count; i++) {
            const at = start + ((bit + i * bpp) >> 3);
            switch (bpp) {
                case 1:
                    out[i] = (data[at] >> (7 - ((bit + i) & 7))) & 1;
                    break;
                case 4:
                    out[i] = ((bit + i * 4) & 4 ? data[at] : data[at] >> 4) & 0xf;
                    break;
                case 8:
                    out[i] = data[at];
                    break;
                case 16:
                    out[i] = data[at] | (data[at + 1] << 8);
                    break;
                case 24:
                    out[i] = data[at] | (data[at + 1] << 8) | (data[at + 2] << 16);
                    break;
                default:
                    out[i] = (data[at] | (data[at + 1] << 8) | (data[at + 2] << 16) | (data[at + 3] << 24)) >>> 0;
            }
        }
    }
}

// The ROP3 of pattern, source and destination: bit (p << 2 | s << 1 | d) of the ROP is the result
export function rop3(rop: number, p: number, s: number, d: number) {
    let r = 0;
    if (rop & 0x01) r |= ~p & ~s & ~d;
    if (rop & 0x02) r |= ~p & ~s & d;
    if (rop & 0x04) r |= ~p & s & ~d;
    if (rop & 0x08) r |= ~p & s & d;
    if (rop & 0x10) r |= p & ~s & ~d;
    if (rop & 0x20) r |= p & ~s & d;
    if (rop & 0x40) r |= p & s & ~d;
    if (rop & 0x80) r |= p & s & d;
    return r >>> 0;
}

type Combine = (p: number, s: number, d: number) => number;
const COMMON: Record<number, Combine> = {
    0x00: () => 0,
    0x33: (_p, s) => ~s,
    0x55: (_p, _s, d) => ~d,
    0x5a: (p, _s, d) => p ^ d,
    0x66: (_p, s, d) => s ^ d,
    0x88: (_p, s, d) => s & d,
    0xcc: (_p, s) => s,
    0xee: (_p, s, d) => s | d,
    0xf0: (p) => p,
    0xff: () => -1,
};
export const combiner = (rop: number): Combine => COMMON[rop] ?? ((p, s, d) => rop3(rop, p, s, d));

export const usesSource = (rop: number) => (((rop >> 2) ^ rop) & 0x33) !== 0;
export const usesPattern = (rop: number) => (((rop >> 4) ^ rop) & 0x0f) !== 0;

// The source column or row of each destination one: the nearest pixel by its center, mirrored if asked
function sampling(dstSize: number, srcSize: number, mirror: boolean) {
    const map = new Int32Array(dstSize);
    for (let i = 0; i < dstSize; i++) {
        const at = mirror ? dstSize - 1 - i : i;
        map[i] = Math.floor(((2 * at + 1) * srcSize) / (2 * dstSize));
    }
    return map;
}

// The clip rectangles within the destination rectangle and surface
function clipsWithin(clips: Rect[], dstRect: Rect, dst: Surface) {
    const area = intersect(dstRect, bounds(dst));
    return clips.map((clip) => intersect(clip, area)).filter((clip) => !isEmpty(clip));
}

export function blt(memory: Memory, op: Blt) {
    const target = new Target(memory, op.dst);
    const { dstRect, srcRect } = op;
    const dstW = dstRect.right - dstRect.left;
    const dstH = dstRect.bottom - dstRect.top;
    if (dstW <= 0 || dstH <= 0) return;
    const clips = clipsWithin(op.clips, dstRect, op.dst);
    if (!clips.length) return;
    if (usesPattern(op.rop) && op.pattern && op.pattern.length < 64) throw new Invalid('pattern');

    // The source pixels the clips reach, read first
    let source: Source | null = null;
    let xs: Int32Array | null = null;
    let ys: Int32Array | null = null;
    if (usesSource(op.rop)) {
        if (!op.src) throw new Invalid('source missing');
        const srcW = srcRect.right - srcRect.left;
        const srcH = srcRect.bottom - srcRect.top;
        if (srcW <= 0 || srcH <= 0) return;
        if (!op.xlate && op.src.bpp !== op.dst.bpp && !(op.src.bpp === 24 && op.dst.bpp === 32))
            throw new Punt('translation');
        if (op.xlate && op.src.bpp > 8) throw new Invalid('table translation');
        xs = sampling(dstW, srcW, op.mirrorX);
        ys = sampling(dstH, srcH, op.mirrorY);
        let [left, right, top, bottom] = [Infinity, -Infinity, Infinity, -Infinity];
        for (const clip of clips) {
            for (let x = clip.left; x < clip.right; x++) {
                left = Math.min(left, xs[x - dstRect.left]);
                right = Math.max(right, xs[x - dstRect.left] + 1);
            }
            for (let y = clip.top; y < clip.bottom; y++) {
                top = Math.min(top, ys[y - dstRect.top]);
                bottom = Math.max(bottom, ys[y - dstRect.top] + 1);
            }
        }
        source = new Source(memory, op.src, {
            left: srcRect.left + left,
            top: srcRect.top + top,
            right: srcRect.left + right,
            bottom: srcRect.top + bottom,
        });
    }

    const { pixels } = target;
    const fill = target.mask;
    const combine = combiner(op.rop);
    const xlate = op.xlate;
    const plain = op.srcKey === null && op.dstKey === null;
    const [patternX, patternY] = [op.patternX & 7, op.patternY & 7];
    const row = new Uint32Array(source ? source.rect.right - source.rect.left : 0);
    // A copy without stretching, translation or keys moves the bytes
    const bytes = op.dst.bpp / 8;
    const copy =
        source &&
        plain &&
        op.rop === 0xcc &&
        !xlate &&
        op.src?.bpp === op.dst.bpp &&
        !op.mirrorX &&
        !op.mirrorY &&
        dstW === srcRect.right - srcRect.left &&
        dstH === srcRect.bottom - srcRect.top;

    for (const clip of clips) {
        const width = clip.right - clip.left;
        if (!source && plain && op.rop === 0xf0 && !op.pattern) {
            // A solid fill
            for (let y = clip.top; y < clip.bottom; y++) {
                const at = target.index(clip.left, y);
                pixels.fill(op.color & fill, at, at + width);
            }
            continue;
        }
        if (copy && source) {
            const dx = srcRect.left - dstRect.left;
            const dy = srcRect.top - dstRect.top;
            for (let y = clip.top; y < clip.bottom; y++) {
                memory.vram.set(source.bytes(y + dy, clip.left + dx, width), target.index(clip.left, y) * bytes);
            }
            continue;
        }
        let decoded = -1;
        for (let y = clip.top; y < clip.bottom; y++) {
            if (source && ys) {
                const sy = srcRect.top + ys[y - dstRect.top];
                if (sy !== decoded) source.row(sy, source.rect.left, row.length, row);
                decoded = sy;
            }
            const patternRow = op.pattern ? ((y - patternY) & 7) * 8 : 0;
            let at = target.index(clip.left, y);
            for (let x = clip.left; x < clip.right; x++, at++) {
                const d = pixels[at];
                if (op.dstKey !== null && d !== op.dstKey) continue;
                let s = 0;
                if (source && xs) {
                    s = row[srcRect.left + xs[x - dstRect.left] - source.rect.left];
                    if (op.srcKey !== null && s === op.srcKey) continue;
                    if (xlate) s = xlate[s] ?? 0;
                }
                const p = op.pattern ? op.pattern[patternRow + ((x - patternX) & 7)] : op.color;
                pixels[at] = combine(p, s, d) & fill;
            }
        }
    }
}

// A glyph's rows, read before anything is drawn
interface GlyphBits {
    glyph: Glyph;
    stride: number;
    data: Uint8Array;
}

export function text(memory: Memory, op: Text) {
    const target = new Target(memory, op.dst);
    const clips = clipsWithin(op.clips, bounds(op.dst), op.dst);
    const glyphs: GlyphBits[] = op.glyphs.map((glyph) => {
        if (glyph.width < 0 || glyph.height < 0 || glyph.width > MAX_SIZE || glyph.height > MAX_SIZE)
            throw new Invalid('glyph');
        const stride = Math.ceil(glyph.width / 8);
        const data = new Uint8Array(stride * glyph.height);
        if (data.length && !memory.read(glyph.bits >>> 0, data.length, data)) throw new Punt('glyph memory');
        return { glyph, stride, data };
    });

    const { pixels } = target;
    if (op.opaque) {
        const back = op.back & target.mask;
        for (const clip of clips) {
            const area = intersect(clip, op.opaque);
            if (isEmpty(area)) continue;
            for (let y = area.top; y < area.bottom; y++) {
                const at = target.index(area.left, y);
                pixels.fill(back, at, at + area.right - area.left);
            }
        }
    }
    const fore = op.fore & target.mask;
    for (const { glyph, stride, data } of glyphs) {
        const box = { left: glyph.x, top: glyph.y, right: glyph.x + glyph.width, bottom: glyph.y + glyph.height };
        for (const clip of clips) {
            const area = intersect(clip, box);
            if (isEmpty(area)) continue;
            for (let y = area.top; y < area.bottom; y++) {
                const row = (y - glyph.y) * stride;
                let at = target.index(area.left, y);
                for (let x = area.left; x < area.right; x++, at++) {
                    const bit = x - glyph.x;
                    if ((data[row + (bit >> 3)] >> (7 - (bit & 7))) & 1) pixels[at] = fore;
                }
            }
        }
    }
}

// x / 255, rounded, for products of two bytes
const div255 = (x: number) => (x + 128 + ((x + 128) >> 8)) >> 8;

export function alpha(memory: Memory, op: Alpha) {
    if (op.dst.bpp !== 32 || op.src.bpp !== 32) throw new Punt('alpha format');
    const target = new Target(memory, op.dst);
    const { dstRect, srcRect } = op;
    const width = dstRect.right - dstRect.left;
    const height = dstRect.bottom - dstRect.top;
    if (width !== srcRect.right - srcRect.left || height !== srcRect.bottom - srcRect.top)
        throw new Punt('alpha stretch');
    if (width <= 0 || height <= 0) return;
    const clips = clipsWithin(op.clips, dstRect, op.dst);
    if (!clips.length) return;
    const [dx, dy] = [srcRect.left - dstRect.left, srcRect.top - dstRect.top];
    let area = clips[0];
    for (const clip of clips)
        area = {
            left: Math.min(area.left, clip.left),
            top: Math.min(area.top, clip.top),
            right: Math.max(area.right, clip.right),
            bottom: Math.max(area.bottom, clip.bottom),
        };
    const source = new Source(memory, op.src, {
        left: area.left + dx,
        top: area.top + dy,
        right: area.right + dx,
        bottom: area.bottom + dy,
    });

    const pixels = target.pixels as Uint32Array;
    const constant = op.alpha & 0xff;
    const row = new Uint32Array(area.right - area.left);
    for (const clip of clips) {
        for (let y = clip.top; y < clip.bottom; y++) {
            const count = clip.right - clip.left;
            source.row(y + dy, clip.left + dx, count, row);
            let at = target.index(clip.left, y);
            for (let i = 0; i < count; i++, at++) {
                let s = row[i];
                const d = pixels[at];
                if (op.perPixel) {
                    // Premultiplied source over the destination, the constant alpha scales the source first
                    if (constant !== 255) {
                        s =
                            (div255((s & 0xff) * constant) |
                                (div255(((s >>> 8) & 0xff) * constant) << 8) |
                                (div255(((s >>> 16) & 0xff) * constant) << 16) |
                                (div255((s >>> 24) * constant) << 24)) >>>
                            0;
                    }
                    const inverse = 255 - (s >>> 24);
                    let out = 0;
                    for (let shift = 0; shift < 32; shift += 8) {
                        const channel = ((s >>> shift) & 0xff) + div255(((d >>> shift) & 0xff) * inverse);
                        out |= Math.min(channel, 255) << shift;
                    }
                    pixels[at] = out >>> 0;
                } else {
                    let out = 0;
                    for (let shift = 0; shift < 32; shift += 8) {
                        const channel = div255(
                            ((s >>> shift) & 0xff) * constant + ((d >>> shift) & 0xff) * (255 - constant),
                        );
                        out |= channel << shift;
                    }
                    pixels[at] = out >>> 0;
                }
            }
        }
    }
}
