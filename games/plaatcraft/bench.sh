#!/bin/bash
# Builds the chunk pipeline benchmark and runs it headless in an emulator, then prints its
# results. Usage: ./bench.sh <wii|wiiu>
# - wii: Dolphin's time base counts emulated cycles, so the results are repeatable but
#   approximate the real Wii.
# - wiiu: cemu-runner interprets the CPU, so the results are far slower than the real Wii U,
#   compare them only with each other.
set -euo pipefail

console=${1:-}
if [[ $console != wii && $console != wiiu ]]; then
    echo "usage: $0 <wii|wiiu>" >&2
    exit 1
fi
cd "$(dirname "$0")"
root=../..
build=$root/target/$console-toolchain
elf=$root/target/powerpc-nintendo-$console/release/plaatcraft.elf
"$root/tools/$console-toolchain/bin/cargo" build --release --locked --features bench
case $console in
wii)
    "$root/tools/wii-toolchain/build.sh" dolphin
    # The runner writes the UART to stderr, the benchmark has ten emulated minutes
    "$build/bin/dolphin-runner" "$elf" 'await BENCH 36000' 2>&1 | grep -o 'BENCH.*'
    ;;
wiiu)
    "$root/tools/wiiu-toolchain/build.sh" cemu
    "$build/bin/elf2rpl" "$elf" "${elf%.elf}.rpx" >/dev/null
    "$build/bin/cemu-runner" "${elf%.elf}.rpx" run 600 2>&1 | grep -o 'BENCH.*'
    ;;
esac
