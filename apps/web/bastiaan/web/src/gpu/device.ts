/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// The host side of the paravirtualized graphics adapter: executes the display driver's 2D
// commands on the VRAM while the guest waits in its port access, and keeps the 60 Hz timeline of DirectDraw's
// vertical blanks.

import { readInto, type GuestCpu } from '../guest-memory.ts';
import { alpha, blt, Invalid, Punt, text, type Glyph, type Memory, type Rect, type Surface } from './blit.ts';
import {
    ALPHA_HEADER_WORDS,
    BLT_HEADER_WORDS,
    GLYPH_WORDS,
    GPU_ALPHA_PER_PIXEL,
    GPU_BLT_DEST_KEY,
    GPU_BLT_MIRROR_X,
    GPU_BLT_MIRROR_Y,
    GPU_BLT_PATTERN,
    GPU_BLT_SOURCE,
    GPU_BLT_SOURCE_KEY,
    GPU_OP_ALPHA,
    GPU_OP_BLT,
    GPU_OP_TEXT,
    GPU_TEXT_OPAQUE,
    PATTERN_PIXELS,
    RECT_WORDS,
    TEXT_HEADER_WORDS,
    V86_GPU_DONE,
    V86_GPU_INVALID,
    V86_GPU_PUNT,
    V86_GPU_VBLANK,
    XLATE_MAX,
} from './protocol.ts';

// The parts of v86 the device uses
export interface GpuCpu extends GuestCpu {
    devices: { vga: { svga_memory: Uint8Array; svga_height: number } };
    svga_mark_dirty(): void;
}

const MAX_COMMAND = 64 * 1024;
const FRAME_MS = 1000 / 60;
// Lines of the vertical blank after the visible ones, like the 45 of a 768 line mode
const BLANK_SHARE = 0.06;

// Reads the words of a command one field at a time
class Reader {
    words: Uint32Array;
    at = 0;

    constructor(words: Uint32Array) {
        this.words = words;
    }

    word() {
        if (this.at >= this.words.length) throw new Invalid('command too short');
        return this.words[this.at++];
    }

    int() {
        return this.word() | 0;
    }

    surface(): Surface {
        return {
            kind: this.word(),
            address: this.word(),
            pitch: this.int(),
            bpp: this.word(),
            width: this.word(),
            height: this.word(),
        };
    }

    rect(): Rect {
        return { left: this.int(), top: this.int(), right: this.int(), bottom: this.int() };
    }

    words32(count: number) {
        if (this.at + count > this.words.length) throw new Invalid('command too short');
        const out = this.words.slice(this.at, this.at + count);
        this.at += count;
        return out;
    }

    rects(count: number) {
        return Array.from({ length: count }, () => this.rect());
    }
}

// Decodes and draws one command
export function execute(memory: Memory, words: Uint32Array) {
    const read = new Reader(words);
    const op = read.word();
    if (op === GPU_OP_BLT) {
        if (words.length < BLT_HEADER_WORDS) throw new Invalid('blit header');
        const flags = read.word();
        const rop = read.word() & 0xff;
        const dst = read.surface();
        const src = read.surface();
        const dstRect = read.rect();
        const srcRect = read.rect();
        const [color, srcKey, dstKey, patternX, patternY] = [
            read.word(),
            read.word(),
            read.word(),
            read.int(),
            read.int(),
        ];
        const xlateCount = read.word();
        const clipCount = read.word();
        if (xlateCount > XLATE_MAX) throw new Invalid('translation table');
        const pattern = flags & GPU_BLT_PATTERN ? read.words32(PATTERN_PIXELS) : null;
        const xlate = xlateCount ? read.words32(xlateCount) : null;
        blt(memory, {
            rop,
            dst,
            dstRect,
            src: flags & GPU_BLT_SOURCE ? src : null,
            srcRect,
            color,
            pattern,
            patternX,
            patternY,
            xlate,
            srcKey: flags & GPU_BLT_SOURCE_KEY ? srcKey : null,
            dstKey: flags & GPU_BLT_DEST_KEY ? dstKey : null,
            mirrorX: (flags & GPU_BLT_MIRROR_X) !== 0,
            mirrorY: (flags & GPU_BLT_MIRROR_Y) !== 0,
            clips: read.rects(clipCount),
        });
    } else if (op === GPU_OP_TEXT) {
        if (words.length < TEXT_HEADER_WORDS) throw new Invalid('text header');
        const flags = read.word();
        const dst = read.surface();
        const [fore, back] = [read.word(), read.word()];
        const opaque = read.rect();
        const [glyphCount, clipCount] = [read.word(), read.word()];
        if (read.at + glyphCount * GLYPH_WORDS + clipCount * RECT_WORDS > words.length)
            throw new Invalid('text arrays');
        const glyphs: Glyph[] = Array.from({ length: glyphCount }, () => ({
            x: read.int(),
            y: read.int(),
            width: read.int(),
            height: read.int(),
            bits: read.word(),
        }));
        text(memory, {
            dst,
            fore,
            back,
            opaque: flags & GPU_TEXT_OPAQUE ? opaque : null,
            glyphs,
            clips: read.rects(clipCount),
        });
    } else if (op === GPU_OP_ALPHA) {
        if (words.length < ALPHA_HEADER_WORDS) throw new Invalid('alpha header');
        const flags = read.word();
        const constant = read.word();
        const dst = read.surface();
        const src = read.surface();
        const dstRect = read.rect();
        const srcRect = read.rect();
        alpha(memory, {
            dst,
            dstRect,
            src,
            srcRect,
            alpha: constant,
            perPixel: (flags & GPU_ALPHA_PER_PIXEL) !== 0,
            clips: read.rects(read.word()),
        });
    } else {
        throw new Invalid(`unknown operation ${op}`);
    }
}

export class GpuDevice {
    cpu: GpuCpu;
    commands = 0;
    declined = 0;

    constructor(cpu: GpuCpu) {
        this.cpu = cpu;
    }

    memory(): Memory {
        const cpu = this.cpu;
        return {
            vram: cpu.devices.vga.svga_memory,
            read: (address, length, out) => readInto(cpu, address, length, out),
        };
    }

    // The V86_CMD_GPU backdoor command: the command at a linear address, its length in bytes
    execute(address: number, length: number) {
        if (length < 4 || length > MAX_COMMAND || length % 4) return V86_GPU_INVALID;
        const bytes = new Uint8Array(length);
        if (!readInto(this.cpu, address, length, bytes)) return V86_GPU_PUNT;
        this.commands++;
        try {
            execute(this.memory(), new Uint32Array(bytes.buffer));
            this.cpu.svga_mark_dirty();
            return V86_GPU_DONE;
        } catch (error) {
            this.declined++;
            if (error instanceof Punt) return V86_GPU_PUNT;
            if (error instanceof Invalid) {
                console.debug('[winxp] invalid display command:', error.message);
                return V86_GPU_INVALID;
            }
            throw error;
        }
    }

    // The V86_CMD_GPU_SCANLINE backdoor command: where a 60 Hz display would be now
    scanline(now = performance.now()) {
        const height = this.cpu.devices.vga.svga_height || 768;
        const total = height + Math.max(1, Math.round(height * BLANK_SHARE));
        const line = Math.floor(((now % FRAME_MS) / FRAME_MS) * total);
        return (line >= height ? V86_GPU_VBLANK | line : line) | 0;
    }
}
