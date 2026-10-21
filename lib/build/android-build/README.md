# Android build Rust library

Locates the Android SDK, NDK, and a JDK, and compiles a crate's Java sources to dex in its
build script, so crates can embed and load small Java helpers without the app or the
bundler knowing about them.

## Getting Started

```toml
[build-dependencies]
android-build = { path = "../../build/android-build" }
```

```rust,ignore
// build.rs
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
        android_build::compile_dex("android".as_ref(), &out_dir, android_build::MIN_SDK).unwrap();
    }
}
```

```rust,ignore
// Load it at runtime with bwindow's DexClassLoader
static DEX: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/classes.dex"));
```

The SDK is found through `ANDROID_HOME` (default `~/Library/Android/sdk` on macOS and
`~/Android/Sdk` elsewhere) and the NDK through `ANDROID_NDK_HOME` or the newest NDK in the
SDK. The JDK is `JAVA_HOME` when it is JDK 17, else an installed JDK 17, else JDK 21.

## License

Copyright © 2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../../LICENSE) license.
