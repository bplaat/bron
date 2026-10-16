#!/bin/sh
set -eu
cd "$(dirname "$0")"

nasm -f bin redsquare.asm -o redsquare-x86.exe
nasm -DWIN64 -f bin redsquare.asm -o redsquare-x64.exe
