# Image

Decode JPEG, PNG/APNG, GIF, BMP, ICO, and QOI images into RGBA pixels. You can
also encode images as JPEG, PNG/APNG, GIF, BMP, or QOI.

## Getting started

```rs
let bytes = std::fs::read("picture.png").expect("read image");
let image = image::decode(&bytes).expect("decode image");
for frame in image.frames() {
    let bitmap = frame.bitmap();
    println!("{}x{}: {:?}", bitmap.width(), bitmap.height(), frame.delay());
}
```

To write an image, create a `Bitmap` from RGBA pixels and call `encode`.
Use `Image::encode` to save a decoded image in another format.

For GIF or APNG, give `encode_animation` a list of full-size frames, their
display times, and a loop count. GIF supports up to 256 colors per frame and
binary transparency. JPEG requires opaque pixels.

PNG/APNG files can carry text entries. Set `EncodeOptions::text` to keyword and
text pairs: Latin-1 text is written as `tEXt` and other text as UTF-8 `iTXt`.
Decoding reads `tEXt`, `zTXt`, and `iTXt` entries into `Image::text`.

## Compression

Normal compression is the default. For PNG/APNG, JPEG, or GIF, you can spend more
time looking for a smaller file. Set `EncodeOptions::style` to `MaxCompression`
and use `encode_with_options` or `encode_animation_with_options`. It keeps the
smallest result it tries.

| Format   | Normal compression                                                                                                                                                                    | Max compression                                                                                                                                                                  |
| -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| BMP      | Chooses 24-bit, 32-bit, packed palette, or RLE8 pixels with literal runs.                                                                                                             | Same as normal.                                                                                                                                                                  |
| GIF      | Uses an exact palette when possible, otherwise reduces colors with median cut and k-means, applies light error diffusion, then LZW. Opaque animation frames store only changed areas. | Also tries exact nearest-color mapping and lighter diffusion.                                                                                                                    |
| PNG/APNG | Uses a palette or color key when possible, and keeps the smaller of unfiltered and adaptive row filters with default Deflate compression. APNG frames store only changed areas.       | Also tries every pixel layout, brute-force row filters, cleared transparent colors, and full APNG frames, then recompresses the smallest result with crunch Deflate compression. |
| JPEG     | Uses grayscale or 4:2:0 color and builds coding tables for the image.                                                                                                                 | Also compares 4:2:2, 4:4:0, and 4:4:4 color.                                                                                                                                     |
| QOI      | Uses run, index, difference, and luma operations.                                                                                                                                     | Same as normal.                                                                                                                                                                  |

JPEG's color sampling can change how the image looks between compression styles.
BMP, PNG/APNG, and QOI preserve RGBA pixels, except that max compression may
replace the color of fully transparent PNG/APNG pixels.

## Reading

Animated images expose full-size frames, display times, and a loop count.
Decoding applies JPEG orientation, reduces 16-bit PNG samples to 8 bits, and
selects the largest image in an ICO file. HDR, streaming, and ICC color
management are not supported. Decoding has a cumulative 1 GiB allocation limit.

All formats are enabled by default. You can enable only the codecs you need
with the `qoi`, `jpeg`, `png`, `gif`, `bmp`, and `ico` Cargo features. The crate
is `no_std` and only needs `alloc`.

## Tests and benchmarks

Run `cargo test -p image` for tests or `cargo bench -p image --bench image` for
benchmarks. Tests use reference images from image-rs.

## License

Copyright © 2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../../LICENSE) license.
