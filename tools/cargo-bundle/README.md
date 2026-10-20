# cargo-bundle

A simple Cargo plugin that builds macOS app bundles and Windows MSIX packages.

## Usage

Add app bundle metadata to your `Cargo.toml`:

```toml
[package.metadata.bundle]
name = "ExampleApp"
identifier = "com.example.App"
copyright = "Copyright © 2025 Anonymous"
iconset = "path/to/icon.iconset" # optional

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
