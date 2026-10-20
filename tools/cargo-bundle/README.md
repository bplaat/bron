# cargo-bundle

A simple Cargo plugin that builds macOS and iOS app bundles and Windows MSIX packages.

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
`target/bundle` directory.

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

## License

Copyright © 2025-2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../LICENSE) license.
