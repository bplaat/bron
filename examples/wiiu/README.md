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

The Rust examples install the nightly toolchain by themselves with [rustup](https://rustup.rs/).

From the Bron repository root, build and run a C example with make, the program ends up in `target/<name>.rpx`:

```sh
cd examples/wiiu/triangle/c
make run
```

Build and run a Rust example with cargo, `cargo run` also writes `target/wiiu/release/<name>.rpx`:

```sh
cd examples/wiiu/triangle/rust
cargo run --release
```

The Rust examples are standalone nightly projects, separate from the main Cargo workspace.
Run Cargo from the example directory so it loads the Wii U target and runner configuration.

All examples draw on both the TV and the GamePad with libwhb. Their shaders are written in Latte
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
