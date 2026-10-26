# gxtexconv

A minimal Rust replacement for devkitPro's `gxtexconv`, used by the [Wii toolchain](..).
It converts any image our [image](../../../lib/graphics/image) crate reads to GameCube / Wii RGB5A3 (`colfmt="5"`), RGBA8 (`colfmt="6"`) or
CMPR (`colfmt="14"`) textures and writes them to a `.tpl` texture file, with a better CMPR
compressor than the original.

## Getting started

Convert a `.scf` script with the command line tool:

```sh
gxtexconv -s textures.scf -o textures.tpl
```

Or use the library, for example from a `build.rs`:

```rs
let tpl = gxtexconv::convert_script(Path::new("textures.scf")).expect("convert textures");
std::fs::write("textures.tpl", tpl).expect("write textures");
```

A script lists one image per line:

```xml
<filepath="dirt.png" id="dirt" colfmt="14" />
```

Mipmaps use devkitPro's options: `mipmap="yes"` with `maxlod` adds the levels up to the image
halved `maxlod` times, `minlod` skips the largest levels and `remaplod` moves their LOD numbers in
the TPL file. Without `maxlod` that's only the full image, like devkitPro's gxtexconv:

```xml
<filepath="dirt.png" id="dirt" colfmt="14" mipmap="yes" maxlod="4" />
```

Mipmaps need power of two sizes and levels smaller than a tile are padded. Unlike devkitPro, they
are averaged in linear light like GPUs filter sRGB textures, and keep the alpha test coverage of
cutouts like leaves. `Texture::load_mipmapped` adds all levels down to one tile.
