# gxtexconv

A minimal Rust replacement for devkitPro's `gxtexconv`, used by the [Wii toolchain](../../../tools/wii-toolchain).
It converts any image our [image](../image) crate reads to GameCube / Wii RGBA8 (`colfmt="6"`) or
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
