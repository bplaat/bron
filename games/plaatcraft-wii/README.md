# PlaatCraft Wii

A Wii port of [PlaatCraft](../plaatcraft), the Minecraft like voxel sandbox. It shares the
world generation, lighting, meshing, and player physics of the
[plaatcraft-engine](../plaatcraft-engine) crate with the desktop and mobile game, and draws
the chunks as GX display lists. It's built with the [Wii toolchain](../../tools/wii-toolchain),
see the [Wii examples](../../examples/wii/README.md#getting-started) for the host tools.

## Run

The port is a standalone nightly project, separate from main Cargo workspace. Build it and
start it in Dolphin from this folder, `cargo run` also writes `target/wii/release/plaatcraft-wii.dol`:

```sh
cargo run --release
```

To play it on a Wii, run this from the repository root with the SD card connected. It builds the
Homebrew Channel bundle with the name, description and icon in `meta/` and copies it to
`apps/plaatcraft/` on the card, `cargo xtask build-wii-bundle plaatcraft` only builds it in
`target/bundle/wii/apps/`:

```sh
cargo xtask install-wii plaatcraft
```

## Controls

Hold the Wii Remote with the Nunchuk attached, the controls follow the console editions of
Minecraft and walking into a one block step always jumps on it:

| Button             | Action                                       |
| ------------------ | -------------------------------------------- |
| Nunchuk stick      | Walk forward and back, turn left and right   |
| D-pad up / down    | Look up and down                             |
| D-pad left / right | Select a hotbar slot                         |
| A / B              | Place or break a block, hold to repeat       |
| C                  | Jump, swim, or fly up, double tap to fly     |
| Z                  | Sneak or fly down                            |
| 2                  | Toggle day/night                             |
| + / Home           | Open the menu, point or use the D-pad + A, B |

## How it works

A worker thread generates, lights, and meshes the chunks around the player nearest first,
and keeps each chunk's blocks compressed for collision and picking. Lighting a chunk needs its
neighbors, so the worker keeps the unlit chunks next to the ones it still has to build instead
of generating every chunk again for each neighbor. Every vertex is a byte
position, a light palette index, and byte texture coordinates, so day and night only update
the palette. The block textures are converted to mipmapped CMPR or RGB5A3 textures during
the build. Edits live as long as the game runs, there is no saving yet.

The sound effects of the engine are rendered from their sfxr samples when the game starts, so
it only starts them while playing. The DSP mixes them with libogc's AESND and also changes
their pitch, volume and pan, the CPU only copies the samples to AESND's buffers.

## Benchmark

`bench.sh` builds the port with the `bench` feature, which times the chunk pipeline instead of
starting the game, runs it in Dolphin without its user interface and prints the milliseconds per
chunk. Dolphin's time base counts emulated cycles, so the results are repeatable but only
approximate the real Wii. `cargo run --release -p plaatcraft-engine --example bench` times the
same chunks on the host.

```sh
./bench.sh
```

## License

Copyright © 2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../LICENSE) license.
