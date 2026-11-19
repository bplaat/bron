# Wii U Toolchain

Everything the [Wii U examples](../../examples/wiiu/) share to build Wii U programs with plain clang and lld, without devkitPro.
`build.sh` builds the toolchain in the repository root's `target/wiiu-toolchain/`. The C
Makefiles and the Rust Cargo wrapper invoke it before building a project. For now the
programs are only tested in the [Cemu](https://cemu.info/) emulator.

- Clones devkitPro's [wut](https://github.com/devkitPro/wut) v1.9.1 and applies `wut/patches/`,
  for its headers, the export lists of the system libraries in `cafe/*.def`, libwhb and libgfd
- Clones devkitPro's [wut-tools](https://github.com/devkitPro/wut-tools) v1.3.5 and applies
  `wut-tools/patches/`, then builds `rplimportgen`, which writes the import stubs of the system
  libraries, and `elf2rpl`, which converts a linked `.elf` to the `.rpx` Cafe OS loads
- Clones [picolibc](https://github.com/picolibc/picolibc) 1.8.12 and applies `picolibc/patches/`,
  then builds it with meson as the C library, its headers and `libc.a` with the math library go
  to `../../target/wiiu-toolchain/sysroot/`
- Builds `../../target/wiiu-toolchain/libwut.a` from the import stubs of all system libraries,
  wutpicolibc, which wut patches add for picolibc (startup, heap, console, clock, sleep, locks and
  SD card files), libwhb (graphics, process lifecycle and logging) and libgfd
- Clones the last commit of [decaf-emu](https://github.com/decaf-emu/decaf-emu) and applies
  `decaf-emu/patches/`, then builds its `latte-assembler`, which assembles Latte shader assembly
  to `.gsh` shader files
- Builds our [gtxconv](gtxconv) with Cargo to `../../target/wiiu-toolchain/gtxconv`, which converts
  images to `.gtx` texture files in the tiled layout of the GPU
- Checks out only the matrix math of devkitPro's [libogc](https://github.com/devkitPro/libogc) v2.11.0,
  applies `libogc/patches/` and builds `../../target/wiiu-toolchain/libgu.a`. Its `ps_` functions use
  the paired single instructions, which the Espresso CPU has like the Wii's Broadway. `gu.h` and the
  two files it includes go to `../../target/wiiu-toolchain/include/`
- Checks out only the builtins of LLVM's [compiler-rt](https://github.com/llvm/llvm-project/tree/main/compiler-rt/lib/builtins)
  llvmorg-23.1.3 and builds the 64-bit integer math and conversions the PowerPC 750 can't do in
  instructions to `../../target/wiiu-toolchain/libcompiler-rt.a`, the C examples link it (Rust has its own)

```sh
cd tools/wiiu-toolchain
./build.sh                 # fetch, patch and build
./build.sh clean           # remove ../../target/wiiu-toolchain/
./build.sh std             # prepare the patched Rust sources
./build.sh test            # run the C and Rust suites without a GPU or JIT
./build.sh cemu            # build ../../target/wiiu-toolchain/cemu-runner
./build.sh export-patches  # write the commits in ../../target/wiiu-toolchain/<name>/ back to <name>/patches/
```

All code is compiled for the PowerPC 750 compatible Espresso CPU (`-mcpu=750`) and optimized for
size with `-Os`, like the Rust examples (`opt-level = "s"`).

## Integration

Install the host dependencies listed in the [examples README](../../examples/wiiu/README.md#getting-started).
Downloaded sources, compiled libraries and host tools stay in the repository root's ignored `target/wiiu-toolchain/` folder.

C projects include `wiiu.mk` from their Makefile, which also assembles `src/shader.vsh` and
`src/shader.psh` to `target/shader.gsh` and converts the textures of `src/assets/textures.toml` to
`target/textures.gtx`. Rust projects use the toolchain's `cargo` wrapper and the shared
`build.rs` helper for shader assembly and texture conversion. The `wut-sys` crate provides
bindings to wut, libwhb, libgfd, picolibc and the libogc matrix functions. Each project
keeps its build output in `target/`.

The toolchain and the examples build separately from the main Cargo workspace, the
[PlaatCraft Wii U port](../../games/plaatcraft/docs/wiiu.md) builds in it.

### Rust standard library

Run the toolchain's `cargo` wrapper for standalone Cargo projects that use `std`.

```sh
./cargo build --release --manifest-path /path/to/my-project/Cargo.toml
./run.sh /path/to/my-project/target/powerpc-nintendo-wiiu/release/my-project.elf
```

The wrapper uses `powerpc-nintendo-wiiu.json` and the upstream Rust nightly pinned in
`build.sh`, and the Wii U's build profile: optimized for size, aborting panics, and no LTO, also
in the Cargo workspace (its per-package optimization levels still apply). It installs the
matching `rust-src` component when needed, applies `rust/patches/` to a local copy and builds
`std` with Cargo. The Rust backend calls picolibc and Cafe OS directly. The target supports
aborting panics, Cafe OS threads, thread-local storage and synchronization, clocks,
environment and arguments, SD files, entropy, and IPv4 TCP and UDP.

The port exposes SD file modification times. File timestamp and permission updates, file
locks, hard links, symlinks, IPv6 and child processes are unsupported. Environment
variables are process-local, and the Rust process ID is a fixed placeholder.

## Files

- `wiiu.mk` has the make rules for the C examples, `wiiu.ld` is the linker script. It places the
  code at 0x02000000, the data at 0x10000000 and the import sections, which it includes from the
  generated `../../target/wiiu-toolchain/imports.ld`, at 0xC0000000. Programs are linked with
  `--emit-relocs`, elf2rpl turns the relocations into the imports of the system libraries
- `powerpc-nintendo-wiiu.json` and `rust/patches/` provide the target and Rust changes used by the
  `cargo` wrapper. `build.rs` assembles shaders and converts textures for Rust projects
- `wut-sys/` has Rust bindings for graphics, matrices, picolibc, threads, input, audio and
  networking. USB input uses the native `nsyskbd` keyboard callbacks and `nsyshid` device,
  protocol, descriptor, and asynchronous report APIs; no external HID library is required.
  Raw sockets use `socketclose` and `socketlasterr` instead of the C file functions
- `gtxconv/` converts images to `.gtx` texture files from a TOML config, as a library and a CLI
- `cc` is the C compiler for cc-rs build scripts of Rust projects: it builds the toolchain and
  compiles with the Wii U flags and the picolibc, wut and libogc matrix headers. `ar` is a separate
  wrapper for `llvm-ar`. The Cargo wrapper selects both for target builds
- `run.sh` converts a program to an `.rpx` and starts it in Cemu, extra arguments go to Cemu
- `cemu/` builds a headless runner from the main branch of [Cemu](https://github.com/cemu-project/Cemu)
  with `cemu/patches/`: Cemu's own interpreter, RPL loader, coreinit (threads, scheduler, memory,
  time and the FSA file functions), nlibnss random bytes, nsysnet sockets, and the FSA of IOSU on a host folder as
  the SD card, without the GPU, audio, input, JIT or GUI. `cemu/runner.cpp` replaces Cemu's system
  startup and stubs the rest of Cemu, `cemu/shim/` has the few parts of boost, glm, wxWidgets, X11,
  GTK, curl and the window system that Cemu's headers need. The other system libraries aren't
  there, so it doesn't run the graphical examples, use the normal Cemu for those
- `test/c/` and `test/rust/` hold the Wii U acceptance suites run by `./build.sh test`

## Test

Install a C++20 compiler and zlib in addition to the toolchain's normal dependencies.
Run `./build.sh test` to build both suites and `cemu-runner` in
`../../target/wiiu-toolchain/`. Each guest gets a fresh user folder whose `sdcard/` is
its SD card. `cargo xtask check-others` runs both suites and builds the Wii and Wii U C
and Rust examples.

The Rust suite covers threads, synchronization,
thread-local storage, clocks, environment, files, processes, networking and randomness.
It checks the platform-specific filesystem and process errors, IPv4 loopback and
a separate panic launch that must print the panic message and exit with status 134.
The randomness checks cover both `std` hash seeding and the workspace's `getrandom`
crate, including fills that span multiple native `getentropy` calls.

The C suite checks constructors, finalizers, memory functions at different alignments,
heap alignment and realloc preservation, calloc overflow, printf and scanf, standard streams,
stdlib and string functions, setjmp and assert, math including IEEE edge cases, compiler-rt's
64-bit operations, paired single matrices and vectors, threads with mutexes and the C library
lock, the clock and sleeping, and files, directories, relative paths, descriptors, stdio streams
and file times on the SD card. It also loads GTX textures made by gtxconv with GFDGetTextureInPlace and checks their formats,
mipmaps and in-place pointers. It assembles the cube example's shaders with latte-assembler
and checks GFD relocation, attribute names, uniforms and samplers. It tests CPU-side asset
loading; shader execution and rendered pixels need the normal emulator.

The runner writes guest output to stderr. `test` waits for a `DONE <n> failed` line and fails for
guest failures, an early exit or when the program doesn't finish in time. `run` waits for the
exit and returns the guest's status. Programs run in real time on Cemu's scheduler with one
core, the clock starts at the host's time. Cemu calibrates the clock against the host clock
first, which takes three seconds. The defaults are `run`, 60 seconds and `cemu-runner` in the
temporary folder as the user folder, set `CEMU_RUNNER_LOG` to also print Cemu's log.

```sh
../../target/wiiu-toolchain/cemu-runner test/c/target/test.rpx test 60 test/c/target/manual-cemu
../../target/wiiu-toolchain/cemu-runner test/c/target/test.rpx run
```

An optional final argument supplies space-delimited arguments to the guest.

Only Cemu's `src/` is checked out, without its submodules, with [fmt](https://github.com/fmtlib/fmt)
12.0.0 and [pugixml](https://github.com/zeux/pugixml) v1.15 next to it. Its patches work like the
other repos, `./build.sh export-patches` also writes `../../target/wiiu-toolchain/cemu/` back to
`cemu/patches/` once it is there:

- `0001` truncates host files on Unix, `FSATruncateFile` was a debug break
- `0002` keeps files working after reading past their end, which left the Unix file stream failing
  every later read and write
- `0003` fails to remove a host directory that isn't empty, which reported success
- `0004` gives host files their modification time, in the time zone of `OSTime`, `FSAGetStat`
  returned 1980
- `0005` fuses the multiply-add instructions on every host, also the paired single ones, on x86-64
  they rounded twice
- `0006` implements NSS initialization and random-byte exports using the host OS
  CSPRNG, with the error codes of [cafe-sys](https://github.com/rust-wiiu/cafe-sys). The headless
  runner links the implementation, and the guest suite checks NSS errors, fresh output and
  reinitialization through `getentropy()`.
- `0007` translates host connection errors to nsysnet error codes, implements the
  `TCP_NODELAY` getter, handles zero-length UDP sends and fixes the `EAFNOSUPPORT` number.

## Shaders

The vertex shaders take the matrices like the Wii's GX: the rows of a libogc `Mtx44` projection
matrix in C0 to C3 and the rows of an `Mtx` model view matrix in C4 to C6.

The shaders are written in the Latte (AMD R700) assembly of decaf-emu's latte-assembler, like the
shaders of devkitPro's [wiiu-examples](https://github.com/devkitPro/wiiu-examples). The comments
starting with `$` set the shader registers and name the attributes, uniforms and samplers, which
libwhb looks up by name. devkitPro has no GLSL compiler, the alternative is
[CafeGLSL](https://github.com/Exzap/CafeGLSL), which compiles GLSL on the console at runtime.

## Textures

The programs embed their textures as a `.gtx` file aligned to 4 KiB and load them with
`GFDGetTextureInPlace`, which a wut patch adds. It points the textures at their data in the file,
so the GPU reads the BC1 compressed, tiled and mipmapped textures in place without decoding or
copying them.

## Patches

wut needs newlib and devkitPPC, its first patch makes it work with picolibc. The patches of wut-tools,
picolibc and decaf-emu make them build and work with LLVM, and decaf-emu without its other
libraries. To change them, edit and commit in `../../target/wiiu-toolchain/<name>/`, run
`./build.sh` to try it and `./build.sh export-patches` to write the commits back. The build stops
instead of applying changed patches over commits or edits in `../../target/wiiu-toolchain/<name>/`
that aren't exported yet.

The wut patches:

- `0001` adds wutpicolibc, which replaces wutcrt, wutnewlib and wutmalloc: `__rpx_start` runs the
  constructors and `main`, `malloc` uses the default heap of Cafe OS, the standard streams write
  lines to `OSReport`, and Cafe OS supplies clocks, mutexes and per-thread `errno`
- `0002` adds `GFDGetTextureInPlace` to libgfd, which points a texture at its image and mipmaps in a
  loaded `.gtx` file instead of copying them
- `0003` implements picolibc's POSIX file functions (`open`, `read`, `stat`, `opendir`, ...) on the
  SD card with FSA. A Cafe OS mutex protects the filesystem state shared by C and Rust callers
- `0004` adds `wutpicolibc/wut_entropy.c`, which supplies picolibc's `getentropy()`
  through `NSSGetRandom`. C programs use `<unistd.h>`, Rust programs use `wut-sys`
  or the workspace's `getrandom` replacement.

The libogc patches, trimmed versions of the Wii toolchain's for only the matrix math files:

- `0001` adds `paired_single.inc`, LLVM doesn't know the paired single instructions, so its
  assembler macros encode them
- `0002` writes the paired single load and store offsets as `d, rA`, which the macros can take apart
- `0003` loads the matrix constants from `.rodata` with absolute addresses, clang has no small data

The wut-tools patch:

- `0001` writes the import stubs in progbits sections and lets elf2rpl give the import and export
  sections their RPL types by name, because LLD rejects sections with the RPL section types

The picolibc patches:

- `0001` and `0002` build its PowerPC port with clang and meson
- `0003` and `0004` copy and set words in the memory functions and compute `sqrt` with `frsqrte`

The decaf-emu patches:

- `0001` builds latte-assembler without its GLSL compile command, which needs glslang and AMD's
  Windows-only shader analyzer, and without the spdlog logger of decaf-emu's common library
- `0002` parses the `$` comments of the shaders: the grammar leaves the semicolon out of comment
  tokens, so upstream silently ignores all register and variable comments

## SD card storage

The SD card is the only storage the C file functions reach, the console's internal storage is left
out. The first file call mounts it at `/vol/external01`, `sd:/` paths and absolute paths like
`/saves/world.dat` are on it and relative paths start at the current directory, which starts at
`sd:/`. Seeking never extends a file, writing after its end fills the gap with zeros. A program
can have 29 files and directories open, besides the standard streams.
