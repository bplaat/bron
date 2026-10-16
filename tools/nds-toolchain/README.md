# Nintendo DS Toolchain

What the ports share to build Nintendo DS programs with plain clang and lld, without devkitPro.
`build.sh` builds it all in `../../target/nds-toolchain/`, the port build scripts run it for you:

- devkitPro's [calico](https://github.com/devkitPro/calico) v1.2.0,
  [libnds](https://github.com/devkitPro/libnds) v2.0.2,
  [libdvm](https://github.com/devkitPro/libdvm) v2.1.0 with
  [FatFs](https://github.com/devkitPro/fatfs-mod) v0.15.3 and the
  [default ARM7 program](https://github.com/devkitPro/default-arm7) v0.8.4, with their
  `<name>/patches/`, to `../../target/nds-toolchain/lib*.a` and `../../target/nds-toolchain/ds7.elf`
- our C library in `libc/` to `../../target/nds-toolchain/libc9.a` and `../../target/nds-toolchain/libc7.a`
- [ndstool](https://github.com/devkitPro/ndstool) v2.3.1 to `../../target/nds-toolchain/bin/ndstool`
- the builtins of LLVM's [compiler-rt](https://github.com/llvm/llvm-project/tree/main/compiler-rt/lib/builtins)
  llvmorg-23.1.3 (64-bit integer math, division and software floating point) to
  `../../target/nds-toolchain/libcompiler-rt9.a` and `../../target/nds-toolchain/libcompiler-rt7.a`

```sh
./build.sh                 # fetch, patch and build
./build.sh clean           # remove ../../target/nds-toolchain/
./build.sh export-patches  # write the commits in ../../target/nds-toolchain/<name>/ back to <name>/patches/
./build.sh test            # build test/ and run it in melonDS
./build.sh desmume         # build ../../target/nds-toolchain/bin/desmume-runner, needs meson and ninja
```

The ARM9 (ARMv5TE) and ARM7 (ARMv4T) have no floating point unit. Code is Thumb, except objects
named `*.32.o` or `*.itcm.o` (a port's `ITCM_SOURCES`), which are ARM code in fast memory.

## Files

- `libc/` replaces newlib: word copying `memcpy`/`memset`/`memcmp` in ITCM (`memset` stores 32
  bytes per two `stm`), which write the bytes at their ends with their halfword in VRAM and the GBA
  slot, they ignore byte writes, a first-fit `malloc` with a far heap for memory outside the main
  RAM, like a Memory Expansion Pak's (`far_heap_init`, `far_malloc`), buffered `stdio` on the
  devoptab devices (SD card, NitroFS, console), which only reads straight into main RAM, the
  `printf` and `scanf` families, `strtof`, float math, time and `setjmp`. Floats are single
  precision throughout, `strtod` and `atof` too, nothing does double precision math. `stdin` is
  always empty. It only uses the calico functions in `libc/calico.h`.
- `nds.mk` has the make rules for the ports' `Makefile.nds`, at `-O3`. A port's own ARM7 code
  (`ARM7_SOURCES`) is linked with the default ARM7 program, which calls its `arm7PortInit`
- `run.sh` starts a program in melonDS, `make run` uses it
- `desmume/runner.c` runs a `.nds` headless in [DeSmuME](https://github.com/TASEmulators/desmume)'s
  interface library, frame exact, for scripts and agents: it presses buttons, touches the touch
  screen, takes PNG screenshots of both screens and saves and loads savestates. Each argument is a
  command, without them it reads one per line from stdin and answers each with `ok`:

    ```sh
    ../../target/nds-toolchain/bin/desmume-runner ../../games/nds/duke3d/target/duke3d.nds 'wait 1200' 'press start' 'wait 60' \
        'press a' 'shot menu.png' 'save menu.dst'
    ```

    The commands are `wait <frames>`, `press <keys> [frames]`, `hold <keys>`, `release`,
    `touch <x> <y> [frames]`, `shot <file.png>`, `save <file>`, `load <file>`, `hook <address>`
    and `count`, which count the executions of a function such as a renderer's for frame rates,
    `profile start`, `busy`, the share of the ARM9's time not waiting, `profile save <file>`, the
    ARM9 cycles per instruction address with their cache misses, `profile stop`, `wav <file>` and
    `wav stop`, which record the sound, `pak`, which puts a Memory Expansion Pak in the GBA slot
    before the first `wait`, `peek <address> [words]`, which prints words of memory, and `quit`,
    keys like `up+a`. DeSmuME has no SD card here, the ports read their data from NitroFS.

- `test/` is a DS program that checks the startup code, the C library, compiler-rt, threads, ITCM,
  FAT and NitroFS. `./build.sh test` runs it in melonDS with an SD card in `test/target/sd/` and
  fails when a check fails or it doesn't finish.

## Patches

Edit and commit in `../../target/nds-toolchain/<name>/`, run `./build.sh` to try it and `./build.sh export-patches`
to write the commits back.

- calico: unified assembly syntax for LLVM, no newlib glue, linker scripts that work with lld
- libnds: builds with clang and without newlib, with a pregenerated `libversion.h`, and
  `ramPakInit` in `nds/arm9/rampak.h` unlocks the 8 MiB RAM of the DS Browser's Memory Expansion
  Pak in the GBA slot and maps it data cached
- default-arm7: calls `arm7PortInit`, which does nothing by default, before its main loop
- desmume: returning from an interrupt to Thumb code at an address that isn't a multiple of 4,
  like calico does with `ldm sp!, {..., pc}^`, ran the instruction before it again, the ARM9
  cycles are counted per instruction address for `profile` of the runner and the sound can be
  recorded for `wav`, the runner can put a Memory Expansion Pak in the GBA slot for `pak`, which
  ignores byte writes like in melonDS, the runner can put an R4 card with the files of a directory
  in the card slot for `--fat`, whose driver returns with `bx`, so calico can run it on the ARM7,
  and whose directory is listed by host paths, so it also works on Linux, and the microphone is
  never captured, it only gets emulated samples
- compiler-rt: the optimized float functions for ARMv5TE, divisions that return to Thumb on
  ARMv4T

From the repository root, `cargo xtask check-others` includes the toolchain and test ROM build on Linux without starting an emulator. To build only the DS test ROM, run `tools/nds-toolchain/build.sh` followed by `make -C tools/nds-toolchain/test`. Run `tools/nds-toolchain/build.sh test` to execute the tests in melonDS.
