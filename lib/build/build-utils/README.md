# Build utils

Shared helpers for Cargo build scripts: Unix runtime library discovery, exact-file
linking, symbol checks, search paths, and GTK capability flags.

Library-specific minimum versions, fallback choices, and environment overrides
remain in the consuming build scripts.

The Android module is also used by cargo-bundle and xtask.

## Android

Locates the Android SDK, NDK, and a JDK, and compiles a crate's Java sources to dex in its
build script, so crates can embed and load small Java helpers without the app or the
bundler knowing about them.

### Usage

```toml
[build-dependencies]
build-utils = { path = "../../build/build-utils" }
```

```rust,ignore
// build.rs
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        let out_dir = std::path::PathBuf::from(
            std::env::var_os("OUT_DIR").expect("OUT_DIR not set"),
        );
        build_utils::android::compile_dex(
            "android".as_ref(),
            &out_dir,
            build_utils::android::MIN_SDK,
        )
        .expect("failed to compile Android Java sources");
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
