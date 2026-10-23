# PlaatCraft

A first-person voxel sandbox written in Rust. Explore a generated landscape,
break and place blocks, and switch between walking and flying. This is the next
desktop version of the original C/OpenGL PlaatCraft game. It uses bwindow and the
workspace's small bwebgpu implementation, backed by Metal, Vulkan, or Direct3D 12.

## Features

- Deterministic terrain with seas, rivers, beaches, plains, forests, deserts,
  taiga, tundra, and mountains. Temperature and humidity influence the biomes.
- Oak, birch, pine, and autumn trees, grass, desert cacti, underground ores,
  bedrock, and occasional lava vents.
- Walking with gravity, jumping, sprinting, slow movement, and block collision;
  flying with vertical movement and a faster travel mode.
- Block removal, placement, material picking, and mouse-wheel material selection.
  A white outline marks the target, and a rotating corner preview shows the
  selected block. Construction materials include bricks, planks, colored wool,
  glass, ice, ores, several soils and stone variants, and a crafting table.
- Translucent water and glass, blue underwater fog, and
  dense red fog inside lava. Both liquids are passable; lava glows even at night.
- A layered Kenney skybox with hills, clouds, sun, moon, a day/night toggle, and
  voxel lighting from the sky and emissive lava.
- Background chunk loading and generation, bounded mesh caching, and a circular
  residency radius of 48 chunks. Chunks are 16 x 16 blocks and 128 blocks tall.
- Persistent terrain, block edits, camera position/orientation, movement mode,
  day/night setting, selected material, and window state in SQLite.
- Window resizing, pointer lock, and live position/FPS statistics in the title bar.

## Run

Use a recent stable Rust toolchain and run from the workspace root:

```sh
cargo run --release -p plaatcraft
```

SQLite development libraries must be available to `libsqlite3-sys`. The windowing
and graphics requirements depend on the platform:

| Platform | Graphics backend | Requirements                                                        |
| -------- | ---------------- | ------------------------------------------------------------------- |
| macOS    | Metal            | Metal-capable Mac and Apple command-line developer tools            |
| Linux    | Vulkan           | Vulkan 1.1 loader/driver and a GTK 3 desktop                        |
| Windows  | Direct3D 12      | D3D12-capable device, Windows build tools, and `d3dcompiler_47.dll` |

Textures are embedded in the executable. Shaders are translated during the Cargo
build, so no separate shader compiler command is needed.

The game creates `world.db` in its working directory. Chunk data is compressed,
and SQLite uses WAL mode. Session state is saved periodically and on normal exit;
block edits are persisted as they happen. Reopening the same database restores
the world and player state.

## Controls

Click the window to lock the pointer and start playing. Losing focus releases it.

| Input               | Action                                          |
| ------------------- | ----------------------------------------------- |
| Mouse movement      | Look around                                     |
| W / A / S / D       | Move                                            |
| F                   | Toggle walking/flying                           |
| Space               | Jump while walking; ascend while flying         |
| Shift               | Move slowly while walking; descend while flying |
| Ctrl                | Sprint while walking; fly faster                |
| Left mouse button   | Break the targeted block                        |
| Right mouse button  | Place the selected material next to the target  |
| Middle mouse button | Select the targeted block's material            |
| Mouse wheel         | Cycle materials                                 |
| N                   | Toggle day/night                                |
| Escape              | Release the mouse                               |

## How rendering works

The CPU generates and meshes the world. The GPU expands compact face records into
triangles and shades them. There is one scene render pass per frame, with six
pipelines selected in a fixed order.

```mermaid
flowchart LR
    A[Load or generate chunks] --> B[Bake voxel lighting]
    B --> C[Greedy mesh and section ranges]
    C --> D[Upload packed faces]
    D --> E[Opaque terrain]
    E --> F[Sky]
    F --> G[Translucent blocks]
    G --> H[Selection outline]
    H --> I[Material preview]
    I --> J[Crosshair]
    J --> K[Submit and present]
```

### Chunk preparation

Terrain workers load saved voxel data or generate missing chunks, applying edits
and computing lighting. Skylight and block light each use four bits per voxel;
light propagates through the surrounding terrain, with attenuation through water,
glass, and leaves. A one-block halo supplies neighbors at chunk boundaries.

The greedy mesher removes hidden faces and combines adjacent coplanar faces with
the same material and lighting. It retains solid faces bordering translucent
blocks or lava. Internal faces between identical translucent blocks are removed.

Each resulting quad occupies just eight bytes: two packed `u32` values encode
its position, orientation, material, dimensions, section, and lighting. During
upload, the renderer adds a slot identifying the chunk's origin. The vertex shader
uses the vertex index to expand each instance into six vertices, forming two
triangles. Grass uses crossed planes with both windings.

Chunks are divided into eight vertical sections. Each section has separate opaque
and translucent instance ranges. Nearby chunks receive loading priority, with visible
chunks preferred; upload work stops after 32 chunks or roughly one millisecond
per frame. Chunk and section frustum tests reduce opaque draw calls.

### Shared resources

All six pipelines share a scene binding layout:

| Binding | Resource                 | Purpose                                                                                                    |
| ------- | ------------------------ | ---------------------------------------------------------------------------------------------------------- |
| 0       | 96-byte uniform buffer   | Camera basis/projection, screen size, distance fog, time, daylight, immersion state, and selected material |
| 1       | sRGB 2D texture array    | 68 block layers followed by eight layered sky images, each with eight mip levels                           |
| 2       | Sampler                  | Repeating textures, nearest magnification, and linear minification/mipmap filtering                        |
| 3       | Read-only storage buffer | Chunk origins relative to the camera's current chunk                                                       |

CPU positions use double precision, while GPU coordinates are relative to the
current camera chunk. Crossing a chunk boundary updates the origin table without
rebuilding every resident mesh, keeping shader coordinates small during travel.

Mipmaps are generated on the CPU using alpha-weighted filtering and coverage
preservation. This reduces dark fringes and keeps small grass/leaf details from
disappearing at a distance.

### Frame order

Each frame updates movement and chunk residency, acquires a surface texture, and
uploads camera and selection data. The color attachment clears to black, and the
Depth32Float attachment clears to the far depth.

| Order | Pipeline  | Behavior                                                                                                                                              |
| ----- | --------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1     | Terrain   | Draw opaque blocks, lava, and alpha-tested vegetation near to far; test and write depth. Transparent cutout pixels are discarded.                     |
| 2     | Sky       | Draw a fullscreen triangle at far depth, filling pixels not covered by terrain.                                                                       |
| 3     | Liquid    | Draw translucent liquid chunks and sections far to near with alpha blending; test depth without writing it. Sorting is approximate, not per triangle. |
| 4     | Selection | Expand the target block's edges into screen-width triangles; test against scene depth.                                                                |
| 5     | Preview   | Draw the selected material inside a small viewport and scissor rectangle, ignoring scene depth.                                                       |
| 6     | Crosshair | Draw a scissored fullscreen triangle whose fragment shader keeps only the center cross, ignoring scene depth.                                         |

The selection outline appears only when there is a target. The preview and
crosshair appear while the pointer is locked.

Surface rendering and liquid immersion are handled separately:

| Shader                                   | Surfaces drawn                                                       | Water and lava effects                                                                      |
| ---------------------------------------- | -------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| [terrain.wgsl](src/shaders/terrain.wgsl) | Opaque blocks, emissive lava surfaces, and cutout vegetation         | Applies blue water fog or dense red lava fog according to the material at the camera's eye. |
| [liquid.wgsl](src/shaders/liquid.wgsl)   | Translucent water and glass, with drifting water texture coordinates | Applies the same water or lava immersion tint and fog to translucent surfaces.              |
| [sky.wgsl](src/shaders/sky.wgsl)         | Layered skybox with hills, clouds, sun, and moon                     | Fills uncovered pixels with the matching blue or red glow while submerged.                  |

`liquid.wgsl` therefore includes lava immersion shading, while lava surface geometry
is drawn by the terrain pipeline. Water samples its drifting tile, while glass samples
its own material layer. The red tint depends on the camera being inside lava.

Terrain shading combines tile color, face brightness, skylight, block light, and
distance fog. The eye's actual voxel material selects air, water, or lava effects,
so immersion works at any elevation. Lava's red glow remains visible at night.
These effects are calculated in the scene shaders, without a separate fullscreen
fog or bloom pass.

Finally, the renderer submits commands and presents with vsync. The local graphics
crate handles native uploads, synchronization, and two frames in flight, retaining
resources until the GPU finishes using them.

### Native graphics layer

`../../lib/graphics/bwebgpu` implements only the API this game consumes. Its runtime Rust dependencies
are `bwindow` and, on macOS, `objc2`. Linux and Windows use direct FFI to
Vulkan and D3D12/DXGI respectively.

WGSL remains the shader source of truth. The build script explicitly registers
shader names and source paths with `bwebgpu-shader-build`, which uses Naga to validate
and translate them into MSL, SPIR-V, and HLSL, embedding only the target's
format. Metal compiles MSL through its system API, Vulkan consumes SPIR-V, and
Windows compiles HLSL through `D3DCompile`. Naga is a build dependency only.

See [the graphics crate README](../../lib/graphics/bwebgpu/README.md) for its supported subset and
backend verification details. Metal has GPU readback tests; Linux and Windows
have compilation checks but still need native rendering tests.

## Development

```sh
cargo test --workspace -- --test-threads=1
cargo clippy --workspace --all-targets -- -D warnings -W clippy::uninlined_format_args
```

On a Mac with a Metal GPU, also run the native rendering test:

```sh
cargo test -p bwebgpu -- --include-ignored
```

Game tests cover terrain, lighting, meshing, collision, picking/editing, streaming,
and persistence. Shader translation runs during builds for all three output
formats. Rendering regression tests include liquid boundaries and immersion
uniforms; the native Metal test checks rendered pixels and resource lifetimes.
