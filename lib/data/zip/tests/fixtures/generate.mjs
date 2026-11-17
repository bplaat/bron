#!/usr/bin/env node
// Copyright (c) 2026 Bastiaan van der Plaat
// SPDX-License-Identifier: MIT

// Generates interoperability fixtures with a ZIP writer independent of the Rust implementation, using Node's zlib
// Usage: ./generate.mjs

import fs from 'node:fs';
import zlib from 'node:zlib';

const STORED = 0;
const DEFLATED = 8;
const DATA_DESCRIPTOR = 0x08;
// 2020-02-29 12:34:56 as MS-DOS time and date
const DOS_TIME = (12 << 11) | (34 << 5) | (56 >> 1);
const DOS_DATE = ((2020 - 1980) << 9) | (2 << 5) | 29;

const pack = (...fields) =>
    Buffer.concat(
        fields.map(([size, value]) => {
            const buffer = Buffer.alloc(size);
            if (size === 1) buffer.writeUInt8(value);
            else if (size === 2) buffer.writeUInt16LE(value);
            else if (size === 4) buffer.writeUInt32LE(value);
            else buffer.writeBigUInt64LE(BigInt(value));
            return buffer;
        }),
    );

// Extended timestamp extra field with the modification time
const unixTime = (time) => pack([2, 0x5455], [2, 5], [1, 1], [4, time]);

// Writes a ZIP with Unix modes, a data descriptor makes the writer stream the entry like it cannot seek back
function writeZip(file, entries, { method = STORED, streamed = false } = {}) {
    const locals = [];
    const centrals = [];
    let offset = 0;
    for (const { name, mode, data = Buffer.alloc(0), extra = Buffer.alloc(0) } of entries) {
        const nameBytes = Buffer.from(name);
        const content = method === DEFLATED ? zlib.deflateRawSync(data) : data;
        const crc = zlib.crc32(data);
        const flags = streamed ? DATA_DESCRIPTOR : 0;
        const header = (crc, compressed, size) =>
            pack([2, 20], [2, flags], [2, method], [2, DOS_TIME], [2, DOS_DATE], [4, crc], [4, compressed], [4, size]);
        const sizes = [crc, content.length, data.length];
        const local = Buffer.concat([
            pack([4, 0x04034b50]),
            streamed ? header(0, 0, 0) : header(...sizes),
            pack([2, nameBytes.length], [2, extra.length]),
            nameBytes,
            extra,
            content,
            streamed ? pack([4, 0x08074b50], [4, crc], [4, content.length], [4, data.length]) : Buffer.alloc(0),
        ]);
        centrals.push(
            Buffer.concat([
                // Made by Unix with ZIP 2.0
                pack([4, 0x02014b50], [2, 0x0314]),
                header(...sizes),
                pack([2, nameBytes.length], [2, extra.length], [2, 0], [2, 0], [2, 0], [4, mode * 0x10000], [4, offset]),
                nameBytes,
                extra,
            ]),
        );
        locals.push(local);
        offset += local.length;
    }
    const central = Buffer.concat(centrals);
    const end = pack(
        [4, 0x06054b50],
        [2, 0],
        [2, 0],
        [2, entries.length],
        [2, entries.length],
        [4, central.length],
        [4, offset],
        [2, 0],
    );
    fs.writeFileSync(new URL(file, import.meta.url), Buffer.concat([...locals, central, end]));
}

const link = (name, target) => ({ name, mode: 0o120777, data: Buffer.from(target) });

writeZip('unix.zip', [
    { name: 'bin/', mode: 0o40500, extra: unixTime(1700000001) },
    { name: 'bin/run.sh', mode: 0o104755, data: Buffer.from('#!/bin/sh\nexit 0\n'), extra: unixTime(1700000002) },
    { ...link('shortcut', 'bin/run.sh'), extra: unixTime(1700000003) },
    { ...link('chain', 'shortcut'), extra: unixTime(1700000004) },
    link('bin/parent-link', '../shortcut'),
]);

writeZip('dos.zip', [{ name: 'file', mode: 0o100644, data: Buffer.from('DOS timestamp') }]);

// NTFS extra field with the modification, access and creation times in 100 ns ticks since 1601
const ticks = 116444736000000000n + 1700000000n * 10000000n + 1234567n;
writeZip('ntfs.zip', [
    {
        name: 'file',
        mode: 0o100644,
        data: Buffer.from('NTFS timestamp'),
        extra: pack([2, 0x000a], [2, 32], [4, 0], [2, 1], [2, 24], [8, ticks], [8, ticks], [8, ticks]),
    },
]);

writeZip('escape-link.zip', [link('link', '../outside')]);
writeZip('absolute-link.zip', [link('link', '/etc/passwd')]);
writeZip('cycle-links.zip', [link('a', 'b'), link('b', 'a')]);
writeZip('existing-link-target.zip', [link('link', 'outside/file')]);
writeZip('special.zip', [{ name: 'fifo', mode: 0o10644 }]);

writeZip(
    'descriptor.zip',
    [{ name: 'streamed.txt', mode: 0o100644, data: Buffer.from('streamed data'.repeat(100)), extra: unixTime(1700000000) }],
    { method: DEFLATED, streamed: true },
);
