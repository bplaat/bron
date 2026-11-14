// Host side of the v86 guest integration, see offline/guest/v86.h
// Extends v86's VMware backdoor with commands the guest agent and drivers use.

const VMWARE_PORT = 0x5658;
const VMWARE_MAGIC = 0x564d5868;
const REG_EAX = 0;
const REG_ECX = 1;
const REG_EBX = 3;
const REG_ESI = 6;
const REG_EDI = 7;

const CMD_HELLO = 0x8600;
const CMD_GETSIZE = 0x8601;
const CMD_RECV = 0x8602;
const CMD_SEND = 0x8603;
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

export class GuestHost extends EventTarget {
    constructor(emulator) {
        super();
        this.emulator = emulator;
        this.queue = [];
        this.clip = [];
        this.clipCurrent = null;
        this.size = 0;
        this.agentSeen = -Infinity;
        const cpu = emulator.v86.cpu;
        const vmware = cpu.devices.vmware;
        const stock = vmware.port_read32;
        cpu.io.register_read(
            VMWARE_PORT,
            vmware,
            () => 0xff,
            () => 0xffff,
            () => {
                const reg = cpu.reg32;
                if (reg[REG_EAX] !== VMWARE_MAGIC) return stock.call(vmware);
                switch (reg[REG_ECX] & 0xffff) {
                    case CMD_HELLO:
                        return HELLO_REPLY;
                    case CMD_GETSIZE:
                        return this.size;
                    case CMD_RECV:
                        this.agentSeen = performance.now();
                        return this.recv(cpu, reg);
                    case CMD_SEND:
                        this.receive(reg[REG_EDI] >>> 0, cpu.mem8.slice(reg[REG_EBX] >>> 0, (reg[REG_EBX] >>> 0) + (reg[REG_ESI] >>> 0)));
                        return 0;
                }
                return stock.call(vmware);
            },
        );
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
