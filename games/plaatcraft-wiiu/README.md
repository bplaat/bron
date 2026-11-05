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

The GamePad shows player 1's full-screen view, including menus, chat, inventory and other
overlays. In split-screen, the TV shows only the other players: a full-screen view for 2 players,
horizontal halves for 3, and a two-by-two grid for 4-5. In single-player, the TV also shows
player 1. Player 1 uses the GamePad, with controls
like Minecraft: Wii U Edition. Additional local players use Wii Remotes with Nunchuks, with
the same [controls as on the Wii](../plaatcraft-wii/README.md#controls): the Nunchuk looks
around, the D-pad moves forward/backward and selects hotbar slots, B breaks, and A places.
C jumps, Z sneaks or flies down, 1 picks a block, 2 opens inventory, Minus opens tools, and
Plus pauses. HOME opens the Wii U system menu. No sensor bar is needed for looking.

GamePad controls, ordered by button name:

| GamePad          | Action                                |
| ---------------- | ------------------------------------- |
| A                | Jump, swim, fly up; double tap to fly |
| B                | Sneak or fly down                     |
| D-pad left/right | Previous/next hotbar slot             |
| L / R            | Previous/next hotbar slot             |
| Left stick       | Walk, click it to run or fly faster   |
| Minus (-)        | Build tools, day/night, and chat      |
| Plus (+)         | Pause menu                            |
| Right stick      | Look around                           |
| Touch screen     | Select a hotbar slot or menu item     |
| X                | Full inventory                        |
| Y                | Pick the targeted block               |
| ZL               | Break a block; hold to repeat         |
| ZR               | Place a block; hold to repeat         |

The menus and the inventory are touched on the GamePad, or moved through with the D-pad, A
presses and B goes back. D-pad navigation wraps from the first item to the last and back.
Text fields have an on-screen keyboard where B erases and + finishes.

For split-screen, pause a Local Play world and choose 2-5 players. Player 1 uses the GamePad;
the others use Wii Remotes 1-4 with Nunchuks, managed through HOME. Inventory, build tools,
and chat input stay in each player's view and are controlled by that player. Main and pause
menus are global and accept the GamePad and every connected Remote, including Remotes that
have not joined split-screen. Every connected Remote shows its cursor in
global menus, even before split-screen is enabled. Each player's inventory changes their
own hotbar.

## How it works

The render distance is 12 chunks, or 4-6 per split-screen view. A worker on its own CPU core
loads or generates, lights, and meshes chunks nearest first; the network polls its sockets
on another core. The
engine's chunk quads become GX2 vertex buffers with linear day and night colors from the light
palette, so the night needs no new meshes. One Latte shader in `src/shader.vsh` and
`src/shader.psh` draws with sRGB textures and output, smooth radial fog per pixel, and
exponential fog with a tint when immersed in water or lava. Water textures drift like the
desktop version, with the same base color mix and increasing opacity in distance fog.
The textures are converted at build time by the toolchain's
[gtxconv](../../tools/wiiu-toolchain/gtxconv).

GX2's alpha test rejects cutout texels before they write color or depth.

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
