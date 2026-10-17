# Copyright (c) 2026 Bastiaan van der Plaat
# SPDX-License-Identifier: MIT

"""Generate interoperability fixtures using Python's independent ZIP writer."""

import io
import struct
import zipfile
from pathlib import Path

ROOT = Path(__file__).parent
DATE = (2020, 2, 29, 12, 34, 56)


def info(name, mode, timestamp=None):
    entry = zipfile.ZipInfo(name, DATE)
    entry.create_system = 3
    entry.external_attr = mode << 16
    if timestamp is not None:
        entry.extra = struct.pack('<HHBi', 0x5455, 5, 1, timestamp)
    return entry


with zipfile.ZipFile(ROOT / 'unix.zip', 'w') as archive:
    archive.writestr(info('bin/', 0o40500, 1700000001), b'')
    archive.writestr(info('bin/run.sh', 0o104755, 1700000002), b'#!/bin/sh\nexit 0\n')
    archive.writestr(info('shortcut', 0o120777, 1700000003), b'bin/run.sh')
    archive.writestr(info('chain', 0o120777, 1700000004), b'shortcut')
    archive.writestr(info('bin/parent-link', 0o120777), b'../shortcut')

with zipfile.ZipFile(ROOT / 'dos.zip', 'w') as archive:
    archive.writestr(info('file', 0o100644), b'DOS timestamp')

with zipfile.ZipFile(ROOT / 'ntfs.zip', 'w') as archive:
    entry = info('file', 0o100644)
    ticks = 116444736000000000 + 1700000000 * 10000000 + 1234567
    entry.extra = struct.pack('<HHIHHQQQ', 0x000a, 32, 0, 1, 24, ticks, ticks, ticks)
    archive.writestr(entry, b'NTFS timestamp')

for filename, entries in {
    'escape-link.zip': [('link', b'../outside')],
    'absolute-link.zip': [('link', b'/etc/passwd')],
    'cycle-links.zip': [('a', b'b'), ('b', b'a')],
    'existing-link-target.zip': [('link', b'outside/file')],
}.items():
    with zipfile.ZipFile(ROOT / filename, 'w') as archive:
        for name, target in entries:
            archive.writestr(info(name, 0o120777), target)

with zipfile.ZipFile(ROOT / 'special.zip', 'w') as archive:
    archive.writestr(info('fifo', 0o10644), b'')


class Unseekable(io.BytesIO):
    def seekable(self):
        return False

    def seek(self, *args):
        raise io.UnsupportedOperation('unseekable')


stream = Unseekable()
with zipfile.ZipFile(stream, 'w', compression=zipfile.ZIP_DEFLATED) as archive:
    entry = info('streamed.txt', 0o100644, 1700000000)
    entry.compress_type = zipfile.ZIP_DEFLATED
    archive.writestr(entry, b'streamed data' * 100)
(ROOT / 'descriptor.zip').write_bytes(stream.getvalue())
