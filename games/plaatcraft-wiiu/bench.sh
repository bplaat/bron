#!/bin/bash
# Builds the chunk pipeline benchmark and runs it headless in the Wii U toolchain's cemu-runner,
# then prints its results. cemu-runner interprets the CPU, so the results are far slower than
# the real Wii U, compare them only with each other.
set -euo pipefail

cd "$(dirname "$0")"
cargo build --release --features bench
../../tools/wiiu-toolchain/build.sh cemu
../../target/wiiu-toolchain/elf2rpl target/wiiu/release/plaatcraft-wiiu.elf target/wiiu/release/plaatcraft-wiiu.rpx >/dev/null
../../target/wiiu-toolchain/cemu-runner target/wiiu/release/plaatcraft-wiiu.rpx run 600 2>&1 |
    grep -o 'BENCH.*'
