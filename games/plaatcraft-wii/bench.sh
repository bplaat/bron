#!/bin/bash
# Builds the chunk pipeline benchmark and runs it headless in the Wii toolchain's dolphin-runner,
# then prints the results it writes to the debug UART. Dolphin's time base counts emulated
# cycles, so the results are repeatable but approximate the real Wii.
set -euo pipefail

cd "$(dirname "$0")"
cargo build --release --features bench
../../tools/wii-toolchain/build.sh dolphin
# The runner writes the UART to stderr, the benchmark has ten emulated minutes
../../target/wii-toolchain/dolphin-runner target/wii/release/plaatcraft-wii.elf 'await BENCH 36000' 2>&1 |
    grep -o 'BENCH.*'
