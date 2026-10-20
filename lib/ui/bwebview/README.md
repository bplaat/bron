# Bassie Webview Rust library

A cross-platform webview library for Rust using WKWebView on macOS and iOS, WebView2 on
Windows, and WebKitGTK on Linux. Windows and the main event loop are provided by
[bwindow](../bwindow).

## Getting Started

Map browser notifications into typed application messages:

```rust,no_run
use bwindow::{Event, EventLoopBuilder, WindowBuilder, WindowId};
use bwebview::{WebviewBuilder, WebviewEvent};

enum AppEvent {
    Browser(WindowId, WebviewEvent),
}

let event_loop = EventLoopBuilder::new().with_user_event::<AppEvent>().build();
let mut window = WindowBuilder::new().title("Browser").build();
let _webview = WebviewBuilder::new(&window)
    .on_event(event_loop.create_proxy(), AppEvent::Browser)
    .load_url("https://example.com")
    .build();

event_loop.run(move |event| {
    if let Event::UserEvent(AppEvent::Browser(id, WebviewEvent::PageTitleChange(title))) = event {
        if id == window.id() {
            window.set_title(title);
        }
    }
});
```

## Linux

The Linux backend requires the GTK 3 and WebKitGTK runtime libraries. Development
headers are not required.

Debian / Ubuntu:

```sh
sudo apt install libgtk-3-0 libwebkit2gtk-4.1-0
```

On distributions using 64-bit `time_t`, install `libgtk-3-0t64` instead.

Fedora:

```sh
sudo dnf install gtk3 webkit2gtk4.1
```

## Screenshots

<table>
<tr>
<td align="center">
<img src="docs/images/screenshots/windows.png" alt="ipc example running on Windows" width="300">
<br>
<a href="./examples/bwebview-ipc.rs">IPC example</a> running on Windows
</td>
<td align="center">
<img src="docs/images/screenshots/macos.png" alt="ipc example running on macOS" width="300">
<br>
<a href="./examples/bwebview-ipc.rs">IPC example</a> running on macOS
</td>
<td align="center">
<img src="docs/images/screenshots/gtk.png" alt="ipc example running on Linux (GTK)" width="300">
<br>
<a href="./examples/bwebview-ipc.rs">IPC example</a> running on Linux (GTK)
</td>
</tr>
</table>

## Features

- `log` (default): forward `console.*` calls to the `log` crate.
- `custom_protocol`: serve content from custom URL schemes.
- `file_drop`: report files dropped on browser content as
  `WindowEvent::DroppedFile`.
- `rust-embed`: serve embedded assets using the `rust-embed` crate.

## Examples

See the [IPC example](examples/bwebview-ipc.rs) for JavaScript messaging and the
[lifecycle example](examples/bwebview-lifecycle.rs) for window teardown.

## License

Copyright © 2025-2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../../LICENSE) license.
