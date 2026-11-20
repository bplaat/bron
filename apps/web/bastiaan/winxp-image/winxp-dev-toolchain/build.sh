#!/bin/sh
# Cross-build the programs that run inside 32-bit Windows XP.
set -eu
cd "$(dirname "$0")"
root=$(cd ../../../../.. && pwd)
output="$root/target/winxp-dev-toolchain"
mkdir -p "$output"
docker build -q -t bron-winxp-dev-toolchain . >/dev/null
docker run --rm -v "$PWD:/scripts:ro" -v "$output:/build" -w /build \
    bron-winxp-dev-toolchain /scripts/build-tools.sh
