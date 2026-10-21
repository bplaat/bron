# wgpu Rust library

A native GPU rendering library for [bwindow](../../ui/bwindow) and offscreen bitmaps,
backed by Metal on macOS and iOS, Direct3D 12 on Windows, and Vulkan on Linux.

Despite sharing its name and part of its API with upstream `wgpu`, this crate is
not API-compatible with it and is not a replacement. It currently supports the
features needed by this repository, including surfaces, buffers, textures, bind
groups, shaders, pipelines, render passes, and bitmap readback.

## Features

- `window` (default): presentation surfaces for [bwindow](../../ui/bwindow) windows
- `image` (default): offscreen render targets read back as
  [image](../image) bitmaps

At least one feature must be enabled. Disable default features and enable only
`image` for headless rendering without any GTK, AppKit view, or Win32 window
code.

## Backends

| Operating system | Backends               | Fallback                                              |
| ---------------- | ---------------------- | ----------------------------------------------------- |
| macOS / iOS      | Metal                  | None                                                  |
| Windows          | Direct3D 12 (fl 11_0+) | D3D12 hardware, then the D3D12 WARP software renderer |
| Linux            | Vulkan 1.1+            | A software Vulkan implementation, when installed      |

## Getting Started

Create a surface from a bwindow content attachment:

```rust,no_run
let window = bwindow::WindowBuilder::new().build();
let instance = wgpu::Instance::default();
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
[wgpu-shader-build](../wgpu-shader-build) and embedded as Metal libraries,
SPIR-V, or Direct3D bytecode. Native GPU objects remain on their creating
thread, and surface creation must happen on the main thread on macOS and iOS.

## Examples

```sh
cargo run -p example-wgpu-triangle
cargo run -p example-wgpu-cube
cargo run -p example-wgpu-blocks
cargo run -p example-wgpu-headless
```

See [wgpu-triangle](examples/triangle),
[wgpu-cube](examples/cube),
[wgpu-blocks](examples/blocks), and
[wgpu-headless](examples/headless). The headless example writes
`frame.png` in the current directory.

## License

Copyright © 2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../../LICENSE) license.

## iOS Simulator

From the workspace root on macOS, build, install and launch the blocks example with:

```sh
cargo run -p cargo-bundle -- bundle --path lib/graphics/wgpu/examples/blocks --ios --run
```

This requires the Xcode iOS Simulator SDK and runtime, the Metal compiler, and the
`aarch64-apple-ios-sim` Rust target. Add `--device <name-or-UDID>` to select a simulator.
The iOS surface uses a CAMetalLayer and on-demand CADisplayLink animation callbacks.
