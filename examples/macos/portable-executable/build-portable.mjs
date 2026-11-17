#!/usr/bin/env node
// Builds a portable executable and ad-hoc signs its embedded x86_64 and ARM64 Mach-O executables
// Usage: build-portable.mjs <source.s>

import fs from 'node:fs';
import path from 'node:path';
import { adhocSignature, nasm, padSignature } from '../codesign.mjs';

const MACHO_COPY_SIZE = 0x4000;
const SIGNATURE_ALIGNMENT = 16;
const MH_MAGIC_64 = 0xfeedfacf;
const MH_EXECUTE = 2;
const X86_64 = { cpuType: 0x01000007, cpuSubtype: 3, pageSize: 0x1000 };
const ARM64 = { cpuType: 0x0100000c, cpuSubtype: 0, pageSize: 0x4000 };

const source = path.resolve(process.argv[2] ?? '');
if (!fs.statSync(source, { throwIfNoEntry: false })?.isFile()) {
    console.error('Usage: build-portable.mjs <source.s>');
    process.exit(1);
}
const output = source.replace(/\.[^./]*$/, '.com');
const name = path.basename(output);

const assemble = (file, { x86_64, arm64 } = {}) =>
    nasm(
        '-I',
        `${path.dirname(source)}${path.sep}`,
        '-D',
        `SIGNATURE_IDENTIFIER_SIZE=${name.length + 1}`,
        ...(x86_64 ? ['-D', `MACHO_X86_64_SIGNATURE_FILE="${x86_64}"`] : []),
        ...(arm64 ? ['-D', `MACHO_ARM64_SIGNATURE_FILE="${arm64}"`] : []),
        source,
        '-o',
        file,
    );

// Signs the Mach-O for this architecture, whose header the portable file embeds and copies to its start when it runs
// as a Mach-O, returns false when the portable file has no Mach-O for it
function writeSignature(portableFile, signatureFile, { cpuType, cpuSubtype, pageSize }) {
    const portable = fs.readFileSync(portableFile);
    const header = Buffer.alloc(16);
    [MH_MAGIC_64, cpuType, cpuSubtype, MH_EXECUTE].forEach((value, i) => header.writeUInt32LE(value, i * 4));
    const headerOffset = portable.indexOf(header);
    if (headerOffset < 0) return false;
    if (headerOffset + MACHO_COPY_SIZE > portable.length) {
        throw new Error('embedded Mach-O header copy extends beyond the portable file');
    }
    const macho = Buffer.from(portable);
    portable.copy(macho, 0, headerOffset, headerOffset + MACHO_COPY_SIZE);

    const { signature, capacity } = adhocSignature(macho, name, pageSize);
    const size = Math.ceil(signature.length / SIGNATURE_ALIGNMENT) * SIGNATURE_ALIGNMENT;
    fs.writeFileSync(signatureFile, padSignature(signature, size, capacity));
    return true;
}

const buildDir = fs.mkdtempSync(path.join(path.dirname(output), `${name}.`));
try {
    // Each signature covers the code before it, so the file is assembled again after each one
    const prefix = path.join(buildDir, 'prefix.com');
    const x86_64 = path.join(buildDir, 'x86_64-signature.bin');
    const arm64 = path.join(buildDir, 'arm64-signature.bin');
    assemble(prefix);
    if (!writeSignature(prefix, x86_64, X86_64)) throw new Error('embedded x86_64 Mach-O header is missing');
    assemble(prefix, { x86_64 });
    const hasArm64 = writeSignature(prefix, arm64, ARM64);

    const executable = path.join(buildDir, name);
    assemble(executable, { x86_64, arm64: hasArm64 ? arm64 : undefined });
    fs.chmodSync(executable, fs.statSync(executable).mode | 0o100);
    fs.renameSync(executable, output);
} finally {
    fs.rmSync(buildDir, { recursive: true, force: true });
}
