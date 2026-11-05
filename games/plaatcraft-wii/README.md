# PlaatCraft Wii

A Wii port of [PlaatCraft](../plaatcraft), the Minecraft like voxel sandbox. The game client
of the consoles lives in the `client` feature of the [plaatcraft-engine](../plaatcraft-engine)
crate, which the [Wii U port](../plaatcraft-wiiu) shares, this port adds drawing with GX, the
Wii Remote, AESND sound, LWP threads and libogc's sockets. It's built with the [Wii toolchain](../../tools/wii-toolchain),
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

## Saved worlds

Worlds and settings are stored only on a FAT32 card in the Wii SD slot:

```text
/data/plaatcraft/settings.db
/data/plaatcraft/worlds/My World.db
/data/plaatcraft/worlds/Island.db
...
```

The game never saves to NAND, insert a FAT32 SD card before creating a world. Worlds use the
same SQLite files as the [desktop game](../plaatcraft), so they can be copied between a
computer and the SD card, close a world on the computer before copying it.

The game saves in the background every two minutes, when the pause menu opens, and when
leaving a world, because SD card writes are slow.

## Controls

Hold the Wii Remote with the Nunchuk attached. Controls are ordered by button name:

| Button           | Action                                  |
| ---------------- | --------------------------------------- |
| 1                | Pick the targeted block                 |
| 2                | Open/close inventory                    |
| A                | Place a block; hold to repeat           |
| B                | Break a block; hold to repeat           |
| C                | Jump, swim, fly up; double tap to fly   |
| D-pad left/right | Previous/next hotbar slot               |
| D-pad up/down    | Move forward/backward                   |
| Home             | Pause menu on Wii; system menu on Wii U |
| Minus (-)        | Build tools, day/night, and chat        |
| Nunchuk stick    | Look left/right/up/down                 |
| Plus (+)         | Pause menu                              |
| Z                | Sneak or fly down                       |

The Nunchuk looks around and the D-pad moves forward/backward. Camera control needs no
sensor bar or motion tracking. Pointing is optional for menu cursors; the D-pad can navigate
all menus. In the inventory, Z/C selects the previous/next hotbar slot.

The menus are the desktop game's: the title screen over the turning default world with Local
Play, Online Play and Settings, the world list, the LAN world list, and the pause menu.
Point with the Wii Remote or move with the D-pad, A presses and B goes back. D-pad navigation
wraps from the first item to the last and back. Text fields have an on-screen keyboard where
B erases and + finishes. In the inventory, the D-pad browses
the blocks, A assigns a block to the selected hotbar slot, Z/C changes the slot, and B or
2 closes it. The on-screen keyboard accepts commands and chat. Build tools provide `/pos1`, `/pos2`,
`/fill`, `/clear`, and `/undo` without typing. A selection holds up to 32768 blocks;
undo remembers up to 32 changes within a 65536-block memory budget.

For split-screen, pause a Local Play world and choose 2-4 players, each with a Wii Remote
and Nunchuk. Inventory, build tools, and chat input stay in each player's view and are
controlled by that player. Main and pause menus accept every connected controller, including
Remotes that have not joined split-screen. All connected Remotes show their cursors in these
menus, even before split-screen is enabled.
Each player's inventory changes their own hotbar.

## How it works

The render distance is 8 chunks, or 4 per split-screen view. A worker loads or generates, lights, and meshes
chunks nearest first, edited chunks before their neighbors, and the main thread keeps their
compressed blocks for collision and picking. The engine's chunk quads become GX display lists
with fixed-point positions for slabs, stairs, and liquid heights, with per-corner lighting from
a palette. The sky layers, held block, breaking particles, player models, and textures
follow the desktop version. Water and lava use the shared engine's flow rules, with a
smaller update budget on the Wii.

The engine synthesizes sound samples at startup; libogc's AESND mixes them on the DSP.
Settings include sound volume, debug info, auto jump, and the username, the render distance
is fixed.

The menus, the game rules of the integrated server (commands, undo, flowing fluid and the
players' physics), client prediction, and the LAN protocol come from the shared engine, so
the Wii plays like the desktop version and they play together on the LAN. Wii hosting limits
a world to three guests. Socket I/O runs on its own thread; remote players use authoritative
physics and client prediction.

## Benchmark

`bench.sh` builds the port with the `bench` feature, which times the chunk pipeline instead of
starting the game, runs it headless in the Wii toolchain's
[dolphin-runner](../../tools/wii-toolchain) and prints the milliseconds per chunk. Dolphin's time
base counts emulated cycles, so the results are repeatable but only approximate the real Wii.
`cargo run --release -p plaatcraft-engine --example bench` times the same chunks on the host.

```sh
./bench.sh
```

## License

Copyright © 2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../LICENSE) license.
