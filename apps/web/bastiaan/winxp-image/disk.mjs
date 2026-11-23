#!/usr/bin/env node
// Offline work on the FAT32 disk with mtools
// Usage: disk.mjs software <disk.img> - copy software/ in, software.cmd installs it at the next logon (QEMU)
//        disk.mjs setup <disk.img>    - copy the guest integration and music in, v86setup runs at logon (v86)
//        disk.mjs finish <disk.img>   - drop the setup files and caches, zero the free space
//        disk.mjs apps <disk.img>     - copy the freshly built native apps over the installed ones

import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

const root = path.dirname(new URL(import.meta.url).pathname);
const [mode, image] = process.argv.slice(2);
const fd = fs.openSync(image, 'r+');
const read = (offset, length) => {
    const buffer = Buffer.alloc(length);
    fs.readSync(fd, buffer, 0, length, offset);
    return buffer;
};
const part = read(0, 512).readUInt32LE(454) * 512;
const drive = `${image}@@${part}`;

// FAT32 layout, for zeroing free space and fixing names mtools gets wrong
const bpb = read(part, 512);
const sectorSize = bpb.readUInt16LE(0x0b);
const clusterSize = bpb[0x0d] * sectorSize;
const fatStart = part + bpb.readUInt16LE(0x0e) * sectorSize;
const fatSize = bpb.readUInt32LE(0x24) * sectorSize;
const dataStart = fatStart + bpb[0x10] * fatSize;
const clusterCount = Math.floor((part + bpb.readUInt32LE(0x20) * sectorSize - dataStart) / clusterSize);
const clusterOffset = (cluster) => dataStart + (cluster - 2) * clusterSize;
const nextCluster = (cluster) => read(fatStart + cluster * 4, 4).readUInt32LE(0) & 0x0fffffff;

// Folder entries as { name, cluster, slots }: slots are the offsets of the long name entries, in name order
function folderEntries(cluster) {
    const entries = [];
    let slots = [];
    for (; cluster >= 2 && cluster < 0x0ffffff8; cluster = nextCluster(cluster)) {
        const data = read(clusterOffset(cluster), clusterSize);
        for (let i = 0; i < clusterSize; i += 32) {
            if (data[i] === 0) return entries;
            if (data[i] === 0xe5) {
                slots = [];
            } else if (data[i + 11] === 0x0f) {
                slots[(data[i] & 0x1f) - 1] = clusterOffset(cluster) + i;
            } else {
                const chars = slots.flatMap((offset) => {
                    const entry = read(offset, 32);
                    return [
                        [1, 5],
                        [14, 6],
                        [28, 2],
                    ].flatMap(([at, n]) => Array.from({ length: n }, (_, j) => entry.readUInt16LE(at + j * 2)));
                });
                const end = chars.indexOf(0);
                const name = slots.length
                    ? String.fromCharCode(...(end < 0 ? chars : chars.slice(0, end)))
                    : data.toString('latin1', i, i + 8).trim() +
                      (data[i + 8] !== 0x20 ? '.' + data.toString('latin1', i + 8, i + 11).trim() : '');
                const first = (data.readUInt16LE(i + 20) << 16) | data.readUInt16LE(i + 26);
                entries.push({ name, cluster: first, slots });
                slots = [];
            }
        }
    }
    return entries;
}

// mtools turns ' into _ in long folder names: write the real name into the long name entries, the checksum in them
// only covers the short name
function fixFolderName(target) {
    const names = target.split('/').filter(Boolean);
    let cluster = bpb.readUInt32LE(0x2c);
    for (const [index, name] of names.entries()) {
        const entries = folderEntries(cluster);
        const last = index === names.length - 1;
        let entry = entries.find((entry) => entry.name.toLowerCase() === name.toLowerCase());
        if (!entry && last) {
            entry = entries.find((entry) => entry.name.toLowerCase() === name.replaceAll("'", '_').toLowerCase());
            if (!entry) return;
            entry.slots.forEach((offset, slot) => {
                const slotEntry = read(offset, 32);
                [
                    [1, 5],
                    [14, 6],
                    [28, 2],
                ].reduce((at, [field, n]) => {
                    for (let j = 0; j < n; j++, at++)
                        if (at < name.length) slotEntry.writeUInt16LE(name.charCodeAt(at), field + j * 2);
                    return at;
                }, slot * 13);
                fs.writeSync(fd, slotEntry, 0, 32, offset);
            });
            return;
        }
        if (!entry) return;
        cluster = entry.cluster;
    }
}
const mtools = (cmd, ...args) =>
    execFileSync(cmd, ['-i', drive, ...args], {
        stdio: ['ignore', 'ignore', 'inherit'],
        env: { ...process.env, MTOOLS_SKIP_CHECK: '1' },
    });
const tryMtools = (...args) => {
    try {
        mtools(...args);
    } catch {}
};
const copy = (source, target) => mtools('mcopy', '-o', '-Q', source, `::${target}`);

// Copies a host folder tree onto the disk without dotfiles, folder by folder so their names can be fixed, each folder's
// files in one mcopy call. keep leaves existing files as is.
function copyTree(source, target, { keep = false, skip = () => false } = {}) {
    if (target) tryMtools('mmd', `::${target}`);
    if (target.includes("'")) fixFolderName(target);
    const entries = fs
        .readdirSync(source, { withFileTypes: true })
        .filter((entry) => !entry.name.startsWith('.') && !skip(entry.name));
    const files = entries.filter((entry) => entry.isFile()).map((entry) => path.join(source, entry.name));
    // mcopy fails when it skips an existing file, which is intended with keep
    if (files.length)
        (keep ? tryMtools : mtools)('mcopy', '-Q', ...(keep ? ['-D', 's'] : ['-o']), ...files, `::${target}/`);
    for (const entry of entries.filter((entry) => entry.isDirectory())) {
        copyTree(path.join(source, entry.name), `${target}/${entry.name}`, { keep });
    }
}

const readLines = (file) => (fs.existsSync(file) ? fs.readFileSync(file, 'utf8').split('\n').filter(Boolean) : []);
const glob = (pattern) => new RegExp(`^${pattern.replace(/[.+^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*')}$`, 'i');

const V86 = '/WINDOWS/v86';
const MUSIC = '/Documents and Settings/Default User/My Documents/My Music';
// Media Player's per user setup removes its desktop shortcut at the first logon of bplaat, setup keeps it aside
const PLAYER_LINK = '/Documents and Settings/All Users/Desktop/Windows Media Player.lnk';

const guest = path.join(root, 'guest');
const copyGuest = (...names) => names.forEach((name) => copy(path.join(guest, name), `${V86}/${name}`));
// The guest programs and drivers, built by build.sh
const copyBuilt = (target, ...names) =>
    names.forEach((name) => copy(path.join(guest, 'target', target, 'release', name), `${V86}/${name}`));
// Runs a guest command once at the next logon, from the All Users Startup folder
function runAtLogon(name, command) {
    const file = path.join(path.dirname(image), name);
    fs.writeFileSync(file, `@${command}\r\n@del "%~f0"\r\n`);
    copy(file, `/Documents and Settings/All Users/Start Menu/Programs/Startup/${name}`);
    fs.rmSync(file);
}

// The Standard PC HAL (out/hal.dll, see build.sh), setup picked the APIC one which hangs on v86
const out = (name) => path.join(path.dirname(image), name);
const installHal = () => copy(out('hal.dll'), '/WINDOWS/system32/hal.dll');

if (mode === 'software') {
    installHal();

    // Setup also lists the boot sector of the FAT32 partition this build formatted as an unknown OS, drop it and the wait
    const bootIni = path.join(path.dirname(image), 'boot.ini');
    fs.writeFileSync(
        bootIni,
        [
            '[boot loader]',
            'timeout=0',
            'default=multi(0)disk(0)rdisk(0)partition(1)\\WINDOWS',
            '[operating systems]',
            'multi(0)disk(0)rdisk(0)partition(1)\\WINDOWS="Microsoft Windows XP Professional" /noexecute=optin /fastdetect',
            '',
        ].join('\r\n'),
    );
    tryMtools('mattrib', '-r', '-s', '-h', '::/boot.ini');
    copy(bootIni, '/boot.ini');
    mtools('mattrib', '+r', '+s', '+h', '::/boot.ini');
    fs.rmSync(bootIni);

    tryMtools('mmd', `::${V86}`);
    copyGuest('software.cmd', 'shortcut.vbs', 'userreg.cmd');

    // Every folder minus its CD contents and what its .diskignore lists, its built root/ goes onto C: as is
    // without replacing existing files, like setups keep newer system files
    tryMtools('mmd', `::${V86}/software`);
    const folders = fs
        .readdirSync(path.join(root, 'software'), { withFileTypes: true })
        .filter((entry) => entry.isDirectory());
    for (const { name } of folders) {
        const dir = path.join(root, 'software', name);
        if (fs.existsSync(path.join(dir, 'root'))) copyTree(path.join(dir, 'root'), '', { keep: true });
        const ignore = ['cd', 'root', ...readLines(path.join(dir, '.diskignore'))].map(glob);
        copyTree(dir, `${V86}/software/${name}`, { skip: (file) => ignore.some((pattern) => pattern.test(file)) });
    }
    runAtLogon('software.cmd', 'call "%SystemRoot%\\v86\\software.cmd"');
} else if (mode === 'setup') {
    installHal();
    copy(out('msgothic.ttc'), '/WINDOWS/Fonts/msgothic.ttc'); // registered and linked by v86setup
    tryMtools('mdeltree', `::${V86}`);
    tryMtools('mmd', `::${V86}`);
    copyGuest('v86vid.inf', 'activate.vbs', 'library.vbs', 'user.bmp');
    copyBuilt('i686-winxp-windows-gnu', 'v86agent.exe', 'v86setup.exe', 'confid.exe');
    copyBuilt('i686-winxp-windows-driver', 'v86mou.sys', 'v86tools.sys', 'v86vid.sys');

    // Music: new profiles start with the Default User's My Music
    tryMtools('mmd', `::${path.dirname(MUSIC)}`);
    tryMtools('mmd', `::${MUSIC}`);
    copyTree(path.join(root, 'music'), MUSIC);
    copyAlbumArt(path.join(root, 'music'), MUSIC);
    tryMtools('mcopy', '-n', `::${PLAYER_LINK}`, out('player.lnk'));
    runAtLogon('v86setup.cmd', '"%SystemRoot%\\v86\\v86setup.exe"');
} else if (mode === 'finish') {
    if (fs.existsSync(out('player.lnk'))) {
        copy(out('player.lnk'), PLAYER_LINK);
        fs.rmSync(out('player.lnk'));
    }
    tryMtools('mdeltree', `::${V86}`);
    for (const name of fs.readdirSync(path.join(root, 'music'))) tryMtools('mdeltree', `::${MUSIC}/${name}`);
    for (const dir of [
        '/WINDOWS/Prefetch',
        '/WINDOWS/Temp',
        '/WINDOWS/system32/dllcache',
        '/WINDOWS/Driver Cache/i386',
    ]) {
        tryMtools('mdeltree', `::${dir}`);
        tryMtools('mmd', `::${dir}`);
    }
    tryMtools('mdel', '::/pagefile.sys');
    tryMtools('mdel', '::/hiberfil.sys');
    zeroFree();
} else if (mode === 'apps') {
    // Their software/ folders install them with shortcuts and logon entries, a new build only replaces the exe and
    // 2048's MiniBlink
    const apps = { bastiaan: 'Bastiaan', reversi: 'Reversi', game2048: '2048', redsquare: 'RedSquare' };
    for (const [name, folder] of Object.entries(apps)) {
        copy(path.join(root, 'software', name, `${name}.exe`), `/Program Files/${folder}/${name}.exe`);
    }
    copy(path.join(root, 'software', 'game2048', 'miniblink.dll'), '/Program Files/2048/miniblink.dll');
} else {
    throw new Error('usage: disk.mjs software|setup|finish|apps <disk.img>');
}
fs.closeSync(fd);

// The JPEG cover in an MP3's ID3v2 tag (APIC frame), or null
function coverArt(file) {
    const data = fs.readFileSync(file);
    if (data.toString('latin1', 0, 3) !== 'ID3') return null;
    const version = data[3];
    const syncsafe = (at) => (data[at] << 21) | (data[at + 1] << 14) | (data[at + 2] << 7) | data[at + 3];
    const end = 10 + syncsafe(6);
    for (let at = 10; at + 10 < end;) {
        const id = data.toString('latin1', at, at + 4);
        const size = version >= 4 ? syncsafe(at + 4) : data.readUInt32BE(at + 4);
        if (!size || !/^[A-Z0-9]{4}$/.test(id)) break;
        const frame = data.subarray(at + 10, at + 10 + size);
        at += 10 + size;
        if (id !== 'APIC') continue;
        // Encoding, MIME type, picture type, description, then the image
        const mimeEnd = frame.indexOf(0, 1);
        const wide = frame[0] === 1 || frame[0] === 2;
        let descEnd = mimeEnd + 2;
        while (descEnd < frame.length && (frame[descEnd] || (wide && frame[descEnd + 1]))) descEnd += wide ? 2 : 1;
        const image = frame.subarray(descEnd + (wide ? 2 : 1));
        if (image[0] === 0xff && image[1] === 0xd8) return image;
    }
    return null;
}

// Media Player shows a folder's hidden Folder.jpg as its album art, taken from the first MP3 with a cover
function copyAlbumArt(source, target) {
    const entries = fs.readdirSync(source, { withFileTypes: true }).filter((entry) => !entry.name.startsWith('.'));
    const cover = entries
        .filter((entry) => entry.isFile() && /\.mp3$/i.test(entry.name))
        .map((entry) => coverArt(path.join(source, entry.name)))
        .find(Boolean);
    if (cover && !entries.some((entry) => entry.name.toLowerCase() === 'folder.jpg')) {
        const file = out('Folder.jpg');
        fs.writeFileSync(file, cover);
        copy(file, `${target}/Folder.jpg`);
        mtools('mattrib', '+h', `::${target}/Folder.jpg`);
        fs.rmSync(file);
    }
    for (const entry of entries.filter((entry) => entry.isDirectory())) {
        copyAlbumArt(path.join(source, entry.name), `${target}/${entry.name}`);
    }
}

// Zeroes the free clusters, so they compress to nothing
function zeroFree() {
    const fat = read(fatStart, fatSize);
    const zero = Buffer.alloc(clusterSize * 256);
    const free = (cluster) => !(fat.readUInt32LE(cluster * 4) & 0x0fffffff);
    let freed = 0;
    for (let cluster = 2; cluster < clusterCount + 2;) {
        let run = 0;
        while (run < 256 && cluster + run < clusterCount + 2 && free(cluster + run)) run++;
        if (run) fs.writeSync(fd, zero, 0, run * clusterSize, clusterOffset(cluster));
        freed += run;
        cluster += run || 1;
    }
    console.log(`zeroed ${((freed * clusterSize) / 2 ** 20).toFixed(0)} MiB of free space`);
}
