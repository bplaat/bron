# PlaatCraft Wii U

A Wii U port of [PlaatCraft](../plaatcraft), the Minecraft like voxel sandbox. The game client
of the consoles lives in the `client` feature of the [plaatcraft-engine](../plaatcraft-engine)
crate, which the [Wii port](../plaatcraft-wii) shares, this port adds drawing with GX2, the
GamePad and the Wii Remote, AX sound, Cafe OS threads and nsysnet sockets. It's built with the
[Wii U toolchain](../../tools/wiiu-toolchain), see the
[Wii U examples](../../examples/wiiu/README.md#getting-started) for the host tools.

## Run

The port is a standalone nightly project, separate from main Cargo workspace. Build it and
start it in Cemu from this folder, `cargo run` also writes `target/wiiu/release/plaatcraft-wiiu.rpx`:

```sh
cargo run --release
```

To play it on a Wii U with Aroma, run this from the repository root with the SD card connected.
It builds the Wii U Menu bundle with the name, icon and splash screens in `meta/` and copies it
to `wiiu/apps/` on the card, `cargo xtask build-wiiu-bundle plaatcraft` only builds it in
`target/bundle/wiiu/apps/`:

```sh
cargo xtask install-wiiu plaatcraft
```

## Saved worlds

Worlds and settings are stored on the SD card, in the same folder as on the Wii:

```text
/data/plaatcraft/settings.db
/data/plaatcraft/worlds/My World.db
...
```

Worlds use the same SQLite files as the [desktop game](../plaatcraft) and the Wii, so they can
be copied between them. The game saves in the background every two minutes, when the pause
menu opens, and when leaving a world.

## Controls

The TV shows the game and the GamePad a copy of it. The GamePad plays like Minecraft: Wii U
Edition, a Wii Remote with a Nunchuk plays like on the Wii at the same time:

| GamePad      | Action                                |
| ------------ | ------------------------------------- |
| Left stick   | Walk, click it to run or fly faster   |
| Right stick  | Look around                           |
| ZL / ZR      | Break/place a block; hold to repeat   |
| A            | Jump, swim, fly up; double tap to fly |
| B            | Sneak or fly down                     |
| L / R, D-pad | Select a hotbar slot, or touch it     |
| X            | Full inventory                        |
| Y            | Pick the targeted block               |
| Minus        | Build tools, day/night, and chat      |
| +            | Pause menu                            |

The menus and the inventory are touched on the GamePad, or moved through with the D-pad, A
presses and B goes back. Text fields have an on-screen keyboard where B erases and + finishes.

## How it works

The render distance is fixed at 12 chunks. A worker on its own CPU core loads or generates,
lights, and meshes chunks nearest first, the network polls its sockets on another core. The
engine's chunk quads become GX2 vertex buffers with a day and a night color from the light
palette, so the night needs no new meshes. One Latte shader in `src/shader.vsh` and
`src/shader.psh` draws everything like GX: a texture times a color, with fog by the distance
from the eye. The textures are converted at build time by the toolchain's
[gtxconv](../../tools/wiiu-toolchain/gtxconv).

## Benchmark

`bench.sh` builds the port with the `bench` feature, which times the chunk pipeline instead of
starting the game, runs it headless in the Wii U toolchain's
[cemu-runner](../../tools/wiiu-toolchain) and prints the milliseconds per chunk. cemu-runner
interprets the CPU, so the results are far slower than the real Wii U.

```sh
./bench.sh
```

## License

Copyright © 2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../LICENSE) license.
