#!/bin/sh
# Converts a program to a DOL file with elf2dol and starts it in Dolphin, set DOLPHIN to use a
# different Dolphin. Usage: ./run.sh <program.elf> [Dolphin options]...
set -eu

elf=$(realpath "$1")
shift
dol="${elf%.elf}.dol"
"$(dirname "$0")/../../target/wii-toolchain/elf2dol" "$elf" "$dol"

if [ -z "${DOLPHIN:-}" ]; then
    if [ "$(uname)" = Darwin ]; then
        DOLPHIN=/Applications/Dolphin.app/Contents/MacOS/Dolphin
    elif command -v dolphin-emu > /dev/null; then
        DOLPHIN=dolphin-emu
    else
        # Debian and Ubuntu install it in /usr/games, which isn't always in PATH
        DOLPHIN=/usr/games/dolphin-emu
    fi
fi
exec "$DOLPHIN" "$@" "--exec=$dol"
