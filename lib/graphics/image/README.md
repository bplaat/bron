# Image

Decode BMP, GIF, ICNS, ICO, JPEG, JPEG 2000, PNG/APNG, QOI, and TGA images into sRGB RGBA
pixels. You can also encode images as BMP, GIF, JPEG, PNG/APNG, QOI, or TGA.

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

`Format::from_extension` and `Format::from_signature` find the format of a file name or
encoded bytes, and `Format::can_encode` tells whether images can be written in it.

PNG/APNG files can carry text entries. Set `EncodeOptions::png_text` to keyword and
text pairs: Latin-1 text is written as `tEXt` and other text as UTF-8 `iTXt`.
Decoding reads `tEXt`, `zTXt`, and `iTXt` entries into `Image::text`.

## Compression

Normal compression is the default. For GIF, JPEG, or PNG/APNG, you can spend more
time looking for a smaller file. Set `EncodeOptions::compression` to
`Compression::Max` and use `encode_with_options` or
`encode_animation_with_options`. It keeps the smallest result it tries.

Set `EncodeOptions::compression` to `Compression::None` to store every pixel in
full: 24 or 32-bit BMP and TGA pixels, RGB or RGBA operations in QOI, and RGB or
RGBA PNG/APNG rows in stored Deflate blocks with full frames. GIF still reduces
colors to a palette but writes uncompressed LZW codes and full frames. JPEG is
always lossy, so it only skips chroma subsampling.

| Format   | Normal compression                                                                                                                                                                    | Max compression                                                                                                                                                                  |
| -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| BMP      | Chooses 24-bit, 32-bit, packed palette, or RLE8 pixels with literal runs.                                                                                                             | Same as normal.                                                                                                                                                                  |
| GIF      | Uses an exact palette when possible, otherwise reduces colors with median cut and k-means, applies light error diffusion, then LZW. Opaque animation frames store only changed areas. | Also tries exact nearest-color mapping and lighter diffusion.                                                                                                                    |
| JPEG     | Uses grayscale or 4:2:0 color and builds coding tables for the image.                                                                                                                 | Also compares 4:2:2, 4:4:0, and 4:4:4 color.                                                                                                                                     |
| PNG/APNG | Uses a palette or color key when possible, and keeps the smaller of unfiltered and adaptive row filters with default Deflate compression. APNG frames store only changed areas.       | Also tries every pixel layout, brute-force row filters, cleared transparent colors, and full APNG frames, then recompresses the smallest result with crunch Deflate compression. |
| QOI      | Uses run, index, difference, and luma operations.                                                                                                                                     | Same as normal.                                                                                                                                                                  |
| TGA      | Tries grayscale and palettes of up to 256 colors, with RLE packets within each row.                                                                                                   | Same as normal.                                                                                                                                                                  |

JPEG's color sampling can change how the image looks between compression levels.
BMP, PNG/APNG, QOI, and TGA preserve RGBA pixels, except that max compression may
replace the color of fully transparent PNG/APNG pixels.

## JPEG metadata

`reorient_jpeg` turns or mirrors a JPEG file by changing its EXIF orientation, so its
pixels are not encoded again and keep their quality. Orientations combine with
`Orientation::then`. `copy_jpeg_exif` adds the EXIF metadata of a JPEG file, like its
capture date, camera and location, to a JPEG of its decoded pixels encoded again: the
orientation and color space are reset, as decoding applies them, the pixel size follows
the new pixels and the thumbnail of the original is left out.

## Reading

Animated images expose full-size frames, display times, and a loop count.
Decoding applies JPEG orientation, reduces 16-bit PNG samples to 8 bits, and
selects the largest image in an ICNS or ICO file. TGA has no file signature, so it is
only detected when no other format matches and its header is valid. Streaming
is not supported. Decoding has a cumulative 1 GiB allocation limit.

TGA decoding honors declared alpha and the TGA 2.0 extension attributes type,
including premultiplied alpha. Legacy files without declared alpha whose alpha
bytes are all zero are treated as opaque. TGA gamma, color-correction tables,
and other extension metadata are not interpreted.

JPEG 2000 reads JP2 containers and raw codestreams (`j2c`, `j2k`, and `jpc`).
It supports reversible and irreversible wavelets, tiles, quality layers, custom
precincts, all five progression orders, multiple tile-parts, arithmetic bypass,
and per-pass termination. JPEG 2000 is decode-only. Its allocation budget releases
scratch buffers between code-blocks, resolutions, and tiles.

ICNS selects the largest image and supports PNG, JPEG 2000, and legacy RGB/ARGB
entries with masks. Palette entries and ICNS encoding are unsupported.

## Color management

Decoded pixels are always sRGB. Each decoder reads the color description of
the file and converts the pixels, clipping colors outside the sRGB gamut:

| Format | Color description                                                                      |
| ------ | -------------------------------------------------------------------------------------- |
| BMP    | V4/V5 calibrated RGB endpoints with gamma, or an embedded ICC profile                  |
| JPEG   | ICC profile in APP2 segments, otherwise the EXIF Adobe RGB color space                 |
| PNG    | `cICP`, then `iCCP`, then `sRGB`, then `cHRM` and `gAMA`; 16-bit samples convert first |
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
with the `bmp`, `gif`, `icns`, `ico`, `jpeg`, `jpeg2000`, `png`, `qoi`, and `tga` Cargo features.
The default `std` feature uses Rust math functions. Disable it for `no_std` with `alloc`
and C library math (`exp`, `pow`, `round`, `roundf`, and `truncf`, depending on the codecs).
The sRGB tables are generated at build time using host `std` math.

## Tests and benchmarks

Run `cargo test -p image` for tests or `cargo bench -p image --bench image` for
benchmarks. Tests use reference images from image-rs, and synthetic TGA images
plus ones written by macOS `sips`.

JPEG 2000 fixtures and RGBA references use OpenJPEG.

## License

Copyright © 2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../../LICENSE) license.
