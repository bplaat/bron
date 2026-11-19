# Wii Toolchain

Everything the [Wii examples](../../examples/wii/) share to build Wii programs with plain clang and lld, without devkitPro.
`build.sh` builds the toolchain in the repository root's `target/wii-toolchain/`. The C
Makefiles and the Rust Cargo wrapper invoke it before building a project.

- Clones upstream [libogc](https://github.com/devkitPro/libogc) v2.11.0 and applies
  `patches/libogc/` as commits, then builds the parts we use (listed in `build.sh`) to
  `../../target/wii-toolchain/lib/libogc.a`, including the official USB mouse and boot-keyboard
  drivers. `libogc-sys` exposes their event APIs, USB device enumeration, interrupt/control
  transfers, and HID report-descriptor access
- Clones [picolibc](https://github.com/picolibc/picolibc) 1.8.12 and applies `patches/picolibc/`,
  then builds it with meson as the C library, its headers and `libc.a` with the math library go
  to `../../target/wii-toolchain/sysroot/`
- Clones devkitPro's [libdvm](https://github.com/devkitPro/libdvm) v2.1.0 and applies
  `patches/libdvm/`, then builds it with [fatfs-mod](https://github.com/devkitPro/fatfs-mod)
  v0.15.3 to `../../target/wii-toolchain/lib/libdvm.a`, the FAT driver for SD card files
- Clones devkitPro's [gamecube-tools](https://github.com/devkitPro/gamecube-tools) v1.0.7 and
  builds `elf2dol` (`.elf` to `.dol`) and `gcdsptool`, which assembles libogc's AESND mixer for
  the DSP that the audio of `libogc.a` loads
- Builds our [gxtexconv](crates/gxtexconv) with Cargo to `../../target/wii-toolchain/bin/gxtexconv`
- Checks out only the builtins of LLVM's [compiler-rt](https://github.com/llvm/llvm-project/tree/main/compiler-rt/lib/builtins)
  llvmorg-23.1.3 and builds the 64-bit integer math and conversions the PowerPC 750 can't do in
  instructions to `../../target/wii-toolchain/lib/libcompiler-rt.a`, the C examples link it (Rust has its own)

```sh
cd tools/wii-toolchain
./build.sh                 # fetch, patch and build
./build.sh std             # prepare the patched Rust sources
./build.sh clean           # remove ../../target/wii-toolchain/
./build.sh export-patches  # write the commits in ../../target/wii-toolchain/src/<name>/ back to patches/<name>/
./build.sh test            # run the C and Rust guest suites headlessly in Dolphin
./build.sh dolphin         # build ../../target/wii-toolchain/bin/dolphin-runner
```

All code is compiled for the PowerPC 750 (`-mcpu=750`) and optimized for size with `-Os`, like
the Rust examples (`opt-level = "s"`). LTO doesn't work with libogc: it waits for interrupts in
loops on plain variables, which LTO can optimize into endless loops. Code is compiled with
`_DEFAULT_SOURCE`, so picolibc also declares its POSIX and BSD functions under `-std=c23`.

## Integration

Install the host dependencies listed in the [examples README](../../examples/wii/README.md#getting-started).
Downloaded sources, compiled libraries and host tools stay in the repository root's ignored `target/wii-toolchain/` folder:
upstream sources in `src/`, libraries in `lib/`, host tools in `bin/`, intermediate files in `build/` and picolibc in `sysroot/`.

C projects include `wii.mk` from their Makefile. Rust projects use the toolchain's `bin/cargo`
wrapper and access libogc and picolibc through `crates/libogc-sys`. Each project keeps its build
output in `target/`.

The toolchain and the examples build separately from the main Cargo workspace, the
[PlaatCraft Wii port](../../games/plaatcraft/docs/wii.md) builds in it.

## Rust standard library

Use the Cargo wrapper from a Rust project with an ordinary `fn main()`. It builds the native
runtime, copies `rust-src` for the pinned upstream nightly, applies the toolchain's Rust patches,
and builds `std` with the `powerpc-nintendo-wii` target. The wrapper supplies the linker script,
native archives, Dolphin runner, and the Wii's build profile: optimized for size, aborting panics,
and no LTO, also in the Cargo workspace (its per-package optimization levels still apply).

```sh
tools/wii-toolchain/bin/cargo build --release --manifest-path path/to/Cargo.toml
tools/wii-toolchain/bin/cargo run --release --manifest-path path/to/Cargo.toml
```

The port uses panic abort, picolibc and devoptab for SD files, and libogc LWP for threads and
thread-local storage. It mounts the SD card at startup when available. Absolute device paths
returned by `std` have a leading slash, for example `/sd:/apps`. Arguments and the executable
path come from the homebrew loader. IPv4 sockets initialize IOS networking on first use.

The FAT backend supports path-based read-only permissions and modification times at FAT's
two-second precision. File locking, changing metadata through an open file, hard links,
symlinks, creating subprocesses, and IPv6 are unsupported. A FAT rename to an existing name
returns `AlreadyExists`. Random seeding uses the entropy source described below.

## Files

- `wii.mk` has the make rules for the C examples, `wii.ld` is the linker script
- `powerpc-nintendo-wii.json` and `patches/rust-std/` provide the target and Rust changes used by the
  `bin/cargo` wrapper. The Rust backend calls picolibc, LWP and libogc's IOS socket APIs directly
- `crates/gxtexconv/` converts images to GX textures, as a library for `build.rs` files and a CLI for
  the Makefiles
- `crates/libogc-sys/` has Rust bindings for graphics, matrices, picolibc, threads, Wii Remote input,
  AESND audio, SD card mounting and IOS network sockets
- `bin/cc` is the C compiler for cc-rs build scripts of Rust projects, like the bundled SQLite of
  `libsqlite3-sys`: it builds the toolchain and compiles with the Wii flags and picolibc.
  `bin/ar` is a separate wrapper for `llvm-ar`. The Cargo wrapper selects both for target builds
- `bin/run` converts a program to a `.dol` and starts it in Dolphin, extra arguments go to Dolphin
- `runner/runner.cpp` runs a Wii program headless in [Dolphin](https://dolphin-emu.org/)'s core,
  field exact, for scripts and agents: it presses the Wii Remote's and the Nunchuk's buttons, points
  the Wii Remote, tilts the Nunchuk's stick, takes PNG screenshots and saves and loads savestates.
  Each argument is a command, without them it reads one per line from stdin and answers each with
  `ok`. What the program writes to the debug UART, like `printf`, goes to stderr:

    ```sh
    ../../target/wii-toolchain/bin/dolphin-runner ../../examples/wii/canvas/c/target/canvas.dol 'wait 120' \
        'point 320 240' 'press a' 'shot canvas.png' 'save canvas.sav'
    ```

    The commands are `wait <frames>`, `press <keys> [frames]`, `hold <keys>`, `release`,
    `point <x> <y>`, which points at a pixel of the 640x480 screen as far as Dolphin's pointer
    range and the program's IR settings agree, `stick <x> <y>` from -100 to 100, `shot <file.png>`,
    the frame the video interface shows, `save <file>`, `load <file>`, `hook <address>` and
    `count`, which count the executions of a function such as a renderer's for frame rates,
    `wav <file>` and `wav stop`, which record the DSP's sound, `peek <address> [words]`, which
    prints words of memory, `await <text> <frames>`, which runs until a UART line after the last
    awaited one contains the text, `test <frames>`, which awaits the test program's
    `DONE <n> failed` line and fails on failures, both fail on a timeout, and `quit`. The keys are the Wii Remote's
    `a`, `b`, `1`, `2`, `plus`, `minus`, `home`, `up`, `down`, `left` and `right` and the
    Nunchuk's `c` and `z`, joined like `up+a`, the first Wii Remote always has a Nunchuk. The
    optional `--sd <directory>` before the program puts an SD card with the files of that
    directory in the SD card slot, its writes stay in a temporary card image. Every run starts
    from a fresh Dolphin user directory, without a GPU, JIT, sound output or window: Dolphin's
    cached interpreter, which runs decoded instructions without generating code, and its
    software renderer on one thread, as fast as it goes, with the clock starting at 2026-01-01
    00:00 UTC, so runs are repeatable. Dolphin maps the emulated memory through `/dev/shm` on
    Linux, which Docker limits to 64 MiB, so give a container more, like `--shm-size=1g`

- `patches/<name>/` hold our changes to the upstream projects as commits
- `test/c/` and `test/rust/` are the C runtime and Rust standard library guest suites

The runner's `test-title` command supplies synthetic title metadata for standalone
ES signing tests. `./build.sh test` uses it to check entropy generation without a
disc or NAND dump; normal programs keep their existing boot behavior.

## Test

`./build.sh test` builds both suites into their `target/` folders and runs them in
`dolphin-runner` with fresh SD images. The C suite checks startup, the C library,
compiler-rt, libogc's paired single math, threads and timers, the C library locks, RGBA8 and
RGB5A3 textures converted by `gxtexconv`, constructors, the standard streams, the clock, and
files, directories, relative paths, descriptors and stdio streams.

The Rust suite covers threads, parking, synchronization,
TLS destructors, clocks, launcher arguments, environment variables, SD files, unsupported subprocess
creation, TCP and UDP loopback, hostname lookup, and randomness through `std` hash seeding and
the workspace's `getrandom` crate. A separate guest run verifies
that a panic produces output and enters the Wii exit routine. Both suites report their results on
the debug UART, and the runner requires `DONE 0 failed` within 3600 frames.

From the repository root on Linux, `cargo xtask check-others` builds the assembly examples and the Nintendo DS test ROM, runs this test, then builds the Wii examples and the PlaatCraft Wii port. On macOS, `cargo xtask check-macos-examples` builds the native macOS applications. `cargo xtask check` includes these platform checks.

## Patches

libogc needs newlib and GNU as, its patches make it build with picolibc and LLVM. picolibc's
patches make its PowerPC port build with clang and patch in faster variants of some functions.
To change them, edit and commit in `../../target/wii-toolchain/src/<name>/`, run `./build.sh` to try
it and `./build.sh export-patches` to write the commits back. The build stops instead of applying
changed patches over commits or edits in `../../target/wii-toolchain/src/<name>/` that aren't
exported yet.

The libogc patches:

- `0001` to `0007` remove the newlib glue: reent structs, devoptab, malloc locks, time hooks,
  the text console and the network cleanup, `0002` also uses standard C headers instead of the
  newlib and POSIX ones
- `0008` to `0010` add assembler macros for the Gekko / Broadway paired single instructions,
  which LLVM doesn't know, so the fast paired single matrix code keeps working
- `0011` and `0012` stop using small data sections, which clang doesn't support
- `0013` implements the system functions picolibc leaves to the platform, like `sbrk` and `stdout`
  to Dolphin's debug UART
- `0014` implements picolibc's locks with LWP mutexes, so threads can share `malloc`
- `0015` busy waits while the Wii Remotes disconnect, because sleeping hangs when only the main
  thread runs, so exiting to the Homebrew Channel works
- `0016` implements picolibc's POSIX file functions (`open`, `read`, `stat`, `opendir`, ...) on
  devoptab devices. `0003` and `0005` removed newlib's devoptab glue, and picolibc's stdio only
  calls these functions, so without it no file could be opened and the standard streams could
  not reach Dolphin's debug UART. It also runs the constructors and finalizers of the linker
  script's init and fini arrays, because picolibc's stdio registers its flush at exit in one.
  It also resolves device paths for `realpath`, implements `chmod` and `utimensat`, and keeps
  the errors of directory iteration
- `0017` implements `gettimeofday` with the time base, which libogc starts at the RTC time.
  picolibc leaves it to the platform, without it `time()` fails and the FAT driver can't give
  files their modification time
- `0018` adds `libogc/getentropy.c`, which supplies picolibc's `getentropy()` using
  ChaCha20 seeded with hashed ES signatures, with key rotation after each request
  and a fresh seed after at most 1 MiB of output. The initial request and reseeds
  still require IOS signing. C programs use `<unistd.h>`, Rust programs use
  `libogc-sys` or the workspace's `getrandom` replacement.
  Its security depends on fresh, unpredictable IOS signing nonces; emulator
  tests do not establish the quality of this entropy source on real hardware.
- `0019` provides thread-local `errno` and a Rust TLS slot on LWP threads
- `0020` sends and receives in single IOS transfers, a stream gets a partial count above 64 KiB,
  and adds `net_getpeername` and `net_getsockopt` with `SO_ERROR` translated to `errno` values,
  for the Rust networking backend

The libdvm patches:

- `0001` mounts only the SD card, because upstream `dvmInit` also probes USB storage and the
  GameCube card slots, which libogc is built without, so it wouldn't link. It adds
  `wiiMountSd` because `dvmInit` mounts once at startup, a game can't notice a replaced card
- `0002` exposes FAT permission and modification-time updates and adds metadata for the SD root

The Wii `patches/rust-std/` series adds the Wii startup and thread hooks, then SD files and
environment support, then IOS networking. Run `./build.sh std` to prepare its working tree.

The Dolphin patches, for `dolphin-runner`:

- `0001` uses no fastmem exception handler in a generic build, which has no JIT or fastmem
- `0002` runs the software renderer headless without OpenGL, so it needs no GPU or display
- `0003` remembers the XFB of the last field the video interface output, for `shot`
- `0004` counts the hits of breakpoints, also when they don't break, for `hook` and `count`
- `0005` lets frame stepping stop at every VI field, also when it shows the same frame, for an
  exact number of frames
- `0006` builds `DOLPHIN_RUNNER_SOURCE`, a frontend outside Dolphin's source tree
- `0007` keeps fractional elapsed time during IOS socket polling so guest timeouts expire
  when Dolphin runs faster than real time
- `0008` reports the pending error of `SO_ERROR` with an IOS error number instead of the host's

## SD card storage

`wiiMountSd(1)` from `dvm.h` mounts the FAT32 SD card as `sd:/` and makes it the current
directory, so the standard C file functions work on it, `wiiMountSd(0)` also accepts FAT12 and
FAT16. It probes the card again, so call it with all files closed, for example to check for a
card after it was replaced.
