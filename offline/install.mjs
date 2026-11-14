// Installs Windows XP unattended in QEMU onto a fresh FAT32 disk
// Usage: node install.mjs <xp.iso> <product key file> <mbr.bin> <disk.img>
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { sleep } from './lib.mjs';
import { startQemu } from './qemu.mjs';

const DISK_SIZE = 8 * 1024 ** 3; // room to add software later, free space streams as nothing
const PART_START = 63; // sectors, the classic XP layout
const HEADS = 255; // the LBA translation SeaBIOS picks for this disk size

const [iso, keyFile, mbrFile, image] = process.argv.slice(2).map((file) => path.resolve(file));
const out = path.dirname(image);
const run = (cmd, ...args) => execFileSync(cmd, args, { stdio: 'inherit' });

// Disk: MBR boot code and one active FAT32 (LBA) partition over the whole disk, formatted here so setup leaves it alone
const sectors = DISK_SIZE / 512 - PART_START;
const mbr = Buffer.alloc(512);
fs.readFileSync(mbrFile).copy(mbr, 0, 0, 440);
mbr.set([0x80, 1, 1, 0, 0x0c, 0xfe, 0xff, 0xff], 446);
mbr.writeUInt32LE(PART_START, 454);
mbr.writeUInt32LE(sectors, 458);
mbr.writeUInt16LE(0xaa55, 510);
fs.writeFileSync(image, mbr);
fs.truncateSync(image, DISK_SIZE);
run('mformat', '-i', `${image}@@${PART_START * 512}`, '-F', '-T', String(sectors), '-h', String(HEADS), '-s', '63',
    '-H', String(PART_START), '-v', 'WINXP', '::');

// Answer floppy: CD setup reads WINNT.SIF from A:
const sif = path.join(out, 'WINNT.SIF');
const key = fs.readFileSync(keyFile, 'utf8').trim();
const template = fs.readFileSync(new URL('winnt.sif', import.meta.url), 'utf8');
fs.writeFileSync(sif, template.replaceAll('{{PRODUCT_KEY}}', key).replace(/\r?\n/g, '\r\n'));
const floppy = path.join(out, 'floppy.img');
fs.rmSync(floppy, { force: true });
run('mformat', '-i', floppy, '-C', '-f', '1440', '::');
run('mcopy', '-i', floppy, sif, '::WINNT.SIF');
fs.rmSync(sif);

// The GUI setup's first logon reports back over COM1
let complete = false;
const qemu = await startQemu({
    image,
    cdrom: iso,
    floppy,
    bootCd: true,
    onLine: (line) => (complete ||= line.includes('WINXP_SETUP_COMPLETE')),
});

// The CD asks for a key because the disk is bootable
for (let i = 0; i < 15; i++) {
    await qemu.command('sendkey ret');
    await sleep(1000);
}

function setupError() {
    try {
        const log = execFileSync('mtype', ['-i', `${image}@@${PART_START * 512}`, '::/WINDOWS/setuperr.log'], {
            env: { ...process.env, MTOOLS_SKIP_CHECK: '1' },
            stdio: ['ignore', 'pipe', 'ignore'],
        });
        return log.toString('latin1').replace(/[\0\r]/g, '').match(/^[^\n]*(invalid|missing)[^\n]*$/im)?.[0].trim();
    } catch {
        return null; // not written yet
    }
}

const start = Date.now();
const elapsed = () => `${Math.round((Date.now() - start) / 60000)}m`;
let screens = 0;
while (!complete) {
    if (!qemu.running()) throw new Error('QEMU exited during setup');
    if (Date.now() - start > 4 * 60 * 60 * 1000) throw new Error('setup timed out, see out/install.ppm');
    if (++screens % 6 === 0) {
        await qemu.screenshot(path.join(out, 'install.ppm'));
        console.log(`installing... ${elapsed()}`);
        // A bad answer file stops GUI setup at a dialog, setup logs why
        if (setupError()) {
            await qemu.command('quit');
            throw new Error(`setup failed: ${setupError()}, see out/install.ppm`);
        }
    }
    await sleep(10 * 1000);
}
console.log(`setup complete after ${elapsed()}, waiting for the shutdown`);

await qemu.quitWhenIdle();
fs.rmSync(floppy);
console.log(`installed ${image}`);
process.exit(0);
