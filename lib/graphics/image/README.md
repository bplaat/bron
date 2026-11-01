# Image

Decode JPEG, PNG/APNG, GIF, BMP, ICO, and QOI images into sRGB RGBA pixels. You can
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

To write an image, create a `Bitmap` from sRGB RGBA pixels and call `encode`.
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
selects the largest image in an ICO file. Streaming is not supported. Decoding
has a cumulative 1 GiB allocation limit.

## Color management

Decoded pixels are always sRGB. Each decoder reads the color description of
the file and converts the pixels, clipping colors outside the sRGB gamut:

| Format | Color description                                                                      |
| ------ | -------------------------------------------------------------------------------------- |
| PNG    | `cICP`, then `iCCP`, then `sRGB`, then `cHRM` and `gAMA`; 16-bit samples convert first |
| JPEG   | ICC profile in APP2 segments, otherwise the EXIF Adobe RGB color space                 |
| BMP    | V4/V5 calibrated RGB endpoints with gamma, or an embedded ICC profile                  |
| QOI    | The linear RGB header flag                                                             |

ICC v2 and v4 profiles for gray, RGB, and CMYK samples are supported, both
matrix/TRC profiles and `mft1`, `mft2`, and `mAB ` lookup tables. HDR transfer
functions (PQ and HLG) map SDR white to sRGB white and clip brighter colors.
Images without a color description, and profiles that are invalid or do not
match the samples, are treated as sRGB.

To average pixels correctly, for example when resizing or building mipmaps,
convert samples with `srgb_to_linear`, combine them in linear light, and encode
the result with `linear_to_srgb`.

All formats are enabled by default. You can enable only the codecs you need
with the `qoi`, `jpeg`, `png`, `gif`, `bmp`, and `ico` Cargo features. The crate
is `no_std` and only needs `alloc`.

## Tests and benchmarks

Run `cargo test -p image` for tests or `cargo bench -p image --bench image` for
benchmarks. Tests use reference images from image-rs.

## License

Copyright © 2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../../LICENSE) license.
