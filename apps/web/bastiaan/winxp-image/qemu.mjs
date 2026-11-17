// The QEMU machine for the install stages: a Standard PC like v86 (no ACPI, no APIC), so setup picks the matching HAL,
// QEMU's translation is much faster than v86. The guest talks back over COM1, lines go to onLine.
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import path from 'node:path';
import { MACHINE } from '../web/src/machine.ts';
import { sleep } from './lib.mjs';

async function connect(file) {
    for (let i = 0; i < 50; i++) {
        await sleep(200);
        const socket = await new Promise((resolve) => {
            const s = net.connect(file, () => resolve(s)).on('error', () => resolve(null));
        });
        if (socket) return socket;
    }
    throw new Error(`no QEMU socket ${file}`);
}

export async function startQemu({ image, cdrom, floppy, bootCd = false, onLine = () => {} }) {
    const out = path.dirname(image);
    const monitorFile = path.join(out, 'monitor.sock');
    const serialFile = path.join(out, 'serial.sock');
    fs.rmSync(monitorFile, { force: true });
    fs.rmSync(serialFile, { force: true });
    const qemu = spawn('qemu-system-i386', [
        '-machine', 'pc,acpi=off,hpet=off,vmport=off',
        '-accel', 'tcg,thread=single,tb-size=512',
        '-cpu', 'pentium3', '-smp', '1', '-m', `${MACHINE.memory_size / 2 ** 20}M`, // the same RAM as v86
        '-vga', 'std', '-nic', 'none', '-display', 'none',
        '-rtc', 'base=localtime',
        '-drive', `file=${image},format=raw,if=ide,index=0,cache=unsafe`, // safe: every stage ends with a clean shutdown
        '-drive', `${cdrom ? `file=${cdrom},file.locking=off,` : ''}media=cdrom,if=ide,index=2,readonly=on`,
        ...(floppy ? ['-drive', `file=${floppy},format=raw,if=floppy,readonly=on`] : []),
        '-boot', bootCd ? 'order=c,once=d,menu=off' : 'order=c',
        '-serial', `unix:${serialFile},server=on,wait=off`,
        '-monitor', `unix:${monitorFile},server=on,wait=off`,
        '-monitor', `unix:${path.join(out, 'debug.sock')},server=on,wait=off`, // for looking in by hand
    ], { stdio: 'inherit' });
    const exited = new Promise((resolve) => qemu.on('exit', resolve));

    const monitor = await connect(monitorFile);
    let reply = '';
    monitor.on('data', (data) => (reply += data));
    const command = async (line) => {
        reply = '';
        monitor.write(line + '\n');
        await sleep(300);
        return reply;
    };

    const serial = await connect(serialFile);
    let buffer = '';
    serial.on('data', (data) => {
        buffer += data.toString('latin1');
        const lines = buffer.split(/\r?\n/);
        buffer = lines.pop();
        for (const line of lines) if (line.trim()) onLine(line.trim());
    });

    return {
        command,
        exited,
        running: () => qemu.exitCode === null,
        screenshot: (file) => command(`screendump ${file}`),
        insertCd: (iso) => command(`change ide1-cd0 ${iso}`),
        // Windows powers down to "It is now safe to turn off your computer": quit once the disk stays quiet
        async quitWhenIdle() {
            let writes = '';
            for (let quiet = 0; quiet < 6; ) {
                await sleep(10 * 1000);
                const stats = (await command('info blockstats')).match(/wr_operations=\d+/g)?.join() ?? '';
                quiet = stats === writes ? quiet + 1 : 0;
                writes = stats;
            }
            await command('quit');
            await exited;
        },
    };
}
