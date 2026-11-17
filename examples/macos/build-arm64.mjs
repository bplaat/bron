#!/usr/bin/env node
// Builds an ARM64 Mach-O assembly executable and ad-hoc signs it
// Usage: build-arm64.mjs <source.s>

import fs from 'node:fs';
import path from 'node:path';
import { adhocSignature, nasm, padSignature } from './codesign.mjs';

const PAGE_SIZE = 0x4000;

const source = path.resolve(process.argv[2] ?? '');
if (!fs.statSync(source, { throwIfNoEntry: false })?.isFile()) {
    console.error('Usage: build-arm64.mjs <source.s>');
    process.exit(1);
}
const output = source.replace(/\.[^./]*$/, '');
const name = path.basename(output);
const identifierSize = `SIGNATURE_IDENTIFIER_SIZE=${name.length + 1}`;

const buildDir = fs.mkdtempSync(path.join(path.dirname(output), `${name}.`));
try {
    // Assembled once to sign the code, then again with the signature included
    const prefix = path.join(buildDir, `${name}.prefix`);
    nasm('-D', identifierSize, source, '-o', prefix);
    const { signature, capacity } = adhocSignature(fs.readFileSync(prefix), name, PAGE_SIZE);
    const signatureFile = path.join(buildDir, 'signature.bin');
    fs.writeFileSync(signatureFile, padSignature(signature, capacity, capacity));

    const executable = path.join(buildDir, name);
    nasm('-D', identifierSize, '-D', `SIGNATURE_FILE="${signatureFile}"`, source, '-o', executable);
    fs.chmodSync(executable, fs.statSync(executable).mode | 0o100);
    fs.renameSync(executable, output);
} finally {
    fs.rmSync(buildDir, { recursive: true, force: true });
}
