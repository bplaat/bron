// Worker for make-state.mjs: compresses every count-th disk chunk starting at index, replies with the packed bytes
import fs from 'node:fs';
import path from 'node:path';
import { parentPort, workerData } from 'node:worker_threads';
import zlib from 'node:zlib';
import { DISK_CHUNK_SIZE } from '../src/machine.js';

const { image, outDir, size, index, count } = workerData;
const fd = fs.openSync(image, 'r');
const chunk = Buffer.alloc(DISK_CHUNK_SIZE);
let packed = 0;
for (let offset = index * DISK_CHUNK_SIZE; offset < size; offset += count * DISK_CHUNK_SIZE) {
    chunk.fill(0);
    fs.readSync(fd, chunk, 0, DISK_CHUNK_SIZE, offset);
    const data = zlib.zstdCompressSync(chunk, { params: { [zlib.constants.ZSTD_c_compressionLevel]: 19 } });
    packed += data.length;
    fs.writeFileSync(path.join(outDir, `disk/c-${offset}-${offset + DISK_CHUNK_SIZE}.img.zst`), data);
}
fs.closeSync(fd);
parentPort.postMessage(packed);
