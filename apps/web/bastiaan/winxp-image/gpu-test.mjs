#!/usr/bin/env node
// The conformance test of the paravirtualized graphics adapter (guest/src/bin/v86gputest): boots a copy of the site's
// disk headless with the test at logon, checks the pictures the guest asks to be shown and fails on any failed case
// Usage: gpu-test.mjs [disk.img], out/site.img by default

import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { boot, screenshot, sleep, visiblePixel } from './lib.mjs';

const root = path.dirname(new URL(import.meta.url).pathname);
const source = path.resolve(process.argv[2] ?? path.join(root, 'out/site.img'));
const image = path.join(root, 'out/test.img');
const TIMEOUT_MINUTES = 20;
// 16 bpp loses the low bits of each channel
const TOLERANCE = 8;

// A copy-on-write clone where the file system can
fs.copyFileSync(source, image, fs.constants.COPYFILE_FICLONE);
execFileSync(path.join(root, 'disk.mjs'), ['test', image], { stdio: 'inherit' });

const { emulator, host } = await boot({ url: image, async: true, size: fs.statSync(image).size });
host.setSize(1024, 768);
const vga = emulator.v86.cpu.devices.vga;

let failed = 0;
let done = null;
let line = '';
emulator.add_listener('serial0-output-byte', (byte) => {
    if (byte !== 10 && byte !== 13) return void (line += String.fromCharCode(byte));
    const text = line.trim();
    line = '';
    if (text.startsWith('LOG ')) console.log(`guest: ${text.slice(4)}`);
    // The guest waits in its write, so the screen is what the guest just showed
    const check = text.match(/^CHECK-SCREEN (\d+) (\d+) ([0-9a-f]{6}) (.*)$/);
    if (check) {
        const [x, y, expected] = [Number(check[1]), Number(check[2]), parseInt(check[3], 16)];
        const shown = visiblePixel(vga, x, y);
        const close = [16, 8, 0].every(
            (shift) => Math.abs(((shown >> shift) & 0xff) - ((expected >> shift) & 0xff)) <= TOLERANCE,
        );
        const hex = (color) => color.toString(16).padStart(6, '0');
        if (close) {
            console.log(`page: ok ${check[4]} shows ${hex(shown)}`);
        } else {
            failed++;
            console.log(`page: FAIL ${check[4]} shows ${hex(shown)}, expected ${hex(expected)}`);
            screenshot(emulator, path.join(root, `out/gpu-test-${check[4].replaceAll(' ', '-')}.png`));
        }
    }
    const result = text.match(/^GPUTEST DONE (\d+) (\d+)$/);
    if (result) done = { passed: Number(result[1]), failed: Number(result[2]) };
});

const start = Date.now();
while (!done && Date.now() - start < TIMEOUT_MINUTES * 60 * 1000) await sleep(1000);
screenshot(emulator, path.join(root, 'out/gpu-test.png'));
const { commands, declined } = host.gpu;
emulator.destroy();
fs.rmSync(image);

if (!done) {
    console.error(`no result after ${TIMEOUT_MINUTES} minutes, see out/gpu-test.png`);
    process.exit(1);
}
console.log(
    `${done.passed} passed, ${done.failed + failed} failed, ${commands} display commands of which ${declined} declined`,
);
process.exit(done.failed + failed > 0 ? 1 : 0);
