# PlaatCraft

A Minecraft like voxel sandbox written in Rust for macOS, Linux, Windows, iOS, and
Android. It's built with [bwindow](../../lib/ui/bwindow), [bcanvas](../../lib/graphics/bcanvas),
and [bwebgpu](../../lib/graphics/bwebgpu) on Metal, Vulkan, or Direct3D 12.

The world generation, lighting, meshing, and player physics live in the `no_std`
[plaatcraft-engine](../plaatcraft-engine) crate, which the [Wii port](../plaatcraft-wii) shares.

## Features

- Endless generated terrain with biomes, trees, plants, ores, water, and lava
- Water and lava that flow, fall, and dry up like in Minecraft, two water sources make a
  new one and lava hardens into stone in water
- Break and place blocks, with a hotbar and an inventory of every block, the held block
  swings in your hand and broken blocks burst into bits
- Slabs and stairs of stone, planks, and bricks that you walk up
- Smooth ambient occlusion in the corners between blocks
- Build big with `/pos1`, `/pos2`, `/fill`, and `/clear`, and take changes back with `/undo`
- Walking, swimming, and flying
- Day and night with a layered skybox and voxel lighting
- Multiple worlds, each with its own random seed, that you create, rename, and delete
- Your worlds, edits, and player are saved automatically
- LAN multiplayer like Minecraft: open a world to the LAN from the pause menu and others
  find it in Multi Player, or join with Direct Connect
- Players look like the [Kenney](https://kenney.nl/assets/voxel-pack) voxel character,
  named after your system account unless you choose a username
- Sound effects synthesized from filtered noise and resonant tones, mixed by [baudio](../../lib/audio/baudio)
- Settings with a 4-128 chunk Render Distance slider, a Sound Volume slider,
  a Show Debug Info toggle, your username, and saved preferences
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

## Multiplayer

Pause a single player world and choose **Open to LAN**, other players on the network then
see it in **Multi Player** and join it with a double click. **Direct Connect** joins by
address, like `192.168.1.5` or `192.168.1.5:47820`. Edits are shared, everyone sees the
others walk around with their name above their head, and guests continue where they left
the world, which stays one file on the host.

| Port  | Protocol | Used for                                          |
| ----- | -------- | ------------------------------------------------- |
| 47820 | TCP      | Joining, chunks, blocks, edits, and saving guests |
| 47820 | UDP      | Player positions, twenty times a second           |
| 47821 | UDP      | Broadcasts that announce open worlds on the LAN   |

When another world on the computer already uses port 47820, the world opens on a free
port and announces that one. iOS only receives the LAN broadcasts with Apple's multicast
networking entitlement, Direct Connect works without it.

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
| T / Enter / /       | Open the chat, `/` starts a command     |
| Escape              | Pause and return to the menu            |

Walking into a one block step jumps on it, auto jump can be turned off in the settings.

The chat in the bottom left sends messages to the other players and runs commands:
`/fly on|off` starts or stops flying, `/time day|night` changes the time for everyone,
and `/help` lists them. To build big, look at a block and mark the corners of a box with
`/pos1` and `/pos2`, then `/fill` fills it with the held block and `/clear` empties it, up
to 32768 blocks. `/undo` takes back your last fill, clear, or block edit, up to 32 times.

On touch screens, drag on the left side to move and on the right side to look
around, tap to place a block and hold to break one, like Minecraft Pocket Edition.
The on-screen buttons do the other actions.

## How it works

Each world is a single SQLite file in the `worlds` folder of the app data directory,
named after the world. Playing a world starts an integrated server that owns it: the
client only talks to it with messages, to request chunks and blocks, edit blocks, chat,
and send its controls. The host passes these messages over channels without copying them,
LAN guests send them over TCP and UDP, with the chunks deflate compressed.

The server runs the physics of every player twenty times a second from their controls, so
players keep falling while their game is paused and fall when the ground under them breaks.
Clients predict their own player with the same physics, replay the controls the server has
not handled yet when its state arrives, and smooth out the difference. The server's
background threads load or generate and light chunks of
16 x 16 x 128 blocks, which are stored compressed in the world. The client meshes them
on its own threads, then the main thread uploads the meshes and renders the world in one
render pass: terrain, sky, translucent blocks, and the bcanvas drawn HUD on top. Shaders
are written in WGSL and translated for each platform during the build.

## Screenshot

![PlaatCraft Screenshot](docs/images/screenshot.png)

## License

Copyright © 2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../LICENSE) license.
