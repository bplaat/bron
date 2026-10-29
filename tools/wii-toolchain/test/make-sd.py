#!/usr/bin/env python3
"""Create a disposable 128 MiB FAT32 superfloppy image for Dolphin tests."""
import math
from pathlib import Path
import struct
import sys

path = Path(sys.argv[1])
if path.exists():
    raise SystemExit(f"Refusing to replace an existing SD image: {path}")
path.parent.mkdir(parents=True, exist_ok=True)
sectors, reserved, fat_count = 262144, 32, 2
fat_sectors = 1
while True:
    clusters = sectors - reserved - fat_count * fat_sectors
    needed = math.ceil((clusters + 2) * 4 / 512)
    if needed <= fat_sectors:
        break
    fat_sectors = needed
clusters = sectors - reserved - fat_count * fat_sectors
boot = bytearray(512)
boot[:11] = b"\xeb\x58\x90MSWIN4.1"
struct.pack_into("<HBHBHHBHHHII", boot, 11, 512, 1, reserved, fat_count, 0, 0, 0xF8, 0, 63, 255, 0, sectors)
struct.pack_into("<IHHIHH", boot, 36, fat_sectors, 0, 0, 2, 1, 6)
boot[64], boot[66] = 0x80, 0x29
struct.pack_into("<I", boot, 67, 0x504C4353)
boot[71:82], boot[82:90], boot[510:512] = b"WII TEST   ", b"FAT32   ", b"\x55\xaa"
info = bytearray(512)
for offset, value in [(0, 0x41615252), (484, 0x61417272), (488, clusters - 1), (492, 3), (508, 0xAA550000)]:
    struct.pack_into("<I", info, offset, value)
with path.open("xb") as disk:
    disk.truncate(sectors * 512)
    for sector, data in [(0, boot), (1, info), (6, boot), (7, info)]:
        disk.seek(sector * 512)
        disk.write(data)
    for index in range(fat_count):
        disk.seek((reserved + index * fat_sectors) * 512)
        disk.write(struct.pack("<III", 0x0FFFFFF8, 0xFFFFFFFF, 0x0FFFFFFF))
print(f"Created FAT32 test SD card: {path}")
