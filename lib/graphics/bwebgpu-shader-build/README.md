# bwebgpu-shader-build Rust library

Build-time WGSL compilation for this repository's [bwebgpu](../bwebgpu) crate. It
uses Naga to translate each registered shader only to the format required by the
Cargo target, then embeds the target-native output:

- macOS and iOS: MSL is compiled to AIR and linked into a Metal library with `xcrun`.
- Linux and Android: Naga emits SPIR-V directly.
- Windows: HLSL is compiled to DXBC with `d3dcompiler_47.dll`.

Native compilation uses the host platform toolchain; cross-compilation is not
supported. On macOS, `MACOSX_DEPLOYMENT_TARGET` is forwarded to the Metal
compiler and changes to it trigger a shader rebuild. iOS uses `IPHONEOS_DEPLOYMENT_TARGET`
and selects the device or simulator SDK from the Cargo target. Metal libraries contain
GPU-neutral AIR, so they do not require separate x86_64 and arm64 slices.

## Getting Started

Add the crate as a build dependency and register shaders in `build.rs`:

```rust,no_run
fn main() -> bwebgpu_shader_build::Result<()> {
    bwebgpu_shader_build::Builder::new()
        .shader("triangle.wgsl", "src/triangle.wgsl")
        .compile()
}
```

Load the generated target-specific descriptor in the application:

```rust,ignore
let shader = device.create_shader_module(bwebgpu::include_wgsl!("triangle.wgsl"));
```

Shader paths are relative to `CARGO_MANIFEST_DIR`. Generated files are stored in
Cargo's `OUT_DIR` and rewritten only when their contents change.

## License

Copyright © 2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../../LICENSE) license.
