# cargo-bundle

A simple Cargo plugin that builds macOS and iOS app bundles, Windows MSIX packages and Android
APKs.

## Usage

Bundle metadata is optional. By default, the bundle name is `package.name` and the
identifier is `nl.bplaat.<package.name>`. Override these defaults or configure resources
in your `Cargo.toml`:

```toml
[package.metadata.bundle]
name = "ExampleApp" # optional, defaults to package.name
identifier = "com.example.App" # optional, defaults to nl.bplaat.<package.name>
copyright = "Copyright © 2025 Anonymous" # optional
icon = "path/to/icon.icon" # optional, Icon Composer icon for macOS and iOS
resources_dir = "resources" # optional
info_plist = "Info.plist" # optional, merged into the macOS and iOS Info.plist

[package.metadata.bundle.macos]
minimal_os_version = "11.0" # optional
lipo = true # optional, build a universal x86_64 and arm64 binary
iconset = "path/to/icon.iconset" # optional, or icns = "path/to/icon.icns"
info_plist = "macos/Info.plist" # optional, merged into the macOS Info.plist
entitlements = "macos/Entitlements.plist" # optional
hardened_runtime = true # optional, defaults to whether there are entitlements

[package.metadata.bundle.ios]
info_plist = "ios/Info.plist" # optional, merged into the iOS Info.plist
entitlements = "ios/Entitlements.plist" # optional

[package.metadata.bundle.windows]
# Square44x44Logo.png, Square150x150Logo.png, StoreLogo.png and their qualified variants, like
# Square44x44Logo.targetsize-88_altform-unplated.png for a transparent taskbar icon
assets = "meta/windows"
sandbox = true # optional, run in an AppContainer
capabilities = ["internetClient"] # optional
file_types = [{ name = "Example Document", extensions = ["example"] }] # optional
```

Then run `cargo bundle` with the path argument, when using a Cargo workspace run the command in the root of the workspace:

```sh
cargo bundle --path path/to/your/crate
```

### macOS

This builds your crate for x86_64 and arm64, links both architectures into a universal binary,
creates an `.icns` file from the optional iconset, and creates a macOS app bundle in the
`target/bundle` directory. Add `--run` to launch the app afterwards with its console output
attached:

```sh
cargo bundle --path path/to/your/crate --run
```

### iOS

On macOS, `cargo bundle --ios` builds your crate for the iOS Simulator and creates an ad-hoc
signed app bundle in the `target/bundle/<crate>/ios` directory. The optional `icon` is compiled
for iPhone and iPad. Build, install and launch with console output in one command:

```sh
cargo bundle --path path/to/your/crate --ios --run
```

Running requires Xcode selected with `xcode-select`, an installed iOS Simulator runtime and
`rustup target add aarch64-apple-ios-sim`. The runner reuses a booted iOS simulator, or boots
an available one, and opens Simulator.app. Use `--device <name-or-UDID>` to select a device;
use a UDID if names are ambiguous. An existing instance of the app is replaced on launch.

`--run` uses a debug build; add `--release` for an optimized build. Bundling without `--run`
still uses release mode. Use `--bin <name>` for packages with multiple binaries. Cargo's
reported executable path is used, including when `CARGO_TARGET_DIR` is configured.
The application and its dependencies must support iOS; the runner does not translate desktop APIs.

The generated iOS Info.plist supports iPhone and iPad, portrait and landscape on iPhone, and all
orientations on iPad. Override these keys in the iOS Info.plist, for example to only allow portrait
on iPhone:

```xml
<key>UISupportedInterfaceOrientations</key>
<array>
    <string>UIInterfaceOrientationPortrait</string>
</array>
```

### Windows

This builds your crate for x64 and arm64, packs an unsigned `.msix` package for each architecture
and combines them into a `.msixbundle` in the `target/bundle` directory with `MakeAppx` from the
Windows SDK, so Windows installs the package for its own architecture. The package publisher is
the first package author with the
[unsigned package](https://learn.microsoft.com/en-us/windows/msix/package/unsigned-package) OID,
so it can be installed on Windows 11 from an administrator PowerShell:

```powershell
Add-AppxPackage -Path ExampleApp.msixbundle -AllowUnsigned -ForceUpdateFromAnyVersion
```

### Android

Build a signed APK for armeabi-v7a, arm64-v8a and x86_64 with `--android`, add `--run` to install and
start it on the connected device and follow its logcat output:

```sh
cargo bundle --path path/to/your/crate --android --run
```

The app must use [bwindow](../../lib/ui/bwindow). The bundle identifier is the Android package
name, with dashes converted to underscores, so every segment must start with a letter.
For example, `nl.bplaat.example-bcanvas-showcase` becomes `nl.bplaat.example_bcanvas_showcase`.
All Android settings are optional:

```toml
[package.metadata.bundle.android]
min_sdk_version = 26 # the default and the lowest supported level
target_sdk_version = 37 # defaults to the highest installed platform
manifest = "meta/android/AndroidManifest.xml"
res_dir = "meta/android/res"
```

The SDK, NDK, and JDK are found with [android-build](../../lib/build/android-build): through
`ANDROID_HOME` (default `~/Library/Android/sdk` on macOS and `~/Android/Sdk` on Linux), the
newest platform and build-tools, the newest NDK or `ANDROID_NDK_HOME`, and JDK 17 or 21 for
`apksigner` and `keytool`.
The Rust targets are installed with
`rustup target add armv7-linux-androideabi aarch64-linux-android x86_64-linux-android`.
The bundler:

1. Builds the binary as a shared library with the NDK clang, exporting only the
   `ANativeActivity_onCreate` entry point of bwindow.
2. Generates an `AndroidManifest.xml` without code that launches `android.app.NativeActivity`
   with the library, then merges the
   manifest file: its attributes on `<manifest>`, `<uses-sdk>`, `<application>`, and on
   elements with the same `android:name` win, other elements are appended.
3. Compiles and links the resources directory as-is with aapt2, a
   `mipmap*/ic_launcher` resource becomes the app icon.
4. Adds the uncompressed native libraries, aligns them to 16 KB pages,
   and signs the APK with `~/.android/debug.keystore`, creating that keystore when missing.

## License

Copyright © 2025-2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../LICENSE) license.
