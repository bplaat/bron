#!/usr/bin/env node
// Installs the software in QEMU: guest/software.cmd runs every software/*/install.cmd at the Administrator logon and
// powers off. Install scripts can print progress over COM1 with "LOG <text>".
// Usage: software.mjs <disk.img>

import path from 'node:path';
import { sleep } from './lib.mjs';
import { startQemu } from './qemu.mjs';

const image = path.resolve(process.argv[2]);
const out = path.dirname(image);
let done = false;
const qemu = await startQemu({
    image,
    onLine(line) {
        if (line.startsWith('LOG ')) console.log(`guest: ${line.slice(4)}`);
        if (line === 'DONE') done = true;
    },
});

const start = Date.now();
while (!done) {
    if (!qemu.running()) throw new Error('QEMU exited while installing the software');
    if (Date.now() - start > 3 * 60 * 60 * 1000) throw new Error('software install timed out, see out/software.ppm');
    await qemu.screenshot(path.join(out, 'software.ppm'));
    await sleep(10 * 1000);
}
console.log(`software installed after ${Math.round((Date.now() - start) / 60000)}m`);
await qemu.quitWhenIdle();
process.exit(0);
