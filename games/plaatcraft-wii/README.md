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

## Controls

Hold the Wii Remote with the Nunchuk attached, like the original PlaatCraft Wii port:

| Button        | Action                                    |
| ------------- | ----------------------------------------- |
| D-pad         | Move                                      |
| Nunchuk stick | Look around                               |
| A / B         | Jump or swim up, sneak; fly up and down   |
| Z / C         | Break or place a block, hold to repeat    |
| + / -         | Select a hotbar slot                      |
| 1 / 2         | Toggle flying and day/night               |
| Home          | Open the menu, point or use the D-pad + A |

## How it works

A worker thread generates, lights, and meshes the chunks around the player nearest first,
and keeps each chunk's blocks compressed for collision and picking. Lighting a chunk needs its
neighbors, so the worker keeps the unlit chunks next to the ones it still has to build instead
of generating every chunk again for each neighbor. Every vertex is a byte
position, a light palette index, and byte texture coordinates, so day and night only update
the palette. The block textures are converted to mipmapped CMPR or RGB5A3 textures during
the build. Edits live as long as the game runs, there is no saving yet.

## License

Copyright © 2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../LICENSE) license.
