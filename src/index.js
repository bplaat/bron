import { V86 } from 'v86';
import wasmUrl from 'v86/build/v86.wasm?url';
import { GuestHost } from './guest-host.js';
import { SessionStore } from './persist.js';
import { DISK_CHUNK_SIZE, MACHINE, patchNetworkCard } from './machine.js';
import './index.css';

const BASE = 'winxp/';
const MIN_SIZE = [800, 600]; // smaller makes Windows offer to fix the resolution, the page scales down instead
const MAX_SIZE = [2560, 1600];
const MAX_DROP_BYTES = 256 * 1024 * 1024; // dropped files are kept in browser memory
const IS_MAC = /Mac|iPhone|iPad/.test(navigator.platform);
const SAVE_INTERVAL = 2 * 60 * 1000;
// LAN with the other visitors through the server's relay next to the page, or VITE_RELAY_URL ('' turns it off)
const RELAY_URL = import.meta.env.VITE_RELAY_URL ?? new URL('relay', location.href).href.replace(/^http/, 'ws');
const BEST_EFFORT = 'Sessions are kept in this browser on a best effort basis, without guarantees.';

const screen = document.getElementById('screen');
const boot = document.getElementById('boot');
const bootStatus = document.getElementById('boot-status');
const drop = document.getElementById('drop');
const balloon = document.getElementById('balloon');
const activity = document.getElementById('activity');

// Notification balloon, optionally doing something when clicked
let balloonTimer;
let balloonAction = null;
function notify(title, text, { sticky = false, url = null, onClick = null } = {}) {
    document.getElementById('balloon-title').textContent = title;
    document.getElementById('balloon-text').textContent = text;
    balloonAction = url ? () => openTab(url) : onClick;
    balloon.classList.toggle('is-link', !!balloonAction);
    balloon.classList.add('is-visible');
    clearTimeout(balloonTimer);
    const duration = balloonAction || text.length > 100 ? 8000 : 4000;
    if (!sticky) balloonTimer = setTimeout(() => balloon.classList.remove('is-visible'), duration);
}
// Returns false when the browser blocked the new tab
function openTab(url) {
    const tab = window.open(url, '_blank');
    if (tab) tab.opener = null;
    return !!tab;
}

balloon.addEventListener('click', () => {
    balloonAction?.();
    balloon.classList.remove('is-visible');
});

function formatBytes(bytes) {
    return bytes < 1024 * 1024 ? `${Math.ceil(bytes / 1024)} KB` : `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

// Emulator, resuming the session saved in this browser when there is one
const disk = await fetch(BASE + 'disk.json').then((response) => response.json());
const sessions = await SessionStore.open(disk.build).catch(() => null);
const session = await sessions?.load().catch(() => null);
const emulator = new V86({
    ...MACHINE,
    wasm_path: wasmUrl,
    bios: { url: BASE + 'seabios.bin' },
    vga_bios: { url: BASE + 'vgabios.bin' },
    // C: streams in compressed chunks as Windows reads it
    hda: { url: BASE + 'disk/c.img.zst', size: disk.size, async: true, fixed_chunk_size: DISK_CHUNK_SIZE, use_parts: true },
    initial_state: session ? { buffer: session.state } : { url: BASE + 'state.bin.zst' },
    net_device: RELAY_URL ? { ...MACHINE.net_device, relay_url: RELAY_URL } : MACHINE.net_device,
    mac_address_translation: true, // every visitor resumes the same snapshot, v86 gives each page its own MAC
    screen_container: screen,
    autostart: !session, // the saved disk blocks go back in first
});

let activityTimer;
emulator.add_listener('download-progress', (event) => {
    if (event.file_name.includes('/disk/')) {
        // Show the disk light while chunks stream in
        activity.classList.add('is-visible');
        clearTimeout(activityTimer);
        activityTimer = setTimeout(() => activity.classList.remove('is-visible'), 300);
    }
});
emulator.add_listener('download-error', () => {
    bootStatus.textContent = 'Windows XP could not be loaded, please reload the page.';
});

const host = await new Promise((resolve) => emulator.add_listener('emulator-loaded', () => resolve(new GuestHost(emulator))));
patchNetworkCard(emulator);

// Session: saved when the page is hidden and every few minutes, a click on the notice starts over
const hda = emulator.v86.cpu.devices.ide.primary.master.buffer;
if (session) {
    sessions.restore(hda, session.chunks);
    emulator.run();
    notify('Welcome back', `Your previous session was restored. ${BEST_EFFORT} Click here to start fresh.`, {
        onClick: async () => {
            await sessions.clear();
            location.reload();
        },
    });
} else if (sessions?.outdated) {
    notify(
        'Windows was updated',
        `The disk changed since your last visit, so your previous session could not be restored. ${BEST_EFFORT}`,
    );
}
// The snapshot's address belongs to everyone who resumed it
if (RELAY_URL) host.renewNetwork();
if (sessions) {
    sessions.track(hda, DISK_CHUNK_SIZE);
    const save = () => sessions.save(emulator).catch((error) => console.warn('Could not save the session', error));
    document.addEventListener('visibilitychange', () => document.visibilityState === 'hidden' && save());
    setInterval(() => document.visibilityState === 'visible' && save(), SAVE_INTERVAL);
}

// Follow the window size: the guest switches to it, smaller windows scale the guest down
function guestSize() {
    // v86 undoes fractional device pixel ratios, so ask for device pixels there
    const ratio = window.devicePixelRatio % 1 ? window.devicePixelRatio : 1;
    return [
        Math.min(MAX_SIZE[0], Math.max(MIN_SIZE[0], Math.floor(window.innerWidth * ratio))),
        Math.min(MAX_SIZE[1], Math.max(MIN_SIZE[1], Math.floor(window.innerHeight * ratio))),
    ];
}

function fitScreen() {
    const { offsetWidth: width, offsetHeight: height } = screen;
    const scale = width && height ? Math.min(1, window.innerWidth / width, window.innerHeight / height) : 1;
    screen.style.setProperty('--scale', scale);
}

let resizeTimer;
function resize() {
    fitScreen();
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(() => host.setSize(...guestSize()), 250);
}
host.setSize(...guestSize());
window.addEventListener('resize', resize);
new ResizeObserver(fitScreen).observe(screen);

// Reveal the desktop once it runs at the window size and Windows had a moment to repaint, or after a while regardless
const REVEAL_SETTLE = 800;
const REVEAL_TIMEOUT = 8000;
const revealStart = performance.now();
let revealSized = null;
const reveal = setInterval(() => {
    const [width, height] = guestSize();
    const vga = emulator.v86.cpu.devices.vga;
    const sized = vga.svga_width === width && vga.svga_height === height;
    revealSized = sized ? (revealSized ?? performance.now()) : null;
    const now = performance.now();
    if ((revealSized && now - revealSized > REVEAL_SETTLE) || now - revealStart > REVEAL_TIMEOUT) {
        clearInterval(reveal);
        boot.classList.add('is-hidden');
        setTimeout(() => boot.remove(), 600);
    }
}, 100);

// Links opened in Windows open in a new tab, the guest click is still a recent user gesture most of the time
host.addEventListener('open-url', ({ detail: url }) => {
    if (!/^https?:\/\//i.test(url)) return;
    if (!openTab(url)) notify('Open link', `Click here to open ${url}`, { url });
});

// Sound: browsers only start audio after a user gesture
function startAudio() {
    const context = emulator.speaker_adapter?.audio_context;
    if (context?.state === 'suspended') context.resume();
}
window.addEventListener('pointerdown', startAudio, true);
window.addEventListener('keydown', startAudio, true);

function imageCanvas({ width, height, data }) {
    const canvas = Object.assign(document.createElement('canvas'), { width, height });
    canvas.getContext('2d').putImageData(new ImageData(data, width, height), 0, 0);
    return canvas;
}

// Hardware pointer: the pointer Windows sets is the browser's cursor over the screen, so it moves at native speed even
// when Windows is busy, 'none' (v86's default) while Windows draws one itself
const screenCanvas = screen.querySelector('canvas');
const cursorUrls = new Map();
host.addEventListener('pointer', ({ detail: pointer }) => {
    if (!pointer) return void (screenCanvas.style.cursor = 'none');
    if (!cursorUrls.has(pointer.key)) cursorUrls.set(pointer.key, imageCanvas(pointer).toDataURL());
    screenCanvas.style.cursor = `url(${cursorUrls.get(pointer.key)}) ${pointer.x} ${pointer.y}, auto`;
});

// Clipboard: guest to host, written on the next user interaction when the browser holds it back
function imageToPng(image) {
    const canvas = imageCanvas(image);
    return new Promise((resolve, reject) => canvas.toBlob((blob) => (blob ? resolve(blob) : reject()), 'image/png'));
}

let pendingCopy = null;
function writeClipboard() {
    if (!pendingCopy || !navigator.clipboard) return;
    const copy = pendingCopy;
    const write = window.ClipboardItem
        ? navigator.clipboard.write([new ClipboardItem(copy.items)])
        : navigator.clipboard.writeText(copy.text ?? '');
    write.then(
        () => pendingCopy === copy && (pendingCopy = null),
        () => {}, // retried on the next user interaction
    );
}
host.addEventListener('clipboard', ({ detail: { text, html, image } }) => {
    const items = {};
    if (text) items['text/plain'] = new Blob([text], { type: 'text/plain' });
    if (html) items['text/html'] = new Blob([html], { type: 'text/html' });
    if (image) items['image/png'] = imageToPng(image);
    pendingCopy = { items, text };
    writeClipboard();
});
window.addEventListener('pointerdown', writeClipboard, true);
window.addEventListener('focus', writeClipboard);

// Clipboard: host to guest, the paste shortcut is held back until the guest has the clipboard
async function decodeImage(blob) {
    try {
        const bitmap = await createImageBitmap(blob);
        const { width, height } = bitmap;
        const context = Object.assign(document.createElement('canvas'), { width, height }).getContext('2d');
        context.fillStyle = '#fff'; // the guest has no alpha
        context.fillRect(0, 0, width, height);
        context.drawImage(bitmap, 0, 0);
        return context.getImageData(0, 0, width, height);
    } catch {
        return null;
    }
}

async function readClipboard() {
    try {
        const [item] = await navigator.clipboard.read();
        const get = (type) => (item.types.includes(type) ? item.getType(type) : null);
        const imageType = item.types.find((type) => type.startsWith('image/'));
        return {
            text: await (await get('text/plain'))?.text(),
            html: await (await get('text/html'))?.text(),
            image: imageType && (await item.getType(imageType)),
        };
    } catch {
        return {};
    }
}

let pasteData = null;
document.addEventListener('paste', (event) => {
    const data = event.clipboardData;
    if (data) {
        pasteData = {
            text: data.getData('text/plain'),
            html: data.getData('text/html'),
            image: [...data.files].find((file) => file.type.startsWith('image/')),
        };
    }
    event.preventDefault();
});

async function paste() {
    pasteData = null;
    // Let the browser fire a paste event, fall back to the async clipboard API
    await new Promise((resolve) => setTimeout(resolve, 50));
    const { text, html, image } = pasteData ?? (await readClipboard());
    const clip = { text, html, image: image && (await decodeImage(image)) };
    if (clip.text || clip.html || clip.image) {
        await Promise.race([host.setClipboard(clip), new Promise((resolve) => setTimeout(resolve, 5000))]);
    }
    emulator.keyboard_send_scancodes([0x2f, 0xaf]); // V, the guest already holds Ctrl
}

// Keyboard: Cmd acts as Ctrl on macOS, and macOS drops key ups while Cmd is held
let metaKeys = null;
window.addEventListener(
    'keydown',
    (event) => {
        if (IS_MAC && event.key === 'Meta') {
            event.stopImmediatePropagation();
            event.preventDefault();
            if (!metaKeys) emulator.keyboard_send_scancodes([0x1d]);
            metaKeys = new Set();
            return;
        }
        if (metaKeys && event.isTrusted) metaKeys.add(event.code);
        if ((event.ctrlKey || (IS_MAC && event.metaKey)) && event.code === 'KeyV' && !event.repeat && host.agentAlive) {
            event.stopImmediatePropagation(); // v86 would prevent the paste event
            paste();
        }
    },
    true,
);
window.addEventListener(
    'keyup',
    (event) => {
        if (IS_MAC && event.key === 'Meta' && metaKeys) {
            event.stopImmediatePropagation();
            event.preventDefault();
            for (const code of metaKeys) window.dispatchEvent(new KeyboardEvent('keyup', { code, key: code }));
            emulator.keyboard_send_scancodes([0x9d]);
            metaKeys = null;
        } else if (metaKeys) {
            metaKeys.delete(event.code);
        }
    },
    true,
);
window.addEventListener('blur', () => {
    if (!metaKeys) return;
    emulator.keyboard_send_scancodes([0x9d]);
    metaKeys = null;
});

// Drag and drop files onto the desktop
const hasFiles = (event) => event.dataTransfer?.types.includes('Files');
let dragDepth = 0;
window.addEventListener('dragenter', (event) => {
    if (!hasFiles(event)) return;
    event.preventDefault();
    dragDepth++;
    drop.classList.add('is-visible');
});
window.addEventListener('dragleave', (event) => {
    if (!hasFiles(event)) return;
    if (--dragDepth <= 0) drop.classList.remove('is-visible');
});
window.addEventListener('dragover', (event) => {
    if (!hasFiles(event)) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = 'copy';
});
window.addEventListener('drop', async (event) => {
    if (!hasFiles(event)) return;
    event.preventDefault();
    dragDepth = 0;
    drop.classList.remove('is-visible');
    if (!host.agentAlive) return notify('Windows is starting', 'Try dropping the files again in a moment.');

    const files = [...event.dataTransfer.files].filter((file) => file.size || file.type);
    const total = files.reduce((sum, file) => sum + file.size, 0);
    if (!files.length) return;
    if (total > MAX_DROP_BYTES) {
        return notify('Files too large', `You can drop up to ${formatBytes(MAX_DROP_BYTES)} at a time.`);
    }
    let done = 0;
    for (const file of files) {
        const data = new Uint8Array(await file.arrayBuffer());
        await host.sendFile(file.name, data, (progress) => {
            const percent = Math.floor(((done + progress * file.size) / Math.max(total, 1)) * 100);
            notify('Copying to the desktop', `${file.name} - ${percent}%`, { sticky: true });
        });
        done += file.size;
    }
    notify('Copy complete', `${files.length === 1 ? files[0].name : `${files.length} files`} copied to the desktop.`);
});
