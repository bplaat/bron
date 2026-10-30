# Nintendo DS Toolchain

What the ports share to build Nintendo DS programs with plain clang and lld, without devkitPro.
`build.sh` builds it all in `../../target/nds-toolchain/`, the port build scripts run it for you:

- devkitPro's [calico](https://github.com/devkitPro/calico) v1.2.0,
  [libnds](https://github.com/devkitPro/libnds) v2.0.2,
  [libdvm](https://github.com/devkitPro/libdvm) v2.1.0 with
  [FatFs](https://github.com/devkitPro/fatfs-mod) v0.15.3 and the
  [default ARM7 program](https://github.com/devkitPro/default-arm7) v0.8.4, with their
  `<name>/patches/`, to `../../target/nds-toolchain/lib*.a` and `../../target/nds-toolchain/ds7.elf`
- [picolibc](https://github.com/picolibc/picolibc) 1.8.12 with `picolibc/patches/`, built with meson
  as the C library for each CPU, its headers and `libc.a` with the math library go to
  `../../target/nds-toolchain/sysroot9/` and `../../target/nds-toolchain/sysroot7/`
- [ndstool](https://github.com/devkitPro/ndstool) v2.3.1 to `../../target/nds-toolchain/bin/ndstool`
- the builtins of LLVM's [compiler-rt](https://github.com/llvm/llvm-project/tree/main/compiler-rt/lib/builtins)
  llvmorg-23.1.3 (64-bit integer math, division and software floating point) to
  `../../target/nds-toolchain/libcompiler-rt9.a` and `../../target/nds-toolchain/libcompiler-rt7.a`

```sh
./build.sh                 # fetch, patch and build
./build.sh clean           # remove ../../target/nds-toolchain/
./build.sh export-patches  # write the commits in ../../target/nds-toolchain/<name>/ back to <name>/patches/
./build.sh test            # build test/ and run it headless in melonDS, as a DS and as a DSi
./build.sh melonds         # build ../../target/nds-toolchain/bin/melonds-runner
```

Building picolibc needs meson and ninja, melonDS needs CMake, ninja and zlib. The ARM9 (ARMv5TE)
and ARM7 (ARMv4T) have no floating point unit, picolibc's float math is single precision. Code is
Thumb, except objects named `*.32.o` or `*.itcm.o` (a port's `ITCM_SOURCES`), which are ARM code in
fast memory. Code is compiled with `_DEFAULT_SOURCE`, so picolibc also declares its POSIX and BSD
functions.

## Files

- `nds.mk` has the make rules for the ports' `Makefile.nds`, at `-O3`. A port's own ARM7 code
  (`ARM7_SOURCES`) is linked with the default ARM7 program, which calls its `arm7PortInit`
- `run.sh` starts a program in melonDS, `make run` uses it
- `melonds/runner.cpp` runs a `.nds` headless in [melonDS](https://github.com/melonDS-emu/melonDS)'s
  core library, with its interpreter, software renderer, free BIOSes and generated firmware, frame
  exact, for scripts and agents: it presses buttons, touches the touch screen, takes PNG
  screenshots of both screens and saves and loads savestates. Each argument is a command, without
  them it reads one per line from stdin and answers each with `ok`:

    ```sh
    ../../target/nds-toolchain/bin/melonds-runner ../../games/nds/duke3d/target/duke3d.nds 'wait 1200' 'press start' 'wait 60' \
        'press a' 'shot menu.png' 'save menu.mln'
    ```

    The commands are `wait <frames>`, `press <keys> [frames]`, `hold <keys>`, `release`,
    `touch <x> <y> [frames]`, `shot <file.png>`, `save <file>`, `load <file>`, `hook <address>`
    and `count`, which count the executions of a function such as a renderer's for frame rates,
    `profile start`, `busy`, the share of the ARM9's time not waiting, `profile save <file>`, the
    ARM9 cycles per instruction address with their cache misses, `profile stop`, `wav <file>` and
    `wav stop`, which record the sound, `pak`, which puts a Memory Expansion Pak in the GBA slot
    (not on a DSi), `peek <address> [words]`, which prints words of memory, and `quit`, keys like
    `up+a`. Before the ROM argument, `--dsi` runs it as a DSi instead of a DS, and `--fat
<directory>` puts a writable SD card initialized from that directory in melonDS's R4 like card,
    as `fat:/`, or on a DSi in its SD card slot, as `sd:/`, the ROM's `argv[0]` points to it there.
    Its writes stay in emulated memory. Without it, ports read their data from NitroFS.
    `test <address> <frames>` reads the test ROM's status and log from RAM, returning an error on
    failed tests or a frame timeout.

- `test/` is a DS program that checks the startup code, the C library, compiler-rt, threads, ITCM,
  FAT, NitroFS and the console's escape sequences. `./build.sh test` builds melonDS and runs it
  headless twice, as a DS with a writable R4 card and as a DSi with a writable SD card, both
  initialized from `test/target/sd/`. It prints the results and fails when a check fails or the ROM
  does not finish within 3600 emulated frames. The ROM also saves `test.txt` on the emulated card.

## Patches

Edit and commit in `../../target/nds-toolchain/<name>/`, run `./build.sh` to try it and `./build.sh export-patches`
to write the commits back.

- calico: unified assembly syntax for LLVM, no newlib glue, linker scripts that work with lld, and
  the system functions picolibc leaves to the platform: the POSIX file functions on the devoptab
  devices (SD card, NitroFS, console) in `sys/iosupport.h`, which read into memory outside main RAM
  through main RAM, `stdin` is always empty, `sbrk`, locks on calico mutexes, time and sleeping
- libnds: builds with clang and without newlib, with a pregenerated `libversion.h`, and
  `ramPakInit` in `nds/arm9/rampak.h` unlocks the 8 MiB RAM of the DS Browser's Memory Expansion
  Pak in the GBA slot and maps it data cached, the standard `printf` names, and the console keeps
  escape sequences between writes, picolibc writes the unbuffered `stdout` a character at a time
- picolibc: `memcpy`, `memmove` and `memset` that write the bytes at their ends with their
  halfword in VRAM and the GBA slot, which ignore byte writes, as ARM code in ITCM (`memset`
  stores 32 bytes per two `stm`), and a far heap in `malloc.h` for memory outside the main RAM,
  like a Memory Expansion Pak's (`far_heap_init`, `far_malloc`), and `clock` counts milliseconds,
  so its 32 bits wrap after 24 days
- default-arm7: calls `arm7PortInit`, which does nothing by default, before its main loop
- melonds: the ARM9 interpreter calls a hook after each instruction with its address and cycles,
  for `hook` and `profile` of the runner, a DSi SD card's CSD has the size of its image, so calico
  doesn't reject its partition as bigger than the card, and complete SD writes of the R4 like card
  no longer print that they are incomplete
- compiler-rt: the optimized float functions for ARMv5TE, divisions that return to Thumb on
  ARMv4T

From the repository root, `./meta.sh check-others` (or `cargo xtask check-others`) builds and runs
these tests on Linux alongside the other platform checks. To run only the DS tests on Linux or
macOS, use `tools/nds-toolchain/build.sh test`. The runner is built from melonDS's core library,
interactive `make run` uses the melonDS app.
