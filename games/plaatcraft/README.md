# PlaatCraft

A Minecraft like voxel sandbox written in Rust for macOS, Linux, Windows, iOS, and
Android. It's built with [bwindow](../../lib/ui/bwindow), [bcanvas](../../lib/graphics/bcanvas),
and [bwebgpu](../../lib/graphics/bwebgpu) on Metal, Vulkan, or Direct3D 12.

## Features

- Endless generated terrain with biomes, trees, plants, ores, water, and lava
- Break and place blocks, with a hotbar and an inventory of every block
- Walking, swimming, and flying
- Day and night with a layered skybox and voxel lighting
- Your world, edits, and player are saved automatically
- Settings with a 4-128 chunk Render Distance slider,
  Show Debug Info toggle, and saved preferences
- On-screen touch controls on iOS and Android

## Run

From the workspace root:

```sh
cargo run --release -p plaatcraft
```

Package it with [cargo-bundle](../../tools/cargo-bundle) for macOS, iOS, Android, or
Windows, add `--run` to install and start it:

```sh
cargo bundle --path games/plaatcraft --ios --run
cargo bundle --path games/plaatcraft --android --run
```

Textures use `cargo-bundle-assets`: normal Cargo builds embed them in the binary,
while `cargo bundle` stores them as separate resource files. Universal macOS
executables and multi-ABI Android APKs share one texture collection.

## Controls

| Desktop             | Action                                  |
| ------------------- | --------------------------------------- |
| Mouse / WASD        | Look around and move                    |
| Space / Shift       | Jump or swim up, sneak; fly up and down |
| Ctrl                | Run or fly faster                       |
| Left / Right mouse  | Break or place a block                  |
| Middle mouse        | Pick the targeted block                 |
| 1 - 9 / Mouse wheel | Select a hotbar slot                    |
| E                   | Open the inventory                      |
| F / N               | Toggle flying and day/night             |
| Escape              | Pause and return to the menu            |

On touch screens, drag on the left side to move and on the right side to look
around, tap to place a block and hold to break one, like Minecraft Pocket Edition.
The on-screen buttons do the other actions.

## How it works

Background threads generate, light, and mesh chunks of 16 x 16 x 128 blocks, which
are stored compressed in SQLite. The main thread uploads the meshes and renders the
world in one render pass: terrain, sky, translucent blocks, and the bcanvas drawn HUD
on top. Shaders are written in WGSL and translated for each platform during the
build.

## Screenshot

![PlaatCraft Screenshot](docs/images/screenshot.png)

## License

Copyright © 2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../LICENSE) license.
