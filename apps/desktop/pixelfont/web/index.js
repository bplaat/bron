/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

window.addEventListener('contextmenu', (e) => e.preventDefault());

const isMacosBwebview = navigator.userAgent.includes('bwebview') && navigator.userAgent.includes('Macintosh');

const ACTIONS = {
    new: 'newFile',
    open: 'openFile',
    save: 'saveFile',
    saveAs: 'saveFileAs',
    exportAsm: 'exportAsm',
    exportC: 'exportC',
    copy: 'copyChar',
    paste: 'pasteChar',
    clear: 'opClear',
    invert: 'opInvert',
    rotate: 'opRotate',
    mirrorH: 'opMirrorH',
    mirrorV: 'opMirrorV',
    clearAll: 'opClearAll',
    invertAll: 'opInvertAll',
    rotateAll: 'opRotateAll',
    mirrorHAll: 'opMirrorHAll',
    mirrorVAll: 'opMirrorVAll',
};

// IPC helpers
function ipcSend(type, data = {}) {
    window.ipc.postMessage(JSON.stringify({ type, ...data }));
}

// Requests of the same type are sent one at a time, so each response matches its request
const pendingRequests = {};
function ipcRequest(type, data = {}) {
    const response = (pendingRequests[type] || Promise.resolve()).then(
        () =>
            new Promise((resolve, reject) => {
                const listener = (event) => {
                    let message;
                    try {
                        message = JSON.parse(event.data);
                    } catch (e) {
                        window.ipc.removeEventListener('message', listener);
                        reject(e);
                        return;
                    }
                    if (message.type === `${type}Response`) {
                        window.ipc.removeEventListener('message', listener);
                        resolve(message);
                    }
                };
                window.ipc.addEventListener('message', listener);
                ipcSend(type, data);
            }),
    );
    pendingRequests[type] = response.catch(() => {});
    return response;
}

// Font helpers
function makeEmptyFont() {
    return new Uint8Array(256 * 8);
}

function getCharBytes(fontData, charIndex) {
    return fontData.slice(charIndex * 8, charIndex * 8 + 8);
}

function setCharBytes(fontData, charIndex, bytes) {
    for (let i = 0; i < 8; i++) {
        fontData[charIndex * 8 + i] = bytes[i];
    }
}

function charToAscii(charIndex) {
    if (charIndex >= 32 && charIndex < 127) return String.fromCharCode(charIndex);
    return null;
}

// Operations on a single char (8 bytes)
function opClearBytes() {
    return new Uint8Array(8);
}

function opInvertBytes(bytes) {
    return bytes.map((b) => (~b >>> 0) & 0xff);
}

function opRotateBytes(bytes) {
    const result = new Uint8Array(8);
    for (let col = 0; col < 8; col++) {
        let byte = 0;
        for (let row = 0; row < 8; row++) {
            if (bytes[row] & (1 << (7 - col))) {
                byte |= 1 << row;
            }
        }
        result[col] = byte;
    }
    return result;
}

function opMirrorHBytes(bytes) {
    return new Uint8Array(
        bytes.map((b) => {
            let r = 0;
            for (let i = 0; i < 8; i++) r |= ((b >> i) & 1) << (7 - i);
            return r;
        }),
    );
}

function opMirrorVBytes(bytes) {
    return new Uint8Array([...bytes].reverse());
}

// Draw a char onto a canvas (2-color: on/off from CSS vars, scale derived from canvas.width)
function renderCharCanvas(canvas, bytes) {
    const scale = canvas.width / 8;
    const ctx = canvas.getContext('2d');
    const style = getComputedStyle(document.documentElement);
    const onColor = style.getPropertyValue('--color-pixel-on').trim();
    const offColor = style.getPropertyValue('--color-pixel-off').trim();

    const img = ctx.createImageData(canvas.width, canvas.height);
    for (let row = 0; row < 8; row++) {
        for (let col = 0; col < 8; col++) {
            const on = !!(bytes[row] & (1 << (7 - col)));
            const color = on ? onColor : offColor;
            const hex = color.replace('#', '');
            let r, g, b;
            if (hex.length === 3) {
                r = parseInt(hex[0] + hex[0], 16);
                g = parseInt(hex[1] + hex[1], 16);
                b = parseInt(hex[2] + hex[2], 16);
            } else {
                r = parseInt(hex.slice(0, 2), 16);
                g = parseInt(hex.slice(2, 4), 16);
                b = parseInt(hex.slice(4, 6), 16);
            }
            for (let sy = 0; sy < scale; sy++) {
                for (let sx = 0; sx < scale; sx++) {
                    const idx = ((row * scale + sy) * canvas.width + (col * scale + sx)) * 4;
                    img.data[idx] = r;
                    img.data[idx + 1] = g;
                    img.data[idx + 2] = b;
                    img.data[idx + 3] = 255;
                }
            }
        }
    }
    ctx.putImageData(img, 0, 0);
}

// Module-level paint state (not reactive)
let isPainting = false;
let paintValue = true;
let replacementQueue = Promise.resolve();

PetiteVue.createApp({
    fontFileName: '',
    fontFilePath: '',
    fontData: makeEmptyFont(),
    selectedChar: 0,
    dirty: false,
    closePending: false,

    get charInfoText() {
        const idx = this.selectedChar;
        const ascii = charToAscii(idx);
        const dec = String(idx).padStart(3, ' ');
        const hex = '0x' + idx.toString(16).toUpperCase().padStart(2, '0');
        return ascii ? `Char: ${ascii} / ${dec} / ${hex}` : `Char: ${dec} / ${hex}`;
    },

    async init() {
        window.ipc.addEventListener('message', async (event) => {
            const message = JSON.parse(event.data);
            if (message.type === 'openFile') await this._openFont(message.path);
            if (message.type === 'restoreLastFile') {
                const lastFontPath = localStorage.getItem('lastFontPath');
                if (lastFontPath) await this._openFont(lastFontPath);
            }
            if (message.type === 'closeRequested') await this.closeWindow();
            if (message.type === 'menuAction') this.performAction(message.action);
        });
        window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
            this.renderAllChars();
            this._renderPreview();
        });

        window.addEventListener('keydown', (e) => {
            // Unmodified arrow keys walk the character grid
            if (e.metaKey || e.ctrlKey || e.altKey) return;
            let next = this.selectedChar;
            if (e.key === 'ArrowLeft') next = Math.max(0, this.selectedChar - 1);
            if (e.key === 'ArrowRight') next = Math.min(255, this.selectedChar + 1);
            if (e.key === 'ArrowUp') next = Math.max(0, this.selectedChar - 16);
            if (e.key === 'ArrowDown') next = Math.min(255, this.selectedChar + 16);
            if (next !== this.selectedChar) {
                e.preventDefault();
                this.selectChar(next);
                document.querySelector(`.char-canvas[data-char="${next}"]`)?.scrollIntoView({ block: 'nearest' });
            }
        });

        this.$nextTick(async () => {
            this._updateTitle();
            this.renderAllChars();
            this.selectChar(0);
            ipcSend('ready');
        });
    },

    performAction(id) {
        const action = ACTIONS[id];
        if (action) this[action]();
    },

    // File operations
    _updateTitle() {
        const name = this.fontFileName || 'Untitled';
        document.title = `8x8 Pixel Font Editor - ${name}${this.dirty ? '*' : ''}`;
    },

    _setDirty(dirty) {
        this.dirty = dirty;
        this._updateTitle();
        if (isMacosBwebview) ipcSend('macosDocumentEdited', { edited: dirty });
    },

    _markDirty() {
        if (!this.dirty) this._setDirty(true);
    },

    async _confirmSaveChanges() {
        if (!this.dirty) return true;
        const filename = this.fontFileName || 'Untitled';
        const { choice } = await ipcRequest('confirmSaveChanges', { filename });
        if (choice === 'save') return await this.saveFile();
        return choice === 'discard';
    },

    _replaceDocument(action) {
        const request = replacementQueue.then(async () => {
            if (!(await this._confirmSaveChanges())) return false;
            return await action();
        });
        replacementQueue = request.catch(() => false);
        return request;
    },

    newFile() {
        return this._replaceDocument(async () => {
            this.fontData = makeEmptyFont();
            this.fontFilePath = '';
            this.fontFileName = '';
            this._setDirty(false);
            this.$nextTick(() => {
                this.renderAllChars();
                this.selectChar(0);
            });
            return true;
        });
    },

    async openFile() {
        const { path } = await ipcRequest('openFileDialog');
        if (!path) return;
        await this._openFont(path);
    },

    _openFont(path) {
        return this._replaceDocument(() => this._loadFont(path));
    },

    async _loadFont(path) {
        const { ok, data, error } = await ipcRequest('openFont', { path });
        if (!ok) {
            if (localStorage.getItem('lastFontPath') === path) {
                localStorage.removeItem('lastFontPath');
            }
            await ipcRequest('showError', { title: 'Failed to open font file', description: error });
            return false;
        }
        this.fontData = new Uint8Array(data);
        this.fontFilePath = path;
        this.fontFileName = path.replace(/.*[\\/]/, '');
        localStorage.setItem('lastFontPath', path);
        this._setDirty(false);

        this.$nextTick(() => {
            this.renderAllChars();
            this.selectChar(0);
        });
        return true;
    },

    async saveFile() {
        if (!this.fontFilePath) {
            return await this.saveFileAs();
        }
        return await this._writeFont(this.fontFilePath);
    },

    async saveFileAs() {
        const baseName = this.fontFileName || 'font.pf';
        const { path } = await ipcRequest('saveFileDialog', { filename: baseName });
        if (!path) return false;
        const saved = await this._writeFont(path);
        if (saved) {
            this.fontFilePath = path;
            this.fontFileName = path.replace(/.*[\\/]/, '');
            localStorage.setItem('lastFontPath', path);
            this._updateTitle();
        }
        return saved;
    },

    async _writeFont(path) {
        const data = Array.from(this.fontData);
        const { ok, error } = await ipcRequest('saveFont', { path, data });
        if (!ok) {
            await ipcRequest('showError', { title: 'Failed to save font file', description: error });
            return false;
        }
        this._setDirty(false);
        return true;
    },

    async closeWindow() {
        if (this.closePending) return;
        this.closePending = true;
        try {
            await this._replaceDocument(async () => {
                ipcSend('closeWindow');
                return true;
            });
        } finally {
            this.closePending = false;
        }
    },

    // Export
    async exportAsm() {
        const filename = (this.fontFileName || 'font').replace(/\.pf$/i, '') + '.asm';
        const { path } = await ipcRequest('exportFileDialog', { filename });
        if (!path) return;
        const text = this._generateAsm();
        const { ok, error } = await ipcRequest('exportFile', { path, text });
        if (!ok) await ipcRequest('showError', { title: 'Failed to export', description: error });
    },

    async exportC() {
        const filename = (this.fontFileName || 'font').replace(/\.pf$/i, '') + '.h';
        const { path } = await ipcRequest('exportFileDialog', { filename });
        if (!path) return;
        const text = this._generateC();
        const { ok, error } = await ipcRequest('exportFile', { path, text });
        if (!ok) await ipcRequest('showError', { title: 'Failed to export', description: error });
    },

    _generateAsm() {
        const lines = [`; Font: ${this.fontFileName || 'font.pf'}`, ''];
        for (let i = 0; i < 256; i++) {
            const bytes = getCharBytes(this.fontData, i);
            const hex = Array.from(bytes)
                .map((b) => '0x' + b.toString(16).toUpperCase().padStart(2, '0'))
                .join(', ');
            const ascii = charToAscii(i);
            const comment = ascii ? ` ; ${ascii}` : '';
            lines.push(`char_${String(i).padStart(3, '0')}: db ${hex}${comment}`);
        }
        return lines.join('\n') + '\n';
    },

    _generateC() {
        const lines = [
            `// Font: ${this.fontFileName || 'font.pf'}`,
            '',
            '#pragma once',
            '',
            '#include <stdint.h>',
            '',
            'static const uint8_t font[256][8] = {',
        ];
        for (let i = 0; i < 256; i++) {
            const bytes = getCharBytes(this.fontData, i);
            const hex = Array.from(bytes)
                .map((b) => '0x' + b.toString(16).toUpperCase().padStart(2, '0'))
                .join(', ');
            const ascii = charToAscii(i);
            const label = ascii ? `${String(i).padStart(3, ' ')} '${ascii}'` : `${String(i).padStart(3, ' ')}    `;
            const comma = i < 255 ? ',' : ' ';
            lines.push(`    { ${hex} }${comma} /* ${label} */`);
        }
        lines.push('};', '');
        return lines.join('\n');
    },

    // Character selection & rendering
    selectChar(index) {
        this.selectedChar = index;
        this._renderPreview();
    },

    renderAllChars() {
        for (let i = 0; i < 256; i++) {
            this._renderCharAt(i);
        }
    },

    _renderCharAt(index) {
        const canvas = document.querySelector(`.char-canvas[data-char="${index}"]`);
        if (!canvas) return;
        renderCharCanvas(canvas, getCharBytes(this.fontData, index));
    },

    _renderPreview() {
        const canvas = this.$refs.previewCanvas;
        if (!canvas) return;
        renderCharCanvas(canvas, getCharBytes(this.fontData, this.selectedChar));
    },

    // Pixel editor reads/writes directly from/to fontData
    getPixel(row, col) {
        return !!(this.fontData[this.selectedChar * 8 + row] & (1 << (7 - col)));
    },

    getRowDec(row) {
        return String(this.fontData[this.selectedChar * 8 + row]).padStart(3, ' ');
    },

    getRowHex(row) {
        return this.fontData[this.selectedChar * 8 + row].toString(16).toUpperCase().padStart(2, '0');
    },

    // Pointer-based painting (robust cross-cell drag)
    gridDown(event) {
        const cell = this._cellFromEvent(event);
        if (!cell) return;
        isPainting = true;
        paintValue = !this.getPixel(cell.row, cell.col);
        event.currentTarget.setPointerCapture(event.pointerId);
        this._setPixel(cell.row, cell.col, paintValue);
    },

    gridMove(event) {
        if (!isPainting) return;
        const cell = this._cellFromEvent(event);
        if (!cell) return;
        this._setPixel(cell.row, cell.col, paintValue);
    },

    gridUp() {
        isPainting = false;
    },

    _cellFromEvent(event) {
        const grid = this.$refs.pixelGrid;
        if (!grid) return null;
        const rect = grid.getBoundingClientRect();
        const col = Math.floor(((event.clientX - rect.left) / rect.width) * 8);
        const row = Math.floor(((event.clientY - rect.top) / rect.height) * 8);
        if (col < 0 || col >= 8 || row < 0 || row >= 8) return null;
        return { row, col };
    },

    _setPixel(row, col, value) {
        const idx = this.selectedChar * 8 + row;
        const newData = new Uint8Array(this.fontData);
        if (value) {
            newData[idx] |= 1 << (7 - col);
        } else {
            newData[idx] &= ~(1 << (7 - col));
        }
        this.fontData = newData;
        this._renderCharAt(this.selectedChar);
        this._renderPreview();
        this._markDirty();
    },

    // Single-char operations (write directly to fontData)
    _opChar(fn) {
        const bytes = fn(getCharBytes(this.fontData, this.selectedChar));
        const newData = new Uint8Array(this.fontData);
        setCharBytes(newData, this.selectedChar, bytes);
        this.fontData = newData;
        this._renderCharAt(this.selectedChar);
        this._renderPreview();
        this._markDirty();
    },

    opClear() {
        this._opChar(() => opClearBytes());
    },
    opInvert() {
        this._opChar((b) => opInvertBytes(b));
    },
    opRotate() {
        this._opChar((b) => opRotateBytes(b));
    },
    opMirrorH() {
        this._opChar((b) => opMirrorHBytes(b));
    },
    opMirrorV() {
        this._opChar((b) => opMirrorVBytes(b));
    },

    // Copy hex bytes to clipboard
    copyChar() {
        const hex = Array.from(getCharBytes(this.fontData, this.selectedChar))
            .map((b) => '0x' + b.toString(16).toUpperCase().padStart(2, '0'))
            .join(', ');
        navigator.clipboard.writeText(hex).catch(() => {});
    },

    // Paste hex bytes from clipboard
    async pasteChar() {
        try {
            const text = await navigator.clipboard.readText();
            const values = text.match(/0x[0-9A-Fa-f]{1,2}|\b[0-9]{1,3}\b/g);
            if (!values || values.length < 8) return;
            const bytes = new Uint8Array(8);
            for (let i = 0; i < 8; i++) {
                const v = values[i];
                const n = v.startsWith('0x') ? parseInt(v, 16) : parseInt(v, 10);
                if (n < 0 || n > 255) return;
                bytes[i] = n;
            }
            const newData = new Uint8Array(this.fontData);
            setCharBytes(newData, this.selectedChar, bytes);
            this.fontData = newData;
            this._renderCharAt(this.selectedChar);
            this._renderPreview();
            this._markDirty();
        } catch (_) {}
    },

    // Bulk operations (all 256 chars)
    _applyToAll(fn) {
        const newData = new Uint8Array(this.fontData);
        for (let i = 0; i < 256; i++) {
            setCharBytes(newData, i, fn(getCharBytes(newData, i)));
        }
        this.fontData = newData;
        this.renderAllChars();
        this._renderPreview();
        this._markDirty();
    },

    opClearAll() {
        this._applyToAll(() => opClearBytes());
    },
    opInvertAll() {
        this._applyToAll((b) => opInvertBytes(b));
    },
    opRotateAll() {
        this._applyToAll((b) => opRotateBytes(b));
    },
    opMirrorHAll() {
        this._applyToAll((b) => opMirrorHBytes(b));
    },
    opMirrorVAll() {
        this._applyToAll((b) => opMirrorVBytes(b));
    },
}).mount('#app');
