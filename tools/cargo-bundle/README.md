# cargo-bundle

A simple Cargo plugin that builds macOS and iOS app bundles, Windows MSIX packages, Linux `.deb`
and `.rpm` packages and Android APKs.

## Usage

Bundle metadata is optional. By default, the bundle name is `package.name` and the
identifier is `nl.bplaat.<package.name>`. The `description`, `license`, `authors`, `homepage` and
`repository` package fields, also when inherited from the workspace, describe the Linux packages. Override these defaults or configure resources
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

[package.metadata.bundle.windows.shell_extension] # optional
package = "../example-shell" # cdylib crate exporting DllGetClassObject and DllCanUnloadNow
library = "example_shell" # its DLL name without .dll
thumbnail_handler = "00000000-0000-0000-0000-000000000001" # optional IThumbnailProvider CLSID
preview_handler = "00000000-0000-0000-0000-000000000002" # optional IPreviewHandler CLSID

[package.metadata.bundle.linux]
icons = "meta/freedesktop/icons" # optional, <size>x<size>.png and scalable.svg hicolor icons
categories = ["Graphics", "Viewer"] # optional, desktop entry categories
mime_types = ["image/png", "image/x-example"] # optional, opened files and thumbnailed types
mime_info = "meta/freedesktop/mime.xml" # optional, shared-mime-info definitions of new types
thumbnailer = "--thumbnail -s %s %u %o" # optional, thumbnailer arguments of the executable
desktop_entry = "meta/freedesktop/.desktop" # optional, merged into the desktop entry
```

Then run `cargo bundle` with the path argument, when using a Cargo workspace run the command in the root of the workspace:

```sh
cargo bundle --path path/to/your/crate
```

### Application assets

Use [`cargo-bundle-assets`](../../lib/build/cargo-bundle-assets) to declare files
with `#[derive(cargo_bundle_assets::Assets)]` and `#[folder = "assets"]`.
Ordinary Cargo builds embed the bytes. Bundle builds copy declared files into
macOS `Contents/Resources/<hash>/`, iOS `assets/<hash>/`, Windows
MSIX asset packages containing `Assets/<hash>/`, Linux `/usr/share/<crate>/<hash>/`, or shared
Android APK `assets/<hash>/` entries.
Each collection has its own namespace, shared by all executable architectures.
APK assets are stored without ZIP compression and read through the native asset
manager. No runtime extraction, asset catalogs, build script, or pack format is
required. Existing `rust-embed` and `include_bytes!` users retain their behavior.

### macOS

This builds your crate for x86_64 and arm64, links both architectures into a universal binary,
creates an `.icns` file from the optional iconset, and creates a macOS app bundle in the
`target/bundle` directory. Add `--run` to launch the app afterwards with its console output
attached, which only builds the host architecture unless `--zip` or `--dmg` is also given:

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
still uses release mode. Cargo's reported executable path is used, including when
`CARGO_TARGET_DIR` is configured.
The application and its dependencies must support iOS; the runner does not translate desktop APIs.

The generated iOS Info.plist supports iPhone and iPad, portrait and landscape on iPhone, and all
orientations on iPad, and allows frame rates above 60 Hz on ProMotion iPhones. Override these keys
in the iOS Info.plist, for example to only allow portrait on iPhone:

```xml
<key>UISupportedInterfaceOrientations</key>
<array>
    <string>UIInterfaceOrientationPortrait</string>
</array>
```

### Windows

This builds your crate for x64 and arm64, packs an unsigned `.msix` package for each architecture
and combines them into a `.msixbundle` in the `target/bundle` directory with `MakeAppx` from the
Windows SDK, so Windows installs the package for its own architecture.
When the executables declare `cargo-bundle-assets` collections, the tool automatically
adds one architecture-neutral `<name>_assets.msix` containing the combined collection
files at `Assets/<hash>/`. Windows package folding makes those paths available beside
the installed executable. Conflicting payloads across architectures fail bundling.
Logos and `resources.pri` stay in each architecture package; the shared asset package
contains neither executable code nor a resource index. Apps without declared assets
keep their existing architecture-only bundle.

A `shell_extension` is built for the same architectures and its DLL is packaged beside the
executable. The manifest registers its classes as a COM surrogate server and adds the
thumbnail and preview handlers to every file type, so File Explorer shows thumbnails and
previews without changing the registry.

Shared asset packages require an NTFS installation volume. Publishing them through
the Microsoft Store requires approval from Windows developer support; see
[Microsoft's asset package documentation](https://learn.microsoft.com/en-us/windows/msix/package/asset-packages).

The package publisher is
the first package author with the
[unsigned package](https://learn.microsoft.com/en-us/windows/msix/package/unsigned-package) OID,
so it can be installed on Windows 11 from an administrator PowerShell:

```powershell
Add-AppxPackage -Path ExampleApp.msixbundle -AllowUnsigned -ForceUpdateFromAnyVersion
```

### Linux

On Linux, `cargo bundle` builds your crate for the host architecture and writes both a Debian
`.deb` and an RPM `.rpm` package to the `target/bundle/<crate>` directory, add `--deb` or `--rpm`
to only build one. Both packages are written in Rust, so building them needs neither `dpkg-deb`
nor `rpmbuild`, and build an RPM on Debian or a `.deb` on Fedora. They install:

- The executable as `/usr/bin/<crate>` and the declared assets in `/usr/share/<crate>`
- A desktop entry `/usr/share/applications/<identifier>.desktop` with the bundle name, the
  package description, the categories, and the MIME types, the executable receives the opened
  files as arguments. The optional `desktop_entry` file overrides its keys and adds groups like
  desktop actions
- The icons as `/usr/share/icons/hicolor/<size>/apps/<identifier>.png`
- AppStream metainfo `/usr/share/metainfo/<identifier>.metainfo.xml`, so software centers show
  the app with its summary, license, developer, and homepage
- The optional shared-mime-info file and a thumbnailer that runs the executable with the
  `thumbnailer` arguments for the MIME types

The GTK application ID of bwindow apps is the identifier, so the desktop shell matches windows
with their desktop entry. The packages depend on the shared libraries that the executable links:
the RPM requires their sonames like `libgtk-3.so.0()(64bit)`, the `.deb` depends on the Debian
packages that contain them, found with `ldconfig` and `dpkg-query` when building on Debian.
Desktop, MIME and icon caches are updated by the triggers of the package managers. Install the
packages with their dependencies:

```sh
sudo apt install ./target/bundle/example/example_0.1.0_amd64.deb
sudo dnf install ./target/bundle/example/example-0.1.0-1.x86_64.rpm
```

Set `SOURCE_DATE_EPOCH` for reproducible packages, otherwise files get the build time.

### Android

Build a signed APK for armeabi-v7a, arm64-v8a and x86_64 with `--android`, add `--run` to install and
start it on the connected device or emulator and follow its logcat output. Running only builds the
first ABI of the device that the bundle supports, read with `adb shell getprop ro.product.cpu.abilist`:

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
