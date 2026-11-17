// Host side of the v86 guest integration, see winxp-image/guest/v86.h
// Extends v86's VMware backdoor with commands the guest agent and drivers use.

const VMWARE_PORT = 0x5658;
const VMWARE_MAGIC = 0x564d5868;
const VMWARE_CMD_ABSPOINTER_DATA = 39;
const VMWARE_RELATIVE_PACKET = 0x10000;
const REG_EAX = 0;
const REG_ECX = 1;
const REG_EBX = 3;
const REG_ESI = 6;
const REG_EDI = 7;

const CMD_HELLO = 0x8600;
const CMD_GETSIZE = 0x8601;
const CMD_RECV = 0x8602;
const CMD_SEND = 0x8603;
const CMD_POINTER_SHAPE = 0x8604;
const CMD_POINTER_MOVE = 0x8605;
const POINTER_HIDDEN = 0xffffffff;
const POINTER_RESEND = 1;
const POINTER_ATTRIBUTES = 24; // VIDEO_POINTER_ATTRIBUTES before its pixels
const POINTER_SIZE = 32; // v86vid.sys offers monochrome pointers up to 32x32, 4 bytes per mask row
const POINTER_STRIDE = POINTER_SIZE / 8;
const HELLO_REPLY = 0x86868686 | 0;

const MSG_CLIP_FORMAT = 1;
const MSG_FILE_BEGIN = 2;
const MSG_FILE_DATA = 3;
const MSG_FILE_END = 4;
const MSG_LOG = 5;
const MSG_SHUTDOWN = 6;
const MSG_CLIP_DATA = 7;
const MSG_CLIP_END = 8;
const MSG_OPEN_URL = 9;
const MSG_NET_RENEW = 10;
const CHUNKED = new Set([MSG_FILE_DATA, MSG_CLIP_DATA]);

const CLIP_TEXT = 0;
const CLIP_HTML = 1;
const CLIP_IMAGE = 2;
const CLIP_MAX = 64 * 1024 * 1024;

function utf16(text) {
    const out = new Uint8Array(text.length * 2);
    for (let i = 0; i < text.length; i++) {
        const c = text.charCodeAt(i);
        out[i * 2] = c & 0xff;
        out[i * 2 + 1] = c >> 8;
    }
    return out;
}

function fromUtf16(bytes) {
    return new TextDecoder('utf-16le').decode(bytes);
}

function uint32s(...values) {
    const out = new Uint8Array(values.length * 4);
    const view = new DataView(out.buffer);
    values.forEach((value, i) => view.setUint32(i * 4, value, true));
    return out;
}

// RGBA <-> BGRX, the guest has no alpha
function swapRedBlue(src, dst, alpha) {
    for (let i = 0; i < dst.length; i += 4) {
        dst[i] = src[i + 2];
        dst[i + 1] = src[i + 1];
        dst[i + 2] = src[i];
        dst[i + 3] = alpha;
    }
    return dst;
}

function encodeImage({ width, height, data }) {
    const out = new Uint8Array(8 + width * height * 4);
    out.set(uint32s(width, height));
    swapRedBlue(data, out.subarray(8), 0);
    return out;
}

function decodeImage(bytes) {
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    if (bytes.length < 8) return null;
    const width = view.getUint32(0, true);
    const height = view.getUint32(4, true);
    if (!width || !height || bytes.length < 8 + width * height * 4) return null;
    return { width, height, data: swapRedBlue(bytes.subarray(8), new Uint8ClampedArray(width * height * 4), 255) };
}

// Guest memory at a linear address, through the page tables of the running code: null when a page is not present
const u32 = (mem, address) =>
    (mem[address] | (mem[address + 1] << 8) | (mem[address + 2] << 16) | (mem[address + 3] << 24)) >>> 0;
function translate(cpu, linear) {
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

function readLinear(cpu, address, length) {
    const out = new Uint8Array(length);
    for (let done = 0; done < length; ) {
        const linear = (address + done) >>> 0;
        const physical = translate(cpu, linear);
        const count = Math.min(length - done, 0x1000 - (linear & 0xfff));
        if (physical < 0 || physical + count > cpu.mem8.length) return null;
        out.set(cpu.mem8.subarray(physical, physical + count), done);
        done += count;
    }
    return out;
}

// A monochrome pointer (VIDEO_POINTER_ATTRIBUTES) as RGBA, null when it inverts the screen, which a browser cursor
// cannot
function decodePointer(bytes) {
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const [width, height, stride] = [view.getUint32(4, true), view.getUint32(8, true), view.getUint32(12, true)];
    if (width > POINTER_SIZE || height > POINTER_SIZE || stride !== POINTER_STRIDE) return null;
    const xorStart = POINTER_ATTRIBUTES + stride * height;
    const data = new Uint8ClampedArray(width * height * 4);
    for (let y = 0; y < height; y++) {
        for (let x = 0; x < width; x++) {
            const bit = 0x80 >> (x & 7);
            const and = bytes[POINTER_ATTRIBUTES + y * stride + (x >> 3)] & bit;
            const xor = bytes[xorStart + y * stride + (x >> 3)] & bit;
            if (and && xor) return null;
            if (!and) data.set(xor ? [255, 255, 255, 255] : [0, 0, 0, 255], (y * width + x) * 4);
        }
    }
    return { width, height, data, key: bytes.subarray(4, xorStart + stride * height).join() };
}

export class GuestHost extends EventTarget {
    constructor(emulator) {
        super();
        this.emulator = emulator;
        this.queue = [];
        this.clip = [];
        this.clipCurrent = null;
        this.size = 0;
        this.agentSeen = -Infinity;
        // Hardware pointer: the shape the guest set (undefined until it does), its top left and the hotspot per shape
        this.pointerShape = undefined;
        this.pointerAt = null;
        this.pointerHot = new Map();
        this.pointerShown = null;
        this.mouse = null; // the last absolute position the guest read, 0 to 0xffff
        const cpu = emulator.v86.cpu;
        const vmware = cpu.devices.vmware;
        const stock = vmware.port_read32;
        this.cpu = cpu;
        this.vmware = vmware;
        cpu.io.register_read(
            VMWARE_PORT,
            vmware,
            () => 0xff,
            () => 0xffff,
            () => {
                const reg = cpu.reg32;
                if (reg[REG_EAX] !== VMWARE_MAGIC) return stock.call(vmware);
                switch (reg[REG_ECX] & 0xffff) {
                    case VMWARE_CMD_ABSPOINTER_DATA: {
                        const full = reg[REG_EBX] === 4;
                        const status = stock.call(vmware);
                        if (full && !(status & VMWARE_RELATIVE_PACKET)) this.mouse = [reg[REG_EBX], reg[REG_ECX]];
                        return status;
                    }
                    case CMD_POINTER_SHAPE:
                        return this.setPointerShape(
                            readLinear(cpu, reg[REG_EBX] >>> 0, POINTER_ATTRIBUTES + 2 * POINTER_STRIDE * POINTER_SIZE),
                        );
                    case CMD_POINTER_MOVE:
                        return this.movePointer(reg[REG_EBX] >>> 0);
                    case CMD_HELLO:
                        return HELLO_REPLY;
                    case CMD_GETSIZE:
                        return this.size;
                    case CMD_RECV:
                        this.agentSeen = performance.now();
                        return this.recv(cpu, reg);
                    case CMD_SEND:
                        this.receive(
                            reg[REG_EDI] >>> 0,
                            cpu.mem8.slice(reg[REG_EBX] >>> 0, (reg[REG_EBX] >>> 0) + (reg[REG_ESI] >>> 0)),
                        );
                        return 0;
                }
                return stock.call(vmware);
            },
        );
    }

    setPointerShape(bytes) {
        this.pointerShape = bytes && decodePointer(bytes);
        this.pointerAt = null;
        this.updatePointer();
        return this.pointerShape ? 1 : 0;
    }

    movePointer(position) {
        if (this.pointerShape === undefined) return POINTER_RESEND;
        this.pointerAt = position === POINTER_HIDDEN ? null : [position >> 16, (position << 16) >> 16];
        this.learnHotspot();
        this.updatePointer();
        return 0;
    }

    // The guest only tells where the pointer's top left is: the hotspot is where the guest's mouse is in the shape,
    // learned once the guest has read every queued mouse packet. Windows maps 0 to 0xffff onto the screen.
    learnHotspot() {
        const shape = this.pointerShape;
        if (!shape || !this.pointerAt || !this.mouse || this.vmware.queue.length) return;
        const vga = this.cpu.devices.vga;
        const x = Math.floor((this.mouse[0] * vga.svga_width) / 0x10000) - this.pointerAt[0];
        const y = Math.floor((this.mouse[1] * vga.svga_height) / 0x10000) - this.pointerAt[1];
        if (x >= 0 && y >= 0 && x < shape.width && y < shape.height) this.pointerHot.set(shape.key, [x, y]);
    }

    // Fires 'pointer' with { width, height, data, key, x, y } (x, y the hotspot) or null when Windows draws it itself
    updatePointer() {
        const shape = this.pointerShape;
        const [x, y] = (shape && this.pointerHot.get(shape.key)) ?? [0, 0];
        const shown = shape && this.pointerAt ? { ...shape, x, y } : null;
        const old = this.pointerShown;
        if (shown?.key === old?.key && shown?.x === old?.x && shown?.y === old?.y) return;
        this.pointerShown = shown;
        this.dispatchEvent(new CustomEvent('pointer', { detail: shown }));
    }

    get agentAlive() {
        return performance.now() - this.agentSeen < 2000;
    }

    setSize(width, height) {
        this.size = ((width & 0xffff) << 16) | (height & 0xffff);
    }

    push(type, data = new Uint8Array(0), extra = {}) {
        this.queue.push({ type, data, offset: 0, ...extra });
    }

    // Replaces the guest clipboard with { text, html, image: ImageData }, resolves once the guest has it
    setClipboard({ text, html, image }) {
        const formats = [];
        if (text) formats.push([CLIP_TEXT, utf16(text.replace(/\r?\n/g, '\r\n'))]);
        if (html) formats.push([CLIP_HTML, new TextEncoder().encode(html)]);
        if (image) formats.push([CLIP_IMAGE, encodeImage(image)]);
        return new Promise((resolve) => {
            for (const [format, data] of formats) {
                if (data.length > CLIP_MAX) continue;
                this.push(MSG_CLIP_FORMAT, uint32s(format, data.length));
                if (data.length) this.push(MSG_CLIP_DATA, data);
            }
            this.push(MSG_CLIP_END, undefined, { onDone: resolve });
        });
    }

    // Resolves once the guest has received the whole file
    sendFile(name, data, onProgress) {
        name = name.normalize('NFC'); // macOS can hand out decomposed names, Windows XP expects composed ones
        const header = new Uint8Array(4 + name.length * 2);
        header.set(uint32s(data.length));
        header.set(utf16(name), 4);
        return new Promise((resolve) => {
            this.push(MSG_FILE_BEGIN, header);
            if (data.length) this.push(MSG_FILE_DATA, data, { onProgress });
            this.push(MSG_FILE_END, undefined, { onDone: resolve });
        });
    }

    // Powers the guest off, used by the build to store a clean disk
    shutdown() {
        this.push(MSG_SHUTDOWN);
    }

    // Gets a fresh address from the LAN relay, every visitor resumes the same snapshot
    renewNetwork() {
        this.push(MSG_NET_RENEW);
    }

    recv(cpu, reg) {
        const item = this.queue[0];
        if (!item) return 0;
        const phys = reg[REG_EBX] >>> 0;
        const length = Math.min(reg[REG_ESI] >>> 0, item.data.length - item.offset);
        cpu.write_blob(item.data.subarray(item.offset, item.offset + length), phys);
        item.offset += length;
        // Only data chunks continue over several messages; anything else is truncated
        if (!CHUNKED.has(item.type) || item.offset >= item.data.length) this.queue.shift();
        item.onProgress?.(item.offset / item.data.length);
        item.onDone?.();
        reg[REG_EBX] = length;
        return item.type;
    }

    receive(type, data) {
        if (type === MSG_CLIP_FORMAT) {
            const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
            const format = data.length >= 8 ? view.getUint32(0, true) : -1;
            const size = data.length >= 8 ? view.getUint32(4, true) : 0;
            const valid = format >= CLIP_TEXT && format <= CLIP_IMAGE && size <= CLIP_MAX;
            this.clipCurrent = valid ? (this.clip[format] = { data: new Uint8Array(size), offset: 0 }) : null;
        } else if (type === MSG_CLIP_DATA && this.clipCurrent) {
            const current = this.clipCurrent;
            const chunk = data.subarray(0, current.data.length - current.offset);
            current.data.set(chunk, current.offset);
            current.offset += chunk.length;
        } else if (type === MSG_CLIP_END) {
            const [text, html, image] = this.clip.map((format) => format?.data);
            this.clip = [];
            this.clipCurrent = null;
            const detail = {
                text: text && fromUtf16(text).replace(/\r\n/g, '\n'),
                html: html && new TextDecoder().decode(html),
                image: image && decodeImage(image),
            };
            if (Object.values(detail).some(Boolean)) this.dispatchEvent(new CustomEvent('clipboard', { detail }));
        } else if (type === MSG_OPEN_URL) {
            this.dispatchEvent(new CustomEvent('open-url', { detail: fromUtf16(data) }));
        } else if (type === MSG_LOG) {
            console.debug('[winxp]', fromUtf16(data));
        }
    }
}
