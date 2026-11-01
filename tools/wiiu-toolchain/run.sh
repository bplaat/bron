#!/bin/sh
# Converts a program to an RPX file with elf2rpl and starts it in Cemu, set CEMU to use a
# different Cemu. Usage: ./run.sh <program.elf> [Cemu options]...
set -eu

elf=$(realpath "$1")
shift
rpx="${elf%.elf}.rpx"
"$(dirname "$0")/../../target/wiiu-toolchain/elf2rpl" "$elf" "$rpx" > /dev/null

if [ -z "${CEMU:-}" ]; then
    if [ "$(uname)" = Darwin ]; then
        CEMU=/Applications/Cemu.app/Contents/MacOS/Cemu
    else
        CEMU=cemu
    fi
fi
exec "$CEMU" "$@" -g "$rpx"
