# Wii Examples

Wii homebrew example programs written in both C and Rust.

## Getting Started

The examples build with a normal LLVM toolchain (clang, lld and llvm-ar). The [Wii toolchain](../../tools/wii-toolchain/)
script clones upstream libogc, devkitPro's tools and LLVM's compiler-rt builtins, patches and builds
them the first time you build an example. Install LLVM and the
[Dolphin](https://dolphin-emu.org/) emulator:

```sh
# macOS
brew install llvm lld
brew install --cask dolphin

# Debian / Ubuntu
sudo apt install clang lld llvm make git dolphin-emu
```

The toolchain builds its texture converter with Cargo, so all examples need [rustup](https://rustup.rs/). The Rust examples install the nightly toolchain by themselves.

From the Bron repository root, build and run a C example with make, the program ends up in `target/<name>.dol`:

```sh
cd examples/wii/triangles/c
make run
```

Build and run a Rust example with cargo, `cargo run` also writes `target/wii/release/<name>.dol`:

```sh
cd examples/wii/triangles/rust
cargo run --release
```

The Rust examples are standalone nightly projects, separate from main Cargo workspace.
Run Cargo from the example directory so it loads the Wii target and runner configuration.

## Triangles

A minimal program that draws a few triangles using a display list.

## Canvas

2D rendering with PNG textures, bitmap font text and Wii Remote cursors.

## Blocks

Generates a random 64x64x64 voxel world and renders its visible block faces with GX display lists.

## License

Copyright © 2023-2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../LICENSE) license.
