# cargo-bundle-assets

Bundle-aware assets with the same `get()`, `iter()`, and `file.data` interface as
this workspace's `rust-embed`. No build script or build helper is required.

```rust,ignore
use cargo_bundle_assets::Assets;

#[derive(Assets)]
#[folder = "assets"]
struct Assets;

let file = Assets::get("tiles/grass_top.png").expect("Texture");
let bytes = &file.data;
```

Ordinary `cargo build`, `cargo run`, and `cargo test` embed the files, including
release builds. These executables can be moved and run without the source tree.
`cargo-bundle` sets `CARGO_BUNDLE`: these builds contain collection metadata
instead of file payloads. The tool copies each declared file into the app's
resources at `Contents/Resources/<hash>/` on macOS and `assets/<hash>/` on
iOS, a shared neutral MSIX asset package at `Assets/<hash>/`, shared APK
`assets/<hash>/` entries, or `/usr/share/<name>/<hash>/` beside `/usr/bin/<name>`
in Linux packages. Resources are read
directly without extraction. No asset catalogs or custom pack files are created.
Windows automatically folds the shared asset package into the selected architecture
package, so the runtime uses the same executable-relative asset paths.

The derive implements `RustEmbed`, so existing `bwebview` rust-embed loaders work.
`get()` returns `None` for unknown files or read errors; `try_get()` returns
`io::Result<Option<EmbeddedFile>>` when callers need the underlying error.
`iter()` returns sorted, slash-separated relative names. Only declared names can
be read. Folders are relative to the declaring Cargo package; `$ENV_VAR` folder
interpolation is supported, including `$OUT_DIR`.
Variable values are inserted literally; missing variables produce a compile error.

Existing file edits and changes to `CARGO_BUNDLE` are tracked by rustc. Stable
Rust does not yet expose directory dependency tracking to procedural macros:
packages with an existing build script can emit
`println!("cargo:rerun-if-changed=assets");` to watch the collection directory,
as Plaatcraft does. Otherwise, after adding a new asset, touch the source
containing the derive (or clean that package) before an ordinary Cargo build.
`cargo-bundle` refreshes collection membership before building. Files and folders reached through symlinks are
rejected to avoid including resources outside the declared collection.
