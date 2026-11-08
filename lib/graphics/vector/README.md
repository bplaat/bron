# Vector

A safe Rust decoder for SVG/SVGZ (subset) and TinyVG. Documents decode to a recording
[bcanvas](../bcanvas) `OffscreenCanvas` of their intrinsic size, which `draw_canvas` draws
sharp at any scale.

## Usage

```rs
let bytes = std::fs::read("picture.svg").expect("read vector image");
let image = vector::decode_vector(&bytes).expect("decode vector image");
println!("{}x{}", image.logical_width(), image.logical_height());

let mut canvas = bcanvas::OffscreenCanvas::new(256, 256);
canvas.draw(|ctx| ctx.draw_canvas(&image, 0.0, 0.0, 256.0, 256.0));
```

Both formats are enabled by default. Use `default-features = false` with `svg`
or `tinyvg` to select a decoder. Input is limited to 64 MiB. SVGZ is detected from its gzip signature and uses
the SVG decoder after decompression, also limited to 64 MiB.

## SVG subset

Static SVG supports common geometry, viewports, transforms, CSS presentation
styles, gradients, clipping, masks, markers, and paint order. Valid but
unsupported content, including text, images, filters, scripts, animation, and
external resources, is skipped.

## Tests and benchmarks

Run `cargo test -p vector` and `cargo bench -p vector --bench vector`. The SVG
fixtures come from WPT revision `987a2d0c1a45f1a193f1f05d9508c4efc307c3bf`.
The TinyVG examples come from revisions `e7c4c624fbe9276740fff2a9b6231ff86e01a89c`
and `b8d8c7e88ed221f2ce1100f9e25b5c6e7e6dc78d`.

## License

Copyright © 2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../../LICENSE) license.
