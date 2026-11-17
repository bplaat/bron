#!/usr/bin/env node
// Installs an old Microsoft game from its CD like its setup does, without running the setup: the setup program holds
// its file table as NUL terminated pairs of CD path and destination, destinations use %APPPATH and %STRINGn from the
// setup DLL's string table. Files from the CD's SYSTEM folder go to Windows, the disk copy keeps newer existing ones.
// CABs on the CD are extracted separately.
// Usage: ebu.mjs <setup.exe> <strings.dll> <cd dir> <C:\ root dir> <install dir relative to C:\>

import fs from 'node:fs';
import path from 'node:path';

// Yields { type, name, data } for every resource of a PE file
function* resources(file) {
    const data = fs.readFileSync(file);
    const pe = data.readUInt32LE(0x3c);
    const sectionCount = data.readUInt16LE(pe + 6);
    const sectionTable = pe + 24 + data.readUInt16LE(pe + 20);
    const sections = Array.from({ length: sectionCount }, (_, i) => {
        const at = sectionTable + i * 40 + 8;
        return [0, 4, 8, 12].map((field) => data.readUInt32LE(at + field)); // size, va, raw size, raw
    });
    const offset = (rva) => {
        const [, va, , raw] = sections.find(([size, va, rawSize]) => va <= rva && rva < va + Math.max(size, rawSize));
        return rva - va + raw;
    };
    const base = offset(data.readUInt32LE(pe + 24 + 96 + 2 * 8));
    const entries = (directory) => {
        const count = data.readUInt16LE(directory + 12) + data.readUInt16LE(directory + 14);
        return Array.from({ length: count }, (_, i) => [
            data.readUInt32LE(directory + 16 + i * 8),
            data.readUInt32LE(directory + 20 + i * 8),
        ]);
    };
    for (const [type, names] of entries(base)) {
        for (const [name, languages] of entries(base + (names & 0x7fffffff))) {
            for (const [, leaf] of entries(base + (languages & 0x7fffffff))) {
                const start = offset(data.readUInt32LE(base + leaf));
                yield { type, name, data: data.subarray(start, start + data.readUInt32LE(base + leaf + 4)) };
            }
        }
    }
}

// RT_STRING blocks hold 16 length prefixed UTF-16 strings each
function stringTable(file) {
    const strings = new Map();
    for (const { type, name: block, data } of resources(file)) {
        if (type !== 6) continue;
        for (let i = 0, pos = 0; i < 16; i++) {
            const length = data.readUInt16LE(pos);
            strings.set((block - 1) * 16 + i, data.toString('utf16le', pos + 2, pos + 2 + length * 2));
            pos += 2 + length * 2;
        }
    }
    return strings;
}

// The n of every %STRINGn in a destination
const stringRefs = (dest) =>
    dest
        .split('%STRING')
        .slice(1)
        .map((ref) => {
            const n = Number(ref.split('%')[0].replace(/\\+$/, ''));
            if (!Number.isInteger(n)) throw new Error(`bad string reference in ${dest}`);
            return n;
        });

const [setup, stringsDll, cd, root, app] = process.argv.slice(2);
const strings = stringTable(stringsDll);
const cdFiles = new Map(
    fs
        .readdirSync(cd, { recursive: true })
        .filter((relative) => fs.statSync(path.join(cd, relative)).isFile())
        .map((relative) => [relative.replaceAll('/', '\\').toUpperCase(), path.join(cd, relative)]),
);

const tokens = fs.readFileSync(setup).toString('latin1').split('\0');
const pairs = tokens
    .slice(0, -1)
    .map((source, i) => [source.toUpperCase(), tokens[i + 1]])
    .filter(([source]) => cdFiles.has(source))
    .map(([source, dest]) => [cdFiles.get(source), dest]);

// %STRINGn is resource string n plus a per setup base, the one under which every reference resolves
const refs = new Set(pairs.flatMap(([, dest]) => stringRefs(dest)));
const base = [0, 500, 1000].find((base) => [...refs].every((n) => strings.get(base + n)));

const installed = new Map();
for (let [source, dest] of pairs) {
    for (const n of [...new Set(stringRefs(dest))].sort((a, b) => b - a)) {
        dest = dest.replaceAll(`%STRING${n}`, strings.get(base + n));
    }
    if (dest.toUpperCase().startsWith('%APPPATH\\')) dest = dest.slice('%APPPATH\\'.length);
    if (/[%:@]/.test(dest) || !dest.includes('.')) continue; // setup variables and noise
    const [folder, subfolder] = path.relative(cd, source).toUpperCase().split(path.sep);
    if (folder === 'DIRECTX') continue; // DirectX updates for Windows 9x, XP has newer
    dest =
        folder === 'SYSTEM'
            ? `WINDOWS\\${subfolder === 'FONTS' ? 'Fonts' : 'system32'}\\${dest}`
            : `${app.replaceAll('/', '\\')}\\${dest}`;
    installed.set(dest.toLowerCase(), [dest, source]);
}

for (const [dest, source] of installed.values()) {
    const file = path.join(root, ...dest.split('\\'));
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.copyFileSync(source, file);
}
console.log(`${installed.size} files from ${setup}`);
