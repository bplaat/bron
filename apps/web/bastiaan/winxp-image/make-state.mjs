#!/usr/bin/env node
// Splits the disk into compressed chunks for streaming, then boots them and saves a snapshot of the desktop
// Usage: make-state.mjs <disk.img> <output dir>

import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { Worker } from 'node:worker_threads';
import zlib from 'node:zlib';
import { boot, screenshot, sleep, waitFor, waitIdle } from './lib.mjs';

// The snapshot compresses on every core, the same output in a third of the time
const zstd = (data) =>
    zlib.zstdCompressSync(data, {
        params: {
            [zlib.constants.ZSTD_c_compressionLevel]: 19,
            [zlib.constants.ZSTD_c_nbWorkers]: os.availableParallelism(),
        },
    });
const [, , image, outDir] = process.argv;
fs.mkdirSync(path.join(outDir, 'disk'), { recursive: true });
const size = fs.statSync(image).size;
// Chunks whose contents stay the same keep their compressed file, a new app build changes only a few
const hashesFile = path.resolve('out/chunks.json');
const previous = fs.existsSync(hashesFile) ? JSON.parse(fs.readFileSync(hashesFile, 'utf8')) : {};
const count = os.availableParallelism(); // chunks.mjs compresses them in parallel
const results = await Promise.all(
    Array.from(
        { length: count },
        (_, index) =>
            new Promise((resolve, reject) => {
                const workerData = { image, outDir, size, index, count, previous };
                new Worker(new URL('chunks.mjs', import.meta.url), { workerData })
                    .on('message', resolve)
                    .on('error', reject);
            }),
    ),
);
fs.writeFileSync(hashesFile, JSON.stringify(Object.assign({}, ...results.map((result) => result.hashes))));
const packed = results.reduce((sum, result) => sum + result.packed, 0);
const compressed = results.reduce((sum, result) => sum + result.compressed, 0);
console.log(
    `disk ${(size / 2 ** 30).toFixed(1)} GiB, packed ${(packed / 2 ** 20).toFixed(1)} MiB, ${compressed} chunks compressed`,
);
for (const name of ['seabios.bin', 'vgabios.bin']) {
    fs.copyFileSync(path.join(path.dirname(new URL(import.meta.url).pathname), 'bios', name), path.join(outDir, name));
}

// The raw image stands in for the chunks: both buffers store only written blocks in a snapshot
const { emulator, host } = await boot({ url: path.resolve(image), async: true, size });
host.setSize(1024, 768); // a smaller desktop makes Windows offer to fix the resolution
await waitFor(emulator, 'desktop', () => host.agentAlive, 30);
await waitIdle(emulator, 30); // let the logon and Bastiaan settle
await sleep(10 * 1000);
screenshot(emulator, path.resolve('out/desktop.png'));

const state = new Uint8Array(await emulator.save_state());
const packedState = zstd(state);
fs.writeFileSync(path.join(outDir, 'state.bin.zst'), packedState);
// The build id ties sessions saved in browsers to this disk and snapshot
const build = crypto.createHash('sha256').update(packedState).digest('hex').slice(0, 16);
fs.writeFileSync(path.join(outDir, 'disk.json'), JSON.stringify({ size, build }) + '\n');
console.log(
    `state ${(state.length / 2 ** 20).toFixed(1)} MiB, packed ${(packedState.length / 2 ** 20).toFixed(1)} MiB`,
);
emulator.destroy();
process.exit(0);
