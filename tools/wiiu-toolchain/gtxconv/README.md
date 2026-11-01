# gtxconv

A Wii U texture converter, used by the [Wii U toolchain](..). It converts any image our
[image](../../../lib/graphics/image) crate reads to GX2 RGBA8 or BC1 (DXT1) textures, optionally
with mipmaps, and writes them to a GFD `.gtx` texture file that wut's libgfd reads. Like a TPL
file, a `.gtx` file holds many textures.

The textures are written in the tiled layout of the Latte GPU, with the sizes, alignment and
mipmap offsets `GX2CalcSurfaceSizeAndAlignment` computes and the texture registers
`GX2InitTextureRegs` sets. So the GPU reads them as they are, the image and mipmaps of each
texture are aligned in the file. Big levels are macro tiled (`2D_TILED_THIN1`) and the levels
smaller than a macro tile micro tiled (`1D_TILED_THIN1`), like AMD's R600 addrlib lays them out.

## Getting started

Convert the textures of a config file with the command line tool:

```sh
gtxconv -c textures.toml -o textures.gtx
```

The config lists the textures in the order of the `.gtx` file, the image paths are relative to
the config file:

```toml
[[textures]]
path = "dirt.png"
format = "bc1"
mipmaps = true
```

- `format` is `rgba8` (default) for 32-bit RGBA textures or `bc1` to compress them to 4 bits per
  pixel, BC1 has no alpha
- `mipmaps = true` adds mipmaps down to one pixel on the shortest side, they need power of two
  sizes

Or use the library, for example from a `build.rs`:

```rs
let gtx = gtxconv::convert_config(Path::new("textures.toml")).expect("convert textures");
std::fs::write("textures.gtx", gtx).expect("write textures");
```

Mipmaps are averaged in linear light like GPUs filter sRGB textures, and keep the alpha test
coverage of cutouts like leaves.
