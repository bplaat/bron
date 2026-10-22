# Bassie Canvas Rust library

Cross-platform 2D drawing for [bwindow](../../ui/bwindow) windows using the system-native
graphics API: Core Graphics on macOS and iOS, Direct2D on Windows, Cairo on Linux, and
`android.graphics.Canvas` on Android.

## Features

- Paths, clipping, transforms, solid RGBA colors, and text
- Logical pixel coordinates
- One event loop shared with window and webview content

## Getting Started

```rust,no_run
use bcanvas::{CanvasBuilder, Color};
use bwindow::{Event, EventLoopBuilder, WindowBuilder, WindowEvent};

let event_loop = EventLoopBuilder::new().build();
let window = WindowBuilder::new().title("Canvas").build();
let mut canvas = CanvasBuilder::new(&window).build();

event_loop.run(move |event| {
    if let Event::Window(id, WindowEvent::RedrawRequested) = event {
        if id == window.id() {
            canvas.draw(|ctx| {
                ctx.set_fill_style(Color::rgb(30, 120, 200));
                ctx.fill_rect(0.0, 0.0, ctx.width(), ctx.height());
            });
        }
    }
});
```

Drawing state resets each frame. Drawing after the window closes returns `false`.
Images and IME text editing are not supported.

## Canvas 2D API

The context uses [Canvas 2D](https://html.spec.whatwg.org/multipage/canvas.html)
method names in snake_case, with getter/setter pairs such as `fill_style()` /
`set_fill_style(color)`. It is a native subset:

- Colors and style choices use `Color` and enums instead of CSS strings.
- `set_font(family, size)` and `set_font_weight(weight)` use typed font settings;
  size is in logical pixels.
- `measure_text(text)` returns width, not a browser `TextMetrics` object.
- `ellipse(x, y, rx, ry)` adds a full axis-aligned ellipse, and `round_rect` accepts
  one corner radius.
- Text is left-to-right only; baselines approximate browser typography.
- Invalid alpha and line-width values are ignored.
- `save()` / `restore()` do not include the current path.

## Interaction

The window delivers keyboard, pointer, scroll, focus and file drop events of the canvas
as `WindowEvent`s, the canvas only draws. Set the cursor over it with
`window.set_cursor(CursorIcon::Pointer)`, see the bwindow README.

## Android

Frames draw directly into the window surface from the Rust main thread with a hardware
accelerated `android.graphics.Canvas`, through JNI calls with cached method IDs and reused
paints, and are scheduled with the display choreographer. Frames cover the whole window, also
under the system bars, see the bwindow safe area. `clear_rect` reveals the window background
color. `OffscreenCanvas` draws into an Android bitmap.

## Offscreen drawing

`OffscreenCanvas` draws without a window into straight-alpha RGBA8 pixels, for
example to upload with `bwebgpu::Queue::write_texture` or encode with the
[image](../image) crate:

```rust
use bcanvas::{Color, OffscreenCanvas};

let mut canvas = OffscreenCanvas::new(128, 32);
canvas.draw(|ctx| {
    ctx.set_fill_style(Color::rgb(255, 255, 255));
    ctx.fill_text("60 FPS", 8.0, 20.0);
});
let rgba = canvas.pixels();
assert_eq!(rgba.len(), 128 * 32 * 4);
let bitmap = canvas.to_bitmap().expect("create bitmap");
assert_eq!(bitmap.width(), 128);
```

Run `cargo run -p example-bcanvas-headless` to draw and save `frame.png`
without creating a window. The [example](examples/headless) enables
the image crate's PNG feature and calls `image::encode`.

Disable default features to drop the `window` feature and with it `Canvas`, bwindow
and the windowing frameworks. On macOS and iOS an offscreen-only build then links
just Core Graphics, Core Text and Core Foundation:

```toml
bcanvas = { path = "../bcanvas", default-features = false }
```

## License

Copyright © 2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../../LICENSE) license.
