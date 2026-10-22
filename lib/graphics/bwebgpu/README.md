# bwebgpu Rust library

A native GPU rendering library for [bwindow](../../ui/bwindow) and offscreen bitmaps,
backed by Metal on macOS and iOS, Direct3D 12 on Windows, and Vulkan on Linux and Android.

Its API is modeled after WebGPU and upstream `wgpu`, but this crate is not
API-compatible with either and is not a replacement. It currently supports the
features needed by this repository, including surfaces, buffers, textures, bind
groups, shaders, pipelines, render passes, and bitmap readback.

## Features

- `window` (default): presentation surfaces for [bwindow](../../ui/bwindow) windows
- `image` (default): offscreen render targets read back as
  [image](../image) bitmaps

At least one feature must be enabled. Disable default features and enable only
`image` for headless rendering without any GTK, AppKit view, Win32, or Android window
code.

## Backends

| Operating system | Backends               | Fallback                                              |
| ---------------- | ---------------------- | ----------------------------------------------------- |
| macOS / iOS      | Metal                  | None                                                  |
| Windows          | Direct3D 12 (fl 11_0+) | D3D12 hardware, then the D3D12 WARP software renderer |
| Linux            | Vulkan 1.1+            | A software Vulkan implementation, when installed      |
| Android          | Vulkan 1.1+            | None                                                  |

## Getting Started

Create a surface from a bwindow content attachment:

```rust,no_run
let window = bwindow::WindowBuilder::new().build();
let instance = bwebgpu::Instance::default();
let surface = instance
    .create_surface(window.attach_content().expect("attach graphics surface"))
    .expect("create surface");
let (width, height) = surface.size();
```

For headless rendering, request an adapter without a surface, then create a
bitmap target with `device.create_bitmap_target(width, height)`. Render into
`target.create_view()`, submit the commands, and call `target.read(&queue)` to
obtain an `image::Bitmap`. The read waits for the GPU to finish. Encode it with
`image::encode(&bitmap, image::Format::Png)` or another supported format.

WGSL shaders are compiled at build time with
[bwebgpu-shader-build](../bwebgpu-shader-build) and embedded as Metal libraries,
SPIR-V, or Direct3D bytecode. Native GPU objects remain on their creating
thread, and surface creation must happen on the main thread on macOS and iOS.

## Rendering API

Draw commands follow upstream `wgpu`: `draw(vertices, instances)` and
`draw_indexed(indices, base_vertex, instances)` take ranges. Create index buffers
with `BufferUsages::INDEX`, then bind a buffer slice with
`set_index_buffer(slice, IndexFormat::Uint16)` or `IndexFormat::Uint32`.
Buffer slice offsets, index ranges, signed base vertices and starting instances
are preserved on all backends. The pass retains its buffers until the GPU finishes.
Invalid usages, alignment and index ranges panic.

Vertex layouts support `Float32x2`, `Float32x3`, `Float32x4`, `Uint32x2`,
`Uint8x4`, `Unorm8x4` and `Sint16x4`. `vertex_attr_array!` accepts multiple
`location => Format` entries and computes their packed byte offsets.

`CompareFunction` provides all eight depth comparisons. `BlendState` has separate
color and alpha `BlendComponent`s, with configurable factors and operations, plus
`REPLACE`, `ALPHA_BLENDING` and `PREMULTIPLIED_ALPHA_BLENDING` presets.
Samplers expose U/V/W addressing and `lod_min_clamp` / `lod_max_clamp`.

The API remains a subset: one vertex buffer, one bind group with fixed binding
roles, and one color target are supported. Compute, MSAA, stencil, anisotropic
filtering and GPU mipmap generation are not implemented.

## Examples

```sh
cargo run -p example-bwebgpu-triangle
cargo run -p example-bwebgpu-cube
cargo run -p example-bwebgpu-blocks
cargo run -p example-bwebgpu-headless
```

See [bwebgpu-triangle](examples/triangle),
[bwebgpu-cube](examples/cube),
[bwebgpu-blocks](examples/blocks), and
[bwebgpu-headless](examples/headless). The headless example writes
`frame.png` in the current directory.

The triangle uses interleaved position and RGBA vertex attributes. The cube uses
24 vertices and 36 indices. Blocks uses an indexed quad for each face instance,
strict `Less` depth testing and separate water color/alpha blend components,
matching the native Metal and OpenGL examples' draw structure. Its mipmaps are
generated on the CPU and uploaded once.

## License

Copyright © 2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../../LICENSE) license.

## iOS Simulator

From the workspace root on macOS, build, install and launch the blocks example with:

```sh
cargo run -p cargo-bundle -- bundle --path lib/graphics/bwebgpu/examples/blocks --ios --run
```

This requires the Xcode iOS Simulator SDK and runtime, the Metal compiler, and the
`aarch64-apple-ios-sim` Rust target. Add `--device <name-or-UDID>` to select a simulator.
The iOS surface uses a CAMetalLayer and on-demand CADisplayLink animation callbacks.

## Android

From the workspace root, build, install and launch the blocks example on the connected
device or emulator with:

```sh
cargo run -p cargo-bundle -- bundle --path lib/graphics/bwebgpu/examples/blocks --android --run
```

The surface presents to the whole activity window, under the system bars, so
`surface.size()` is the window size in pixels. Keep overlays inside `window.safe_area()`.
Animation frames are scheduled with the display choreographer. The framework replaces the
native window when the app returns to the foreground: until it exists `get_current_texture`
returns `Occluded`, afterwards `Outdated`, so configure the surface again on `Resize` or
`Outdated`. Surfaces support `Rgba8Unorm` and `Rgba8UnormSrgb`.
