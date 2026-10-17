# Bassie's Bron des Codes

My personal monorepo for most of my hobby projects

## Projects

### Apps / Games

Some desktop apps and games written with the [bwindow](lib/ui/bwindow), [bwebview](lib/ui/bwebview), and [bcanvas](lib/graphics/bcanvas) libraries.

<table>
<tr>
<td width="100" align="center">
    <a href="./games/game2048">
        <img src="games/game2048/meta/freedesktop/icons/128x128.png" alt="2048 icon" width="48" height="48"/><br/>
        2048
    </a>
</td>
<td width="100" align="center">
    <a href="./apps/desktop/bassielight">
        <img src="apps/desktop/bassielight/meta/freedesktop/icons/128x128.png" alt="BassieLight icon" width="48" height="48"/><br/>
        BassieLight
    </a>
</td>
<td width="100" align="center">
    <a href="./apps/windows/binman">
        <img src="apps/windows/binman/docs/images/icon.png" alt="Binman icon" width="48" height="48"/><br/>
        Binman
    </a>
</td>
<td width="100" align="center">
    <a href="./apps/macos/macview">
        <img src="apps/macos/macview/docs/images/icon.png" alt="MacView icon" width="48" height="48"/><br/>
        MacView
    </a>
</td>
<td width="100" align="center">
    <a href="./apps/desktop/manexplorer">
        <img src="apps/desktop/manexplorer/meta/freedesktop/icons/128x128.png" alt="ManExplorer icon" width="48" height="48"/><br/>
        ManExplorer
    </a>
</td>
</tr>
<tr>
<td width="100" align="center">
    <a href="./apps/desktop/music-dl">
        <img src="apps/desktop/music-dl/meta/freedesktop/icons/128x128.png" alt="Music Downloader icon" width="48" height="48"/><br/>
        Music Downloader
    </a>
</td>
<td width="100" align="center">
    <a href="./apps/desktop/navidrome">
        <img src="apps/desktop/navidrome/meta/freedesktop/icons/128x128.png" alt="Navidrome icon" width="48" height="48"/><br/>
        Navidrome
    </a>
</td>
<td width="100" align="center">
    <a href="./apps/desktop/pixelfont">
        <img src="apps/desktop/pixelfont/meta/freedesktop/icons/128x128.png" alt="Pixel Font Editor icon" width="48" height="48"/><br/>
        Pixel Font Editor
    </a>
</td>
<td width="100" align="center">
    <a href="./games/reversi">
        <img src="games/reversi/meta/freedesktop/icons/128x128.png" alt="Reversi icon" width="48" height="48"/><br/>
        Reversi
    </a>
</td>
<td width="100" align="center">
    <a href="./apps/desktop/sequelexplorer">
        <img src="apps/desktop/sequelexplorer/meta/freedesktop/icons/128x128.png" alt="Sequel Explorer icon" width="48" height="48"/><br/>
        Sequel Explorer
    </a>
</td>
</tr>
</table>

- [2048](games/game2048) An offline desktop 2048 game app
- [BassieLight](apps/desktop/bassielight) A simple lights controller with GUI
- [Binman](apps/windows/binman) A safe and minimal Windows cleanup tool
- [Empires](games/empires) An Age of Empires rendering demo and DRS archive viewer using SDL2
- [MacView](apps/macos/macview) A macOS bitmap, QOI, SVG, and TinyVG image viewer with Quick Look [preview](apps/macos/macview-preview) and [thumbnail](apps/macos/macview-thumbnail)
- [ManExplorer](apps/desktop/manexplorer) A simple man page explorer tool
- [Nintendo DS Ports](games/nds) DOOM, Duke Nukem 3D, Quake, Quake II, and Wolfenstein 3D ports
- [Navidrome](apps/desktop/navidrome) A [music.bplaat.nl](https://music.bplaat.nl/) webview wrapper
- [Music Downloader](apps/desktop/music-dl) A tool that downloads complete albums with the correct metadata
- [Pixel Font Editor](apps/desktop/pixelfont) An 8x8 pixel font editor
- [Reversi](games/reversi) A native canvas board game with a background AI
- [Sequel Explorer](apps/desktop/sequelexplorer) A read-only SQLite and MySQL database GUI viewer

### Tools / Websites

Various tools and websites. For the deployed sites, go to [bron.bplaat.nl](https://bron.bplaat.nl/).

- [baksteen](apps/web/baksteen/) A brick-laying robot simulator
- [bob](tools/bob) A simple build system for my projects, because I like the simplicity of Cargo
- [ccontinue](tools/ccontinue) A transpiler that translates an OOP extension for the C programming language back to C
- [cargo-bundle](tools/cargo-bundle) A simple Cargo plugin that builds macOS app bundles
- [plaatnotes](apps/web/plaatnotes) A self-hosted note-taking web app with rich Markdown support
- [nds-toolchain](tools/nds-toolchain/) An LLVM-based Nintendo DS toolchain with a small C library and a test ROM
- [wii-toolchain](tools/wii-toolchain/) An LLVM-based Wii toolchain with a small C library, patched libogc, and Dolphin emulator tests

### Examples

- [macOS Examples](examples/macos/) Mach-O assembly, Cocoa, Metal, OpenGL, and iOS examples
- [Wii Examples](examples/wii/) Wii homebrew examples in C and Rust: triangles, a 2D canvas, and voxel blocks
- [Windows Examples](examples/windows/) 32-bit and 64-bit Windows assembly examples using NASM

### Libraries

Most of these libraries are intended primarily for personal use, but can still be useful. Many of them are minimal replacements for commonly used crates.

**User Interfaces**

- [bwebview](lib/ui/bwebview) A cross-platform webview library for Rust with minimal dependencies
- [bwindow](lib/ui/bwindow) Native windows and one typed main event loop
- [plaatui](lib/ui/plaatui) A Preact web components ui library for my own services

**Graphics**

- [bcanvas](lib/graphics/bcanvas) Native 2D canvas content, without a browser dependency
- [gxtexconv](lib/graphics/gxtexconv) A GameCube / Wii TPL texture converter library and CLI, replacing devkitPro's gxtexconv
- [image](lib/graphics/image) A minimal JPEG, PNG/APNG, GIF, BMP, and QOI image decoder crate
- [vector](lib/graphics/vector) SVG (subset) and TinyVG decoders with a backend-neutral display list
- [wgpu](lib/graphics/wgpu) A native GPU rendering library for bwindow
- [wgpu-shader-build](lib/graphics/wgpu-shader-build) Build-time native WGSL compilation for wgpu

**Apple**

- [block2](lib/apple/block2) A minimal replacement for the [block2](https://crates.io/crates/block2) crate
- [objc2](lib/apple/objc2) A minimal replacement for the [objc2](https://crates.io/crates/objc2) crate
- [objc2-proc-macros](lib/apple/objc2-proc-macros) A minimal replacement for the [objc2-proc-macros](https://crates.io/crates/objc2-proc-macros) crate

**Networking**

- [local-ip-address](lib/net/local-ip-address) A minimal replacement for the [local-ip-address](https://crates.io/crates/local-ip-address) crate
- [maxminddb](lib/net/maxminddb) A minimal replacement for the [maxminddb](https://crates.io/crates/maxminddb) crate
- [mime](lib/net/mime) A minimal replacement for the [mime](https://crates.io/crates/mime) crate
- [mime_guess](lib/net/mime_guess) A minimal replacement for the [mime_guess](https://crates.io/crates/mime_guess) crate
- [native-tls](lib/net/native-tls) A minimal replacement for the [native-tls](https://crates.io/crates/native-tls) crate
- [simple-useragent](lib/net/simple-useragent) A simple user agent parser library based on the uap-core regexes **([crates.io](https://crates.io/crates/simple-useragent))**
- [small-http](lib/net/small-http) A simple and small HTTP/1.1 server/client library
- [small-router](lib/net/small-router) A simple and small router for the [small-http](lib/net/small-http) library
- [small-websocket](lib/net/small-websocket) A simple and small WebSocket library for the [small-http](lib/net/small-http) library
- [url](lib/net/url) A minimal replacement for the [url](https://crates.io/crates/url) crate

**Data**

- [base64](lib/data/base64) A minimal replacement for the [base64](https://crates.io/crates/base64) crate
- [bsql](lib/data/bsql) A simple and minimal Rust SQLite and MySQL library with an ergonomic API
- [bsql_derive](lib/data/bsql_derive) The derive macros for the [bsql](lib/data/bsql) crate
- [chrono](lib/data/chrono) A minimal replacement for the [chrono](https://crates.io/crates/chrono) crate
- [from_derive](lib/data/from_derive) A FromEnum and FromStruct derive macro library
- [gzip](lib/data/gzip) A gzip library and command-line utility with a benchmark
- [libsqlite3-sys](lib/data/libsqlite3-sys) A minimal replacement for the [libsqlite3-sys](https://crates.io/crates/libsqlite3-sys) crate
- [plist](lib/data/plist) A minimal replacement for the [plist](https://crates.io/crates/plist) crate
- [regex](lib/data/regex) A minimal replacement for the [regex](https://crates.io/crates/regex) crate
- [serde_yaml](lib/data/serde_yaml) A minimal replacement for the [serde_yaml](https://crates.io/crates/serde_yaml) crate
- [uuid](lib/data/uuid) A minimal replacement for the [uuid](https://crates.io/crates/uuid) crate
- [validate](lib/data/validate) A simple struct validation library
- [validate_derive](lib/data/validate_derive) Validation derive macros library
- [zip](lib/data/zip) A minimal replacement for the [zip](https://crates.io/crates/zip) crate

**Crypto**

- [digest](lib/crypto/digest) A minimal replacement for the [digest](https://crates.io/crates/digest) crate
- [getrandom](lib/crypto/getrandom) A minimal replacement for the [getrandom](https://crates.io/crates/getrandom) crate
- [hmac](lib/crypto/hmac) A minimal replacement for the [hmac](https://crates.io/crates/hmac) crate
- [pbkdf2](lib/crypto/pbkdf2) A PBKDF2-HMAC-SHA256 password hashing library
- [sha1](lib/crypto/sha1) A minimal replacement for the [sha1](https://crates.io/crates/sha1) crate
- [sha2](lib/crypto/sha2) A minimal replacement for the [sha2](https://crates.io/crates/sha2) crate

**System**

- [copy_dir](lib/sys/copy_dir) A minimal replacement for the [copy_dir](https://crates.io/crates/copy_dir) crate
- [directories](lib/sys/directories) A minimal replacement for the [directories](https://crates.io/crates/directories) crate
- [enable-ansi-support](lib/sys/enable-ansi-support) A minimal replacement for the [enable-ansi-support](https://crates.io/crates/enable-ansi-support) crate
- [keyring](lib/sys/keyring) A minimal replacement for the [keyring](https://crates.io/crates/keyring) crate
- [rusb](lib/sys/rusb) A minimal replacement for the [rusb](https://crates.io/crates/rusb) crate
- [simple_logger](lib/sys/simple_logger) A minimal replacement for the [simple_logger](https://crates.io/crates/simple_logger) crate
- [terminal_size](lib/sys/terminal_size) A minimal replacement for the [terminal_size](https://crates.io/crates/terminal_size) crate
- [threadpool](lib/sys/threadpool) A fixed and scalable thread pool for Rust

**Languages**

- [js](lib/lang/js) A WIP JavaScript interpreter

**Build**

- [openapi-generator](lib/build/openapi-generator) A simple OpenAPI code generator
- [rust-embed](lib/build/rust-embed) A minimal replacement for the [rust-embed](https://crates.io/crates/rust-embed) crate
- [rust-embed-impl](lib/build/rust-embed-impl) A minimal replacement for the [rust-embed-impl](https://crates.io/crates/rust-embed-impl) crate
- [winresource](lib/build/winresource) A minimal replacement for the [winresource](https://crates.io/crates/winresource) crate

**Testing**

- [criterion](lib/test/criterion) A minimal replacement for the [criterion](https://crates.io/crates/criterion) crate
- [trybuild](lib/test/trybuild) A minimal replacement for the [trybuild](https://crates.io/crates/trybuild) crate

## Getting Started

- Install [Rust](https://rustup.rs/), [Node.js](https://nodejs.org/), [Hadolint](https://github.com/hadolint/hadolint), a C toolchain (such as GCC, Clang or MSVC), an LLVM toolchain, clang-format, [NASM](https://nasm.us/) and [Dolphin emulator](https://dolphin-emu.org/)
- Install Rust nightly toolchain, `cargo-binstall`, `cargo-deny` and `cargo-nextest`:

    ```sh
    rustup toolchain add nightly --component rust-src --component rustfmt
    cargo install cargo-binstall
    cargo binstall -y --locked cargo-deny cargo-nextest
    ```

- Run checks, or run an example:

    ```sh
    cargo xtask check
    cargo run --bin example-persons-api
    cargo run --bin example-todo-app
    ```

### Additional tools

- Run the local [criterion](lib/test/criterion) benchmarks with:

    ```sh
    cargo bench -p base64 --bench base64
    cargo bench -p image --bench image
    cargo bench -p sha2 --bench sha2
    ```

- For coverage reports, install the `llvm-tools` and `cargo-llvm-cov` tool:

    ```sh
    rustup component add llvm-tools
    cargo binstall -y --locked cargo-llvm-cov
    cargo xtask coverage
    ```

- To build pages, install a wasm target and the `wasm-bindgen-cli` tool:

    ```sh
    rustup target add wasm32-unknown-unknown
    cargo binstall -y --locked wasm-bindgen-cli --version 0.2.127
    cargo xtask build-pages
    ```

- To build macOS app bundles, only on macOS, install targets:

    ```sh
    rustup target add aarch64-apple-darwin x86_64-apple-darwin
    cargo xtask build-bundle
    ```

- To build and install bins and GUI applications to your system:

    ```sh
    cargo xtask install
    ```

## Goals

This project is driven by three main goals:

**1. A self-written minimal crates collection for backend software**

Rather than pulling in large, general-purpose dependencies, this repo grows a curated set of small, focused libraries that cover exactly what is needed - nothing more. This includes an HTTP/1.1 server, a router, a SQLite and MySQL library, validation, password hashing, UUID generation, and more. Many of these are intentional minimal replacements for well-known crates on [crates.io](https://crates.io), rewritten from scratch to be lean, auditable, and free of transitive dependencies.

**2. A minimal Electron/Tauri-like desktop platform**

[bwebview](lib/ui/bwebview) is a cross-platform native webview library that lets you ship desktop applications using web technologies, without the weight of bundling a full browser engine. It uses the system-native webview on each platform (WebKit on macOS/iOS, WebKitGTK on Linux, WebView2 on Windows) and exposes a small, clean Rust API. The [desktop apps](apps/desktop) and [games](games/) in this repo, like a SQLite and MySQL viewer, a pixel font editor, a man page browser and a 2048 game, show that this works in practice.

**3. Tools and a custom build system for complex multi-language projects**

Building projects that span multiple languages and artifact types (Rust, JavaScript, WASM, native bundles) requires coordination that general-purpose build systems handle poorly. [bob](tools/bob) is a minimal build system inspired by `Cargo`'s simplicity, designed to describe and orchestrate such projects cleanly. [cargo-bundle](tools/cargo-bundle) handles macOS `.app` bundle packaging from within the `Cargo` ecosystem. Together with the [xtask](tools/xtask) crate these tools keep the build pipeline typed, cross-platform and easy to follow.

### Repo organization

This repo is a monorepo. Projects are grouped by what they are, not by the language they are written in:

- [apps](apps/) - applications, grouped by platform: `desktop` for cross-platform apps, plus `macos`, `windows` and `web`
- [examples](examples/) - standalone examples, grouped by platform
- [games](games/) - games and their engines
- [lib](lib/) - libraries, grouped by topic like `ui`, `graphics`, `net`, `data` and `crypto`
- [tools](tools/) - developer tools, including the build tooling of this repo

Library examples live next to the library they show off, in its `examples` directory. Standalone platform examples live under the top-level `examples` directory.

The main Rust projects share one `Cargo` workspace and the JavaScript projects share one `npm` workspace. The macOS and iOS Rust examples are standalone projects using the local Apple bindings. The Wii Rust examples are standalone nightly projects with a custom PowerPC target. When a Rust app has a web frontend, its `Cargo` build script runs `npm` for you, so `Cargo` stays the one place where everything comes together.

Every top-level task, like checking, building and installing, goes through the [xtask](tools/xtask) crate with `cargo xtask`, both on your machine and in CI.

## License

Copyright © 2019-2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](LICENSE) license.
