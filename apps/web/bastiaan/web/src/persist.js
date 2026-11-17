/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// Keeps the session across reloads in IndexedDB: the machine state (RAM, CPU and devices) plus the disk blocks Windows
// wrote, grouped per disk chunk. Both are saved together in one transaction while the emulator is paused, so they
// always match: a disk without the RAM that cached it would corrupt FAT32. Saves belong to one build of the disk.
// Best-effort storage on purpose: navigator.storage.persist() shows a permission prompt in Firefox.

const BLOCK_SIZE = 256; // v86's block size for chunked disks
const STORES = ['state', 'chunks'];

const request = (req) =>
    new Promise((resolve, reject) => {
        req.onsuccess = () => resolve(req.result);
        req.onerror = () => reject(req.error);
    });

async function transform(data, Stream) {
    const stream = new Blob([data]).stream().pipeThrough(new Stream('gzip'));
    return new Uint8Array(await new Response(stream).arrayBuffer());
}
const compress = (data) => transform(data, CompressionStream);
const decompress = (data) => transform(data, DecompressionStream);

export class SessionStore {
    static async open(build) {
        const name = `winxp-${build}`;
        // Saves of older builds no longer fit the disk, outdated tells the page one was dropped
        const older = ((await indexedDB.databases?.()) ?? [])
            .map((database) => database.name)
            .filter((other) => other?.startsWith('winxp-') && other !== name);
        older.forEach((other) => indexedDB.deleteDatabase(other));
        const open = indexedDB.open(name, 1);
        open.onupgradeneeded = () => STORES.forEach((store) => open.result.createObjectStore(store));
        return new SessionStore(await request(open), older.length > 0);
    }

    constructor(db, outdated) {
        this.db = db;
        this.outdated = outdated;
        this.saving = false;
    }

    // Resolves to { state: ArrayBuffer, chunks: [{ index, mask, data }] } or null without a save
    async load() {
        const tx = this.db.transaction(STORES, 'readonly');
        const [state, chunks] = await Promise.all([
            request(tx.objectStore('state').get('state')),
            request(tx.objectStore('chunks').getAll()),
        ]);
        if (!state) return null;
        return {
            state: (await decompress(state)).buffer,
            chunks: await Promise.all(chunks.map(async (chunk) => ({ ...chunk, data: await decompress(chunk.data) }))),
        };
    }

    async clear() {
        const tx = this.db.transaction(STORES, 'readwrite');
        STORES.forEach((store) => tx.objectStore(store).clear());
        await new Promise((resolve) => (tx.oncomplete = resolve));
    }

    // Disk chunks with blocks written since the last save
    track(disk, chunkSize) {
        this.disk = disk;
        this.blocksPerChunk = chunkSize / BLOCK_SIZE;
        this.dirty = new Set();
        // Everything written so far, also by the snapshot, is not saved yet
        for (const block of disk.block_cache_is_write) this.dirty.add(Math.floor(block / this.blocksPerChunk));
        const write = disk.set.bind(disk);
        disk.set = (offset, data, done) => {
            const first = Math.floor(offset / BLOCK_SIZE / this.blocksPerChunk);
            const last = Math.floor((offset + data.length - 1) / BLOCK_SIZE / this.blocksPerChunk);
            for (let chunk = first; chunk <= last; chunk++) this.dirty.add(chunk);
            write(offset, data, done);
        };
    }

    // Puts saved disk blocks back, before the emulator runs
    restore(disk, chunks) {
        for (const { index, mask, data } of chunks) {
            for (let i = 0; i < mask.length * 8; i++) {
                if (!(mask[i >> 3] & (1 << (i & 7)))) continue;
                const block = index * (data.length / BLOCK_SIZE) + i;
                disk.block_cache.set(block, data.slice(i * BLOCK_SIZE, (i + 1) * BLOCK_SIZE));
                disk.block_cache_is_write.add(block);
            }
        }
    }

    // Copies a chunk's written blocks into one buffer with a bitmask of which are set
    readChunk(index) {
        const data = new Uint8Array(this.blocksPerChunk * BLOCK_SIZE);
        const mask = new Uint8Array(this.blocksPerChunk / 8);
        for (let i = 0; i < this.blocksPerChunk; i++) {
            const block = index * this.blocksPerChunk + i;
            if (!this.disk.block_cache_is_write.has(block)) continue;
            data.set(this.disk.block_cache.get(block), i * BLOCK_SIZE);
            mask[i >> 3] |= 1 << (i & 7);
        }
        return { index, mask, data };
    }

    async save(emulator) {
        if (this.saving) return;
        this.saving = true;
        const running = emulator.is_running();
        try {
            // Capture state and changed chunks at the same instant, leave the disk out of the state. The network card
            // keeps the guest's MAC: with MAC translation v86 would store this page's random one, which Windows never saw
            if (running) await emulator.stop();
            const net = emulator.v86.cpu.devices.net;
            const mac = net?.mac;
            const diskState = this.disk.get_state;
            this.disk.get_state = () => [[]];
            if (net?.mac_address_in_state) net.mac = net.mac_address_in_state;
            let state;
            try {
                state = await emulator.save_state();
            } finally {
                this.disk.get_state = diskState;
                if (net) net.mac = mac;
            }
            const dirty = [...this.dirty];
            this.dirty.clear();
            const chunks = dirty.map((index) => this.readChunk(index));
            if (running) emulator.run();

            const packed = await compress(state);
            const packedChunks = await Promise.all(
                chunks.map(async (chunk) => ({ ...chunk, data: await compress(chunk.data) })),
            );
            const tx = this.db.transaction(STORES, 'readwrite');
            tx.objectStore('state').put(packed, 'state');
            for (const chunk of packedChunks) tx.objectStore('chunks').put(chunk, chunk.index);
            await new Promise((resolve, reject) => {
                tx.oncomplete = resolve;
                tx.onerror = tx.onabort = () => reject(tx.error);
            }).catch((error) => {
                dirty.forEach((index) => this.dirty.add(index)); // try again next time
                throw error;
            });
        } finally {
            this.saving = false;
        }
    }
}
