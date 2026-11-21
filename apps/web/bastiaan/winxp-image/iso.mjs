#!/usr/bin/env node
// Extracts files from the ISO 9660 file system of a CD image, by their path in the primary volume
// Usage: iso.mjs <image.iso> <out dir> <path>...

import fs from 'node:fs';
import path from 'node:path';

const SECTOR = 2048;
const [image, outDir, ...paths] = process.argv.slice(2);
const fd = fs.openSync(image, 'r');
const read = (offset, length) => {
    const buffer = Buffer.alloc(length);
    fs.readSync(fd, buffer, 0, length, offset);
    return buffer;
};

// A directory record: extent, size and flags, the name without its ";1" version
const record = (data, offset) => ({
    extent: data.readUInt32LE(offset + 2),
    size: data.readUInt32LE(offset + 10),
    isDirectory: (data[offset + 25] & 2) !== 0,
    name: data.toString('latin1', offset + 33, offset + 33 + data[offset + 32]).replace(/;\d+$/, ''),
});

function* entries(directory) {
    const data = read(directory.extent * SECTOR, directory.size);
    for (let offset = 0; offset < data.length; ) {
        const length = data[offset];
        if (length === 0) {
            // Records don't cross sectors, the rest of this one is padding
            offset = (Math.floor(offset / SECTOR) + 1) * SECTOR;
            continue;
        }
        yield record(data, offset);
        offset += length;
    }
}

const volume = read(16 * SECTOR, SECTOR);
if (volume[0] !== 1 || volume.toString('latin1', 1, 6) !== 'CD001') throw new Error(`${image}: no ISO 9660 volume`);
const root = record(volume, 156);

for (const file of paths) {
    const entry = file.split('/').reduce((directory, name) => {
        const found = directory && [...entries(directory)].find((entry) => entry.name.toUpperCase() === name.toUpperCase());
        if (!found) throw new Error(`${image}: ${file} not found`);
        return found;
    }, root);
    fs.writeFileSync(path.join(outDir, path.basename(file)), read(entry.extent * SECTOR, entry.size));
}
