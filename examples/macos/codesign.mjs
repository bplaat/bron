// Ad-hoc code signatures for the Mach-O executables NASM assembles, which reserve room for them with an
// LC_CODE_SIGNATURE load command

import { execFileSync } from 'node:child_process';
import crypto from 'node:crypto';

const LC_CODE_SIGNATURE = 0x1d;
const LC_SEGMENT_64 = 0x19;
const MH_MAGIC_64 = 0xfeedfacf;
const CSMAGIC_CODEDIRECTORY = 0xfade0c02;
const CSMAGIC_EMBEDDED_SIGNATURE = 0xfade0cc0;
const CS_ADHOC = 0x00002;
const CS_LINKER_SIGNED = 0x20000;
const CS_EXECSEG_MAIN_BINARY = 0x1;
const CS_HASHTYPE_SHA256 = 2;
const PLATFORM_MACOS = 1;
const HASH_SIZE = 32;
const CODE_DIRECTORY_HEADER_SIZE = 88;
const SUPERBLOB_HEADER_SIZE = 20;

export const nasm = (...args) => execFileSync('nasm', ['-f', 'bin', ...args], { stdio: 'inherit' });

// The signed code size and reserved signature size from LC_CODE_SIGNATURE, and the __TEXT segment size
function readLayout(macho) {
    if (macho.readUInt32LE(0) !== MH_MAGIC_64) throw new Error('input is not a 64-bit Mach-O executable');
    let codeLimit, capacity, executableSize;
    for (let i = 0, offset = 32; i < macho.readUInt32LE(16); i++, offset += macho.readUInt32LE(offset + 4)) {
        const command = macho.readUInt32LE(offset);
        if (command === LC_CODE_SIGNATURE) {
            [codeLimit, capacity] = [macho.readUInt32LE(offset + 8), macho.readUInt32LE(offset + 12)];
        } else if (command === LC_SEGMENT_64 && macho.toString('latin1', offset + 8, offset + 24).replace(/\0+$/, '') === '__TEXT') {
            executableSize = macho.readBigUInt64LE(offset + 48);
        }
    }
    if (codeLimit === undefined || executableSize === undefined) {
        throw new Error('Mach-O signature or __TEXT layout is missing');
    }
    return { codeLimit, capacity, executableSize };
}

// Returns the unpadded embedded signature and the space the Mach-O reserves for it
export function adhocSignature(macho, name, pageSize) {
    const { codeLimit, capacity, executableSize } = readLayout(macho);
    const identifier = Buffer.from(`${name}\0`, 'ascii');
    const hashes = [];
    for (let offset = 0; offset < codeLimit; offset += pageSize) {
        hashes.push(crypto.createHash('sha256').update(macho.subarray(offset, Math.min(offset + pageSize, codeLimit))).digest());
    }
    const hashOffset = CODE_DIRECTORY_HEADER_SIZE + identifier.length;

    const header = Buffer.alloc(CODE_DIRECTORY_HEADER_SIZE);
    [
        CSMAGIC_CODEDIRECTORY,
        hashOffset + hashes.length * HASH_SIZE,
        0x20400, // version
        CS_ADHOC | CS_LINKER_SIGNED,
        hashOffset,
        CODE_DIRECTORY_HEADER_SIZE, // identifier offset
        0, // special slots
        hashes.length,
        codeLimit,
    ].forEach((value, i) => header.writeUInt32BE(value, i * 4));
    header.set([HASH_SIZE, CS_HASHTYPE_SHA256, PLATFORM_MACOS, Math.log2(pageSize)], 36);
    // Spare, scatter, team and spare fields, the 64-bit code limit and executable segment base stay zero
    header.writeBigUInt64BE(executableSize, 72);
    header.writeBigUInt64BE(BigInt(CS_EXECSEG_MAIN_BINARY), 80);
    const codeDirectory = Buffer.concat([header, identifier, ...hashes]);

    const superblob = Buffer.alloc(SUPERBLOB_HEADER_SIZE);
    [CSMAGIC_EMBEDDED_SIGNATURE, SUPERBLOB_HEADER_SIZE + codeDirectory.length, 1, 0, SUPERBLOB_HEADER_SIZE].forEach(
        (value, i) => superblob.writeUInt32BE(value, i * 4),
    );
    return { signature: Buffer.concat([superblob, codeDirectory]), capacity };
}

// Zero pads a signature to at least size bytes, which must fit in the reserved space
export function padSignature(signature, size, capacity) {
    const needed = Math.max(signature.length, size);
    if (needed > capacity) throw new Error(`signature needs ${needed} bytes, Mach-O reserves ${capacity}`);
    return Buffer.concat([signature, Buffer.alloc(needed - signature.length)]);
}
