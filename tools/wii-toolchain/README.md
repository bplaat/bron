# Wii Toolchain

Everything the [Wii examples](../../examples/wii/) share to build Wii programs with plain clang and lld, without devkitPro.
`build.sh` builds it all in the repository root's `target/wii-toolchain/`, the example Makefiles and Rust `build.rs` files run it for
you:

- Clones upstream [libogc](https://github.com/devkitPro/libogc) v2.11.0 and applies
  `libogc/patches/` as commits, then builds the parts we use (listed in `build.sh`) to
  `../../target/wii-toolchain/libogc.a`
- Clones [picolibc](https://github.com/picolibc/picolibc) 1.8.12 and applies `picolibc/patches/`,
  then builds it with meson as the C library, its headers and `libc.a` with the math library go
  to `../../target/wii-toolchain/sysroot/`
- Clones devkitPro's [gamecube-tools](https://github.com/devkitPro/gamecube-tools) v1.0.7 and
  builds `elf2dol` (`.elf` to `.dol`)
- Builds our [gxtexconv](gxtexconv) with Cargo to `../../target/wii-toolchain/gxtexconv`
- Checks out only the builtins of LLVM's [compiler-rt](https://github.com/llvm/llvm-project/tree/main/compiler-rt/lib/builtins)
  llvmorg-23.1.3 and builds the 64-bit integer math and conversions the PowerPC 750 can't do in
  instructions to `../../target/wii-toolchain/libcompiler-rt.a`, the C examples link it (Rust has its own)

```sh
cd tools/wii-toolchain
./build.sh                 # fetch, patch and build
./build.sh clean           # remove ../../target/wii-toolchain/
./build.sh export-patches  # write the commits in ../../target/wii-toolchain/<name>/ back to <name>/patches/
./build.sh test            # build test/ and run it in Dolphin
```

All code is compiled for the PowerPC 750 (`-mcpu=750`) and optimized for size with `-Os`, like
the Rust examples (`opt-level = "s"`). LTO doesn't work with libogc: it waits for interrupts in
loops on plain variables, which LTO can optimize into endless loops. Code is compiled with
`_DEFAULT_SOURCE`, so picolibc also declares its POSIX and BSD functions under `-std=c23`.

## Integration

Install the host dependencies listed in the [examples README](../../examples/wii/README.md#getting-started).
Downloaded sources, compiled libraries and host tools stay in the repository root's ignored `target/wii-toolchain/` folder.

C projects include `wii.mk` from their Makefile. Rust projects select `wii.json` and `run.sh` in
`.cargo/config.toml`, then call the shared `build.rs` helper to build the libraries and configure
the linker with an absolute path to `wii.ld`. Each example keeps its own build output in `target/`.

The toolchain and examples build separately from main Cargo workspace. The existing
`games/plaatcraft/wii` port still uses its original devkitPPC build.

## Files

- `wii.mk` has the make rules for the C examples, `wii.ld` is the linker script
- `wii.json` is the Rust target; `build.rs` builds and links `../../target/wii-toolchain/libogc.a`
  and picolibc's `../../target/wii-toolchain/sysroot/lib/libc.a`
- `gxtexconv/` converts images to GX textures, as a library for `build.rs` files and a CLI for
  the Makefiles
- `run.sh` converts a program to a `.dol` and starts it in Dolphin, extra arguments go to Dolphin
- `test/` is a Wii program that tests the toolchain, see below

## Test

`./build.sh test` builds `test/` into the repository root's `target/wii-toolchain/test/` and runs it in Dolphin without its user
interface, on Linux headless in `dolphin-emu-nogui` without video when it is installed. It checks
the startup code, the C library, compiler-rt, libogc's paired single math, threads and timers, the
C library locks, and a texture converted by `gxtexconv`. The program writes its results to
`/tmp/test.txt` on Dolphin's NAND after every group, the script prints them and fails when a check
failed or the program didn't finish. The program keeps running after the last group, because
returning reloads IOS, which clears `/tmp`.

From the repository root on Linux, `cargo xtask check-others` builds the assembly examples, Empires, and the Nintendo DS test ROM, then runs this test. On macOS, `cargo xtask check-macos-examples` builds the native macOS applications. `cargo xtask check` includes these platform checks.

## Patches

libogc needs newlib and GNU as, its patches make it build with picolibc and LLVM. picolibc's
patches make its PowerPC port build with clang and patch in faster variants of some functions.
To change them, edit and commit in `../../target/wii-toolchain/<name>/`, run `./build.sh` to try
it and `./build.sh export-patches` to write the commits back. The build stops instead of applying
changed patches over commits or edits in `../../target/wii-toolchain/<name>/` that aren't
exported yet.

The libogc patches:

- `0002` uses standard C headers and removes unused POSIX header dependencies
- `0001` to `0007` remove the newlib glue: reent structs, devoptab, malloc locks, time hooks,
  the text console and the network cleanup
- `0008` to `0010` add assembler macros for the Gekko / Broadway paired single instructions,
  which LLVM doesn't know, so the fast paired single matrix code keeps working
- `0011` and `0012` stop using small data sections, which clang doesn't support
- `0013` implements the system functions picolibc leaves to the platform, like `sbrk` and `stdout`
  to Dolphin's debug UART
- `0014` implements picolibc's locks with LWP mutexes, so threads can share `malloc`
