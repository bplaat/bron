#!/bin/sh
set -eu
cd "$(dirname "$0")"

for source in *.asm; do
    name=${source%.asm}
    nasm -f bin "$source" -o "$name-x86.exe"
    nasm -DWIN64 -f bin "$source" -o "$name-x64.exe"
done

sh redsquare/build.sh
