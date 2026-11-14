// First v86 boot: v86setup.exe sets Windows up at the Administrator logon and reboots into the bplaat account, then
// the guest powers off and its changes are written back to the image
// Usage: node setup.mjs <disk.img>
import fs from 'node:fs';
import path from 'node:path';
import { boot, disk, logSerial, screenshot, waitFor, waitIdle } from './lib.mjs';

const image = path.resolve(process.argv[2]);
const { emulator, host } = await boot({ url: image, async: true, size: fs.statSync(image).size });
logSerial(emulator);

host.setSize(1024, 768);
await waitFor(emulator, 'agent', () => host.agentAlive, 90);
await waitIdle(emulator, 120); // let the first logon of the new account settle
screenshot(emulator, path.resolve('out/setup.png'));
host.shutdown();
await waitFor(emulator, 'logoff', () => !host.agentAlive, 10);
await waitIdle(emulator, 60);

const buffer = disk(emulator);
const fd = fs.openSync(image, 'r+');
for (const index of buffer.block_cache_is_write) {
    const block = buffer.block_cache.get(index);
    fs.writeSync(fd, block, 0, block.length, index * block.length);
}
fs.closeSync(fd);
console.log(`wrote ${buffer.block_cache_is_write.size} blocks back to ${image}`);
emulator.destroy();
process.exit(0);
