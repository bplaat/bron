// Worker for make-state.mjs: compresses every count-th disk chunk starting at index unless its previous hash matches,
// replies with the packed bytes, the number of compressed chunks and the chunk hashes

import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { parentPort, workerData } from 'node:worker_threads';
import zlib from 'node:zlib';
import { DISK_CHUNK_SIZE } from '../web/src/machine.ts';

const { image, outDir, size, index, count, previous } = workerData;
const fd = fs.openSync(image, 'r');
const chunk = Buffer.alloc(DISK_CHUNK_SIZE);
const hashes = {};
let packed = 0;
let compressed = 0;
for (let offset = index * DISK_CHUNK_SIZE; offset < size; offset += count * DISK_CHUNK_SIZE) {
    chunk.fill(0);
    fs.readSync(fd, chunk, 0, DISK_CHUNK_SIZE, offset);
    const file = path.join(outDir, `disk/c-${offset}-${offset + DISK_CHUNK_SIZE}.img.zst`);
    const hash = (hashes[offset] = crypto.createHash('sha1').update(chunk).digest('hex'));
    if (previous[offset] === hash && fs.existsSync(file)) {
        packed += fs.statSync(file).size;
        continue;
    }
    const data = zlib.zstdCompressSync(chunk, { params: { [zlib.constants.ZSTD_c_compressionLevel]: 19 } });
    packed += data.length;
    compressed++;
    fs.writeFileSync(file, data);
}
fs.closeSync(fd);
parentPort.postMessage({ packed, compressed, hashes });
