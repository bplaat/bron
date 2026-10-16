#!/bin/sh
# Starts a program in melonDS, set MELONDS to use a different melonDS. Usage: ./run.sh <program.nds>
set -eu

if [ -z "${MELONDS:-}" ]; then
    if [ "$(uname)" = Darwin ]; then
        MELONDS=/Applications/melonDS.app/Contents/MacOS/melonDS
    else
        MELONDS=melonDS
    fi
fi
exec "$MELONDS" "$(realpath "$1")"
