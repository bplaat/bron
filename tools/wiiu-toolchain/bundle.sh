#!/bin/sh
# Bundles a program with its metadata into a .wuhb for the Wii U Menu with our wuhbtool. The
# metadata folder has meta.ini with the [menu] longname, shortname and author like in a .wuhb, and
# optionally icon.png (128x128), tv.png (1280x720) and drc.png (854x480) splash screens.
# Usage: ./bundle.sh <program.rpx> <meta folder> <output.wuhb>
set -eu

rpx=$1
meta=$2
output=$3
value() { tr -d '\r' <"$meta/meta.ini" | sed -n "s/^$1=//p"; }

set -- "$rpx" "$output" --name="$(value longname)" --short-name="$(value shortname)" \
    --author="$(value author)"
[ -f "$meta/icon.png" ] && set -- "$@" --icon="$meta/icon.png"
[ -f "$meta/tv.png" ] && set -- "$@" --tv-image="$meta/tv.png"
[ -f "$meta/drc.png" ] && set -- "$@" --drc-image="$meta/drc.png"
exec "$(dirname "$0")/../../target/wiiu-toolchain/wuhbtool" "$@"
