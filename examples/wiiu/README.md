# Wii U Examples

Wii U homebrew example programs written in both C and Rust. For now they are only tested in the
[Cemu](https://cemu.info/) emulator.

## Getting Started

The examples build with a normal LLVM toolchain (clang, lld and llvm-ar). The [Wii U toolchain](../../tools/wiiu-toolchain/)
script clones upstream wut, wut-tools, picolibc, LLVM's compiler-rt builtins and decaf-emu's shader
assembler, patches and builds them the first time you build an example. Install LLVM, meson and
ninja (to build picolibc), zlib and the [Cemu](https://cemu.info/) emulator:

```sh
# macOS
brew install llvm lld meson ninja
brew install --cask cemu

# Debian / Ubuntu
sudo apt install clang lld llvm make git meson ninja-build zlib1g-dev
```

Install [rustup](https://rustup.rs/) for Rust builds. The Cargo wrapper installs the pinned nightly and its Rust sources.

From the Bron repository root, build and run a C example with make, the program ends up in `target/<name>.rpx`:

```sh
cd examples/wiiu/triangle/c
make run
```

Use the toolchain's Cargo wrapper to build a Rust example with patched `std` and run it in Cemu.
It writes `target/powerpc-nintendo-wiiu/release/<name>.rpx` in the standalone project directory.

```sh
cd examples/wiiu/triangle/rust
../../../../tools/wiiu-toolchain/cargo run --release --locked
```

All examples draw on the TV with libwhb and copy the image to the GamePad. Their shaders are written in Latte
assembly in `src/shader.vsh` and `src/shader.psh`. Their images are converted at build time to BC1
compressed textures with mipmaps by the toolchain's [gtxconv](../../tools/wiiu-toolchain/gtxconv),
from `src/assets/textures.toml`, and the GPU reads them in place from the program.

## Triangle

A minimal program that draws a rotating triangle with vertex colors.

## Cube

A rotating crate cube scene.

## Blocks

Generates a random 64x64x64 voxel world and renders its visible block faces.

## License

Copyright © 2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../LICENSE) license.
