# Wii U Toolchain

Everything the [Wii U examples](../../examples/wiiu/) share to build Wii U programs with plain clang and lld, without devkitPro.
`build.sh` builds it all in the repository root's `target/wiiu-toolchain/`, the example Makefiles and Rust `build.rs` files run it
for you. For now the programs are only tested in the [Cemu](https://cemu.info/) emulator.

- Clones devkitPro's [wut](https://github.com/devkitPro/wut) v1.9.1 and applies `wut/patches/`,
  for its headers, the export lists of the system libraries in `cafe/*.def`, libwhb and libgfd
- Clones devkitPro's [wut-tools](https://github.com/devkitPro/wut-tools) v1.3.5 and applies
  `wut-tools/patches/`, then builds `rplimportgen`, which writes the import stubs of the system
  libraries, and `elf2rpl`, which converts a linked `.elf` to the `.rpx` Cafe OS loads
- Clones [picolibc](https://github.com/picolibc/picolibc) 1.8.12 and applies `picolibc/patches/`,
  then builds it with meson as the C library, its headers and `libc.a` with the math library go
  to `../../target/wiiu-toolchain/sysroot/`
- Builds `../../target/wiiu-toolchain/libwut.a` from the import stubs of all system libraries,
  wutpicolibc, which a wut patch adds for picolibc, libwhb (graphics, process lifecycle and
  logging) and libgfd
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
./build.sh test            # run the toolchain tests without a GPU or JIT
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
`target/textures.gtx`. Rust projects select `wiiu.json` and `run.sh` in `.cargo/config.toml`, then
call the shared `build.rs` helper to build the libraries, configure the linker, assemble their
shaders and convert their textures. They use the wut, libwhb, libgfd and picolibc functions through
the `wut-sys` crate, which also binds the libogc matrix functions they use. Each project keeps its own build output in `target/`.

The toolchain and the examples build separately from the main Cargo workspace.

## Files

- `wiiu.mk` has the make rules for the C examples, `wiiu.ld` is the linker script. It places the
  code at 0x02000000, the data at 0x10000000 and the import sections, which it includes from the
  generated `../../target/wiiu-toolchain/imports.ld`, at 0xC0000000. Programs are linked with
  `--emit-relocs`, elf2rpl turns the relocations into the imports of the system libraries
- `wiiu.json` is the Rust target; `build.rs` builds and links `../../target/wiiu-toolchain/libwut.a`,
  `libgu.a` and picolibc's `../../target/wiiu-toolchain/sysroot/lib/libc.a` and assembles shaders
- `wut-sys/` has the Rust bindings for the wut, libwhb, libgfd, libogc matrix and picolibc
  functions the programs use
- `gtxconv/` converts images to `.gtx` texture files from a TOML config, as a library and a CLI
- `cc` is the C compiler for cc-rs build scripts of Rust projects: it builds the toolchain and
  compiles with the Wii U flags and the picolibc, wut and libogc matrix headers. `ar` is a separate
  wrapper for `llvm-ar`. Rust projects with C sources set `TARGET_CC` and `TARGET_AR` to these
  scripts in `.cargo/config.toml`
- `run.sh` converts a program to an `.rpx` and starts it in Cemu, extra arguments go to Cemu
- `cemu/` builds a CPU-only runner from the Espresso interpreter of
  [Cemu v2.6](https://github.com/cemu-project/Cemu/tree/v2.6) with `cemu/patches/`. Its build
  links no JIT, graphics, audio or window code and allocates no executable host memory. Its
  frontend loads `.rpx` files like Cafe OS: it inflates their zlib sections, applies their
  relocations and points their imports at a small single-threaded OS bridge for heap allocation,
  OSReport, the clock and cache invalidation. Unsupported OS calls fail, so it doesn't run the
  graphical examples, use the normal Cemu for those
- `test/` is the Wii U program run by `./build.sh test`

## Test

Install a C++20 compiler and zlib in addition to the toolchain's normal dependencies.
Run `./build.sh test` to build the toolchain, `cemu-runner` and the test program in
`../../target/wiiu-toolchain/`, then run the guest suite.
`cargo xtask check-others` runs this suite and builds both the Wii and Wii U C and Rust examples.

The guest suite checks constructors, finalizers, memory functions at different alignments,
heap alignment and realloc preservation, calloc overflow, printf and scanf, standard streams,
stdlib and string functions, setjmp and assert, math including IEEE edge cases, compiler-rt's
64-bit operations, paired single matrices and vectors, and the clock. It also loads GTX textures made by gtxconv with GFDGetTextureInPlace and checks their formats,
mipmaps and in-place pointers. It assembles the cube example's shaders with latte-assembler
and checks GFD relocation, attribute names, uniforms and samplers. It tests CPU-side asset
loading; shader execution and rendered pixels need the normal emulator.

The runner writes guest output to stderr. `test` waits for an exact `DONE <n> failed` line and
fails for guest failures, an early exit or an exhausted instruction budget. `run` waits for
exit and returns the guest's status. Budgets count interpreter instructions and OS calls,
without a frame clock or GPU. Every invocation starts fresh, with the clock at 2026-01-01 UTC.

```sh
../../target/wiiu-toolchain/cemu-runner ../../target/wiiu-toolchain/test/test.rpx 'test 100000000'
../../target/wiiu-toolchain/cemu-runner ../../target/wiiu-toolchain/test/test.rpx 'run 100000000'
```

Only Cemu's interpreter sources are checked out, without its submodules. Its patches work like
the other repos, `./build.sh export-patches` also writes `../../target/wiiu-toolchain/cemu/`
back to `cemu/patches/` once it is there. The first patch removes the debugger, JIT and graphics
hooks from the interpreter; the second implements `crnand`, which clang emits in picolibc's
floating-point formatting.

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
  lines to `OSReport` and `gettimeofday` reads the time base. The programs are single threaded,
  so picolibc is built without locks
- `0002` adds `GFDGetTextureInPlace` to libgfd, which points a texture at its image and mipmaps in a
  loaded `.gtx` file instead of copying them

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
