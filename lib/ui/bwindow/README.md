# Bassie Window Rust library

Cross-platform native windows and a main event loop for Rust, using AppKit on macOS,
UIKit on iOS, Win32 on Windows, and GTK 3 on Linux. Add [bcanvas](../../graphics/bcanvas) for
native 2D drawing or [bwebview](../bwebview) for web content.

## Getting Started

```rust,no_run
use bwindow::{EventLoopBuilder, WindowBuilder};

let event_loop = EventLoopBuilder::new().build();
let window = WindowBuilder::new().title("Window").build();

event_loop.run(move |_event| {
    // Keep the window alive until the event loop exits.
    let _ = &window;
});
```

Use `EventLoopBuilder::new().with_user_event::<AppEvent>()` for typed worker and
content events. Every window event includes a `WindowId`. Closing the last window
or calling a proxy's `exit()` stops the event loop.

## Input Events

Native input uses DOM-like names with typed Rust payloads:

| Window event               | DOM equivalent             |
| -------------------------- | -------------------------- |
| `KeyDown` / `KeyUp`        | `keydown` / `keyup`        |
| `MouseDown` / `MouseUp`    | `mousedown` / `mouseup`    |
| `MouseMove` / `MouseLeave` | `mousemove` / `mouseleave` |
| `Wheel`                    | `wheel`                    |
| `Focus` / `Blur`           | `focus` / `blur`           |

`code` is the physical key (`KeyCode::KeyA` is DOM `code = "KeyA"`), `key` is
layout-dependent and `repeat` marks auto-repeat presses.

```rust,no_run
use bwindow::{Event, EventLoopBuilder, Key, WindowEvent};

let event_loop = EventLoopBuilder::new().build();
event_loop.run(|event| {
    if let Event::Window(_, WindowEvent::KeyDown(key)) = event {
        if !key.repeat && matches!(&key.key, Key::Character(text) if text == "r") {
            println!("R pressed");
        }
        if key.modifiers.meta_key() {
            println!("Meta held");
        }
    }
});
```

- `Modifiers::META` is Command on macOS and Windows/Super elsewhere.
- Mouse positions are logical client-area pixels, like `clientX`/`clientY`.
- Wheel deltas are in pixels or lines; positive values scroll right/down.
- Only close requests can be cancelled, with `prevent_default()`.
- IME text input is not supported.

## Redrawing

Use `request_redraw()` after state changes. For animation, call
`request_animation_frame()` and request the next frame from its
`RedrawRequested` callback.

## Window Content

- Empty window: use `WindowBuilder` alone.
- Native 2D drawing: attach `bcanvas::CanvasBuilder` and paint during that window's
  `RedrawRequested` callback.
- Web content: attach `bwebview::WebviewBuilder` and map browser notifications into
  your application's typed events with `on_event(proxy, mapper)`.

## Features

- `remember_window_state` (default): persist native window placement.
- `dialog`: native message and file dialogs.
- `file_drop`: native file-drop events.
- `menu`: custom macOS menus.
- `progress_bar`: Windows taskbar and GTK launcher progress.

## iOS

Windows always fill the screen, so size, position and resizing options are ignored. Menus and
file drops are ignored, the `dialog` feature is not supported, and touches are delivered as left
mouse button events.

## Linux

The Linux backend requires the GTK 3.18 or newer runtime library. Development
headers are not required.

Debian / Ubuntu:

```sh
sudo apt install libgtk-3-0
```

On distributions using 64-bit `time_t`, install `libgtk-3-0t64` instead.

Fedora:

```sh
sudo dnf install gtk3
```

## License

Copyright © 2025-2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../../LICENSE) license.
