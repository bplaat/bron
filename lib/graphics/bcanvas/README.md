# Bassie Canvas Rust library

Cross-platform 2D drawing for [bwindow](../../ui/bwindow) windows using the system-native
graphics API: Core Graphics on macOS and iOS, Direct2D on Windows, Cairo on Linux, and
`android.graphics.Canvas` on Android.

## Features

- Paths, clipping with fill rules, transforms, dashed strokes, and text
- Solid colors and linear or radial gradients with pad, reflect, or repeat spread
- Layers with alpha, composite operations, and masks
- Images drawn from premultiplied BGRA pixels
- Logical pixel coordinates
- One event loop shared with window and webview content

## Getting Started

```rust,no_run
# #[cfg(feature = "window")]
# fn main() {
use bcanvas::{CanvasBuilder, Color};
use bwindow::{Event, EventLoopBuilder, WindowBuilder, WindowEvent};

let event_loop = EventLoopBuilder::new().build();
let window = WindowBuilder::new().title("Canvas").build();
let mut canvas = CanvasBuilder::new(&window).build();

event_loop.run(move |event| {
    if let Event::Window(id, WindowEvent::RedrawRequested) = event {
        if id == window.id() {
            if !canvas.draw(|ctx| {
                ctx.set_fill_style(Color::rgb(30, 120, 200));
                ctx.fill_rect(0.0, 0.0, ctx.width(), ctx.height());
            }) {
                return;
            }
        }
    }
});
# }
# #[cfg(not(feature = "window"))]
# fn main() {}
```

Drawing state resets each frame. `draw` returns whether its closure ran; handle that
result before advancing frame-dependent state. It returns `false` outside a native paint
callback, after close, or if the frame was already drawn. The library schedules native
frames and retries unavailable paint passes; applications do not need platform-specific retries.
For animation, call `canvas.request_animation_frame()` after drawing. Images and IME
text editing are not supported.

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
- Drawing and transform methods ignore non-finite numeric arguments. Negative finite
  radii in `arc`, `ellipse`, and `round_rect` panic instead of throwing JavaScript
  exceptions. `round_rect` preserves winding when width or height is negative.
- `save()` / `restore()` do not include the current path.
- `fill_with(rule)` and `clip_with(rule)` take the fill rule of `fill(rule)` and
  `clip(rule)`, gradients add `set_spread` and `set_transform` for SVG paints.
- `begin_layer()` / `end_layer()` follow the canvas layers proposal: a layer composites
  with the global alpha and composite operation of its start. `begin_mask_layer()` draws a
  mask until `end_mask()`, whose alpha masks the layer content.
- `draw_image(image, x, y, width, height)` draws a `CanvasImage` of premultiplied BGRA
  pixels, which it shares.
- Composite operations of drawing only change the area that is drawn, like Core Graphics,
  while a layer composites over the whole canvas when it ends.
- Every backend supports the same features. Direct2D blends layers through Direct2D 1.1
  effects. Android needs Android 10 for the blend modes and Android 12 for radial
  gradients with a focal point, before which the focal point is the center.

## Native views

`draw_native` draws into a Core Graphics or cairo context of a native view, like the
`drawRect:` of an NSView or the draw signal of a GTK widget. On Windows `HwndCanvas`
paints a child window in its `WM_PAINT`.

## Input

Keyboard, pointer and file drop events over the canvas arrive as `WindowEvent`s of its
window, see [bwindow](../../ui/bwindow).

## Android

The canvas covers the whole window, also under the system bars, so keep content inside
`window.safe_area()`. `clear_rect` reveals the window background color.

## Offscreen drawing

`OffscreenCanvas` draws without a window into straight-alpha RGBA8 pixels, for
example to upload with `bwebgpu::Queue::write_texture`:

```rust
use bcanvas::{Color, OffscreenCanvas};

let mut canvas = OffscreenCanvas::new(128, 32);
canvas.draw(|ctx| {
    ctx.set_fill_style(Color::rgb(255, 255, 255));
    ctx.fill_text("60 FPS", 8.0, 20.0);
});
let rgba = canvas.pixels();
assert_eq!(rgba.len(), 128 * 32 * 4);
```

With the `image` feature, `to_bitmap` copies the pixels into an [image](../image)
bitmap for encoding. Run `cargo run -p example-bcanvas-headless` to draw and save `frame.png`
without creating a window. The [example](examples/headless) enables the `image`
feature and the image crate's `png` feature, then calls `image::encode`.

Disable default features to drop the `window` feature and with it `Canvas`, bwindow
and the windowing frameworks. On macOS and iOS an offscreen-only build then links
just Core Graphics, Core Text and Core Foundation:

```toml
bcanvas = { path = "../bcanvas", default-features = false }
```

## License

Copyright © 2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../../LICENSE) license.
