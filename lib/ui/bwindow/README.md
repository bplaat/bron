# Bassie Window Rust library

Cross-platform native windows and a main event loop for Rust, using AppKit on macOS,
UIKit on iOS, Win32 on Windows, GTK 3 on Linux, and an Activity on Android. Add
[bcanvas](../../graphics/bcanvas) for native 2D drawing, [bwebgpu](../../graphics/bwebgpu)
for native 3D drawing or [bwebview](../bwebview) for web content.

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

## Single Instance

Set `.app_id("nl", "example", "MyApp")` on the event loop builder to identify the
application. Single-instance mode is enabled by default; `.single_instance(false)`
allows independent processes on Windows and GTK.

On Windows, one process owns a named mutex. Subsequent launches request that the running
instance restore and focus an existing window, and wait up to five seconds for the
request to be acknowledged. A background listener accepts requests even before the first
window or event loop is ready. The UI thread performs the restore/focus operation once
it starts processing messages. No application-level activation event is emitted, and
arguments from subsequent launches are not forwarded.

The Windows implementation follows the Windows App SDK's
[AppInstance registration and redirection model](https://github.com/microsoft/WindowsAppSDK/blob/main/dev/AppLifecycle/AppInstance.cpp)
using Win32 APIs, without an SDK runtime dependency. It recovers abandoned ownership,
retains handles until shutdown, and exits a launcher with an error if registration or
redirection fails. Installed MSIX packages include package identity in the instance key,
separating them from unpackaged builds; AppContainer object namespaces remain isolated.
Foreground activation is subject to Windows focus rules.

GTK uses `GtkApplication` and the session bus to activate the existing instance and
forward files as `Event::OpenFiles`. macOS, iOS and Android rely on their native launch
behavior rather than enforcing this option inside bwindow.

## Input Events

Native input uses DOM-like names with typed Rust payloads:

| Window event               | DOM equivalent             |
| -------------------------- | -------------------------- |
| `KeyDown` / `KeyUp`        | `keydown` / `keyup`        |
| `MouseDown` / `MouseUp`    | `mousedown` / `mouseup`    |
| `MouseMove` / `MouseLeave` | `mousemove` / `mouseleave` |
| `PointerLockChange`        | `pointerlockchange`        |
| `TouchStart` / `TouchMove` | `touchstart` / `touchmove` |
| `TouchEnd` / `TouchCancel` | `touchend` / `touchcancel` |
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
- `MouseMove::movement` is the relative motion, like `movementX`/`movementY`.
- `window.request_pointer_lock()` hides and locks the pointer, like `requestPointerLock()`,
  it is released with `window.exit_pointer_lock()` and `window.pointer_locked()` queries
  it. Lock changes send `PointerLockChange`. Touch screens have no pointer to lock.
- Wheel deltas are in pixels or lines; positive values scroll right/down.
- iOS and Android report every changed finger with a stable `Touch::identifier`.
  The first finger also emulates a mouse. Call `TouchEvent::prevent_default()` on
  `TouchStart` to suppress mouse input for that gesture.
- Close requests can be cancelled with `prevent_default()`.
- IME text input is not supported.
- Input reaches the window the same way for empty, canvas and GPU windows. A web view
  handles its own input; keys and pointer events it handles are not reported again.
- `Focus` / `Blur` follow window activation.

Set the cursor over the window content with `window.set_cursor(CursorIcon::Pointer)`.
`Progress` shows the arrow on macOS, which has no public busy cursor. Cursors are ignored
on touch screens and over a web view, which shows its own cursors.

## Redrawing

Use `request_redraw()` after state changes. For animation, call
`request_animation_frame()` and request the next frame from its
`RedrawRequested` callback. Frames follow the refresh rate of the display the window
is on, and repeated requests coalesce into one frame.

## Window Content

- Empty window: use `WindowBuilder` alone.
- Native 2D drawing: attach `bcanvas::CanvasBuilder` and paint during that window's
  `RedrawRequested` callback.
- Web content: attach `bwebview::WebviewBuilder` and map browser notifications into
  your application's typed events with `on_event(proxy, mapper)`.

A window has one content attachment, taken with `window.attach_content()`. The window
keeps handling input and pacing frames, the attached content only draws. Content draws
into `WindowAttachment::content_handle()`: the client area of the window on Windows, a
layer-backed view on macOS and iOS, a drawing area on GTK and the window surface on
Android.

## Safe Area

Content fills the whole window, also under the status bar, display cutouts and the home
indicator on iOS and Android. `window.safe_area()` returns the `LogicalInsets` from each
window edge to the area that is not covered by system UI, keep text and controls inside it
and let backgrounds fill the window. Changes, like rotating the device, send
`WindowEvent::SafeAreaChanged`. Desktop windows have no insets.

## Features

- `remember_window_state` (default): persist native window placement.
- `dialog`: native message and file dialogs.
- `file_drop`: native file-drop events.
- `menu`: custom application menus and their keyboard shortcuts.
- `progress_bar`: Windows taskbar and GTK launcher progress.

## iOS

Windows always fill the screen, so size, position and resizing options are ignored. The safe
area is the view's `safeAreaInsets`. Menus and file drops are ignored, the `dialog` feature is
not supported, and touches are delivered as left mouse button events.

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

## Android

Build an APK with [cargo-bundle](../../../tools/cargo-bundle) `--android`. The app runs
in the framework `android.app.NativeActivity`, so the APK needs no Java code. Android 8.0
(API 26) is the minimum.

- One window per app. It fills the activity, also under the system bars, `Resize` reports
  its size in dp. The safe area is the system bar and display cutout insets.
- Title, position, size, minimum size, resizable, center, and window state calls
  are no-ops. `background_color` fills the window when no renderer takes the content
  host, also behind content in its own window like a web view, and picks matching
  light or dark system bar icons. `android_system_bar_icons` overrides that choice
  for content such as games. `set_theme` forces the app night mode on
  Android 12+.
- The system night mode is the theme, changes send `ThemeChanged`.
- Touches report `MouseMove` and a left `MouseDown`, lifting the finger reports
  `MouseUp` and `MouseLeave`. Hardware keys report `KeyDown` / `KeyUp`.
- The back gesture sends `CloseRequested`; when not prevented the app moves to the
  background like a native root activity. Closing the window ends the event loop, then
  the activity finishes and the process exits, a relaunch while finishing restarts the
  app in a fresh process.
- stdout, stderr, and panics are written to logcat with the binary name as tag.
- The `dialog`, `file_drop`, `menu`, and `progress_bar` features are unsupported.

## License

Copyright © 2025-2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../../LICENSE) license.
