#!/usr/bin/env python3
"""Installs an old Microsoft game from its CD like its setup does, without running the setup: the setup program holds
its file table as NUL terminated pairs of CD path and destination, destinations use %APPPATH and %STRINGn from the
setup DLL's string table. Files from the CD's SYSTEM folder go to Windows, the disk copy keeps newer existing ones.
CABs on the CD are extracted separately.
Usage: ebu.py <setup.exe> <strings.dll> <cd dir> <C:\ root dir> <install dir relative to C:\>"""
import os
import shutil
import struct
import sys


def resources(path):
    """Yields (type, name, data) for every resource of a PE file"""
    data = open(path, 'rb').read()
    pe = struct.unpack_from('<I', data, 0x3C)[0]
    count, optional_size = struct.unpack_from('<H', data, pe + 6)[0], struct.unpack_from('<H', data, pe + 20)[0]
    sections = [struct.unpack_from('<IIII', data, pe + 24 + optional_size + i * 40 + 8) for i in range(count)]

    def offset(rva):
        return next(rva - va + raw for size, va, raw_size, raw in sections if va <= rva < va + max(size, raw_size))

    base = offset(struct.unpack_from('<I', data, pe + 24 + 96 + 2 * 8)[0])

    def entries(directory):
        n = sum(struct.unpack_from('<HH', data, directory + 12))
        return [struct.unpack_from('<II', data, directory + 16 + i * 8) for i in range(n)]

    for kind, types in entries(base):
        for name, names in entries(base + (types & 0x7FFFFFFF)):
            for _, leaf in entries(base + (names & 0x7FFFFFFF)):
                rva, size = struct.unpack_from('<II', data, base + leaf)
                yield kind, name, data[offset(rva):offset(rva) + size]


def string_table(path):
    """RT_STRING blocks hold 16 length prefixed UTF-16 strings each"""
    strings = {}
    for kind, block, data in resources(path):
        if kind != 6:
            continue
        pos = 0
        for i in range(16):
            length = struct.unpack_from('<H', data, pos)[0]
            strings[(block - 1) * 16 + i] = data[pos + 2:pos + 2 + length * 2].decode('utf-16le')
            pos += 2 + length * 2
    return strings


def main(setup, strings_dll, cd, root, app):
    strings = string_table(strings_dll)
    cd_files = {}
    for folder, _, files in os.walk(cd):
        for name in files:
            path = os.path.join(folder, name)
            cd_files[os.path.relpath(path, cd).replace('/', '\\').upper()] = path

    tokens = open(setup, 'rb').read().split(b'\0')
    tokens += b''.join(data for kind, _, data in resources(setup) if kind == 'SETUPBINARY').split(b'\0')
    pairs = [(s.decode('latin1').upper(), d.decode('latin1')) for s, d in zip(tokens, tokens[1:])]
    pairs = [(cd_files[s], d) for s, d in pairs if s in cd_files]

    # %STRINGn is resource string n plus a per setup base, the one under which every reference resolves
    refs = {int(r.split('%')[0].rstrip('\\')) for _, d in pairs for r in d.split('%STRING')[1:]}
    base = next(b for b in (0, 500, 1000) if all(strings.get(b + n) for n in refs))

    installed = {}
    for source, dest in pairs:
        for n in sorted({int(r.split('%')[0].rstrip('\\')) for r in dest.split('%STRING')[1:]}, reverse=True):
            dest = dest.replace(f'%STRING{n}', strings[base + n])
        if dest.upper().startswith('%APPPATH\\'):
            dest = dest[len('%APPPATH\\'):]
        if '%' in dest or ':' in dest or '.' not in dest or '@' in dest:
            continue  # setup variables and noise
        system = os.path.relpath(source, cd).upper().split(os.sep)
        if system[0] == 'DIRECTX':
            continue  # DirectX updates for Windows 9x, XP has newer
        if system[0] == 'SYSTEM':
            dest = 'WINDOWS\\' + ('Fonts' if system[1] == 'FONTS' else 'system32') + '\\' + dest
        else:
            dest = app.replace('/', '\\') + '\\' + dest
        installed[dest.lower()] = (dest, source)

    for dest, source in installed.values():
        path = os.path.join(root, *dest.split('\\'))
        os.makedirs(os.path.dirname(path), exist_ok=True)
        shutil.copyfile(source, path)
    print(f'{len(installed)} files from {setup}')


if __name__ == '__main__':
    main(*sys.argv[1:])
