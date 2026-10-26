#!/bin/bash
# Builds the chunk pipeline benchmark and runs it in Dolphin without its user interface,
# then prints the results it writes to the debug UART. Dolphin's time base counts emulated
# cycles, so the results are repeatable but approximate the real Wii.
set -euo pipefail

cd "$(dirname "$0")"
user=target/wii/bench-dolphin
log="$user/Logs/dolphin.log"
options=(--batch --video_backend=Null)
# The Qt interface hangs without a GPU, so Linux uses the headless build
nogui=$(command -v dolphin-emu-nogui || echo /usr/games/dolphin-emu-nogui)
if [ -z "${DOLPHIN:-}" ] && [ -x "$nogui" ]; then
    export DOLPHIN="$nogui"
    options=(--platform=headless --video_backend=Null)
fi

rm -f "$log"
cargo build --release --features bench
# Without these a new user folder waits on dialogs, the UART goes to the log file and
# emulation runs as fast as it can
mkdir -p "$user/Config"
printf '[Analytics]\nPermissionAsked = True\n[Interface]\nUsePanicHandlers = False\n[Core]\nEmulationSpeed = 0.0\n' \
    > "$user/Config/Dolphin.ini"
printf '[Logs]\nOSREPORT = True\nOSREPORT_HLE = True\n[Options]\nVerbosity = 5\nWriteToFile = True\n' \
    > "$user/Config/Logger.ini"
../../tools/wii-toolchain/run.sh target/wii/release/plaatcraft-wii.elf "${options[@]}" \
    --user="$(realpath "$user")" > /dev/null 2>&1 &
pid=$!
# Wait at most five minutes for the results
for _ in $(seq 600); do
    grep -qs 'BENCH' "$log" && break
    kill -0 "$pid" 2> /dev/null || break
    sleep 0.5
done
# Dolphin ignores the first SIGTERM
kill -KILL "$pid" 2> /dev/null || true
wait "$pid" 2> /dev/null || true
grep -o 'BENCH.*' "$log" || { echo "The benchmark didn't finish" >&2; exit 1; }
