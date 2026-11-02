# wuhbtool

A Rust replacement for devkitPro's `wuhbtool`, used by the [Wii U toolchain](..). It bundles a
Wii U program with its name, author, icon and splash screens into a `.wuhb` file, which the
Aroma environment shows on the Wii U Menu like an installed title.

## Getting started

The command line is the same as devkitPro's:

```sh
wuhbtool cube.rpx cube.wuhb --name="Cube" --author="Bastiaan" \
    --icon=icon.png --tv-image=tv.png --drc-image=drc.png --content=content
```

- `--name` and `--short-name` default to each other, and both to the program's file name
- `--icon` is the 128x128 icon on the Wii U Menu, `--tv-image` (1280x720) and `--drc-image`
  (854x480) are the splash screens on the TV and the GamePad while the program starts. Any
  image our [image](../../../lib/graphics/image) crate reads works, other sizes are resized with
  a warning
- `--content` adds a directory as `content/`, which the program reads at runtime

Or use the library, for example from a build script:

```rs
let bundle = wuhbtool::build(&wuhbtool::Options {
    rpx: "cube.rpx".into(),
    name: Some("Cube".to_owned()),
    icon: Some("icon.png".into()),
    ..Default::default()
})
.expect("build bundle");
std::fs::write("cube.wuhb", bundle.data).expect("write bundle");
```

## Format

A `.wuhb` file is a RomFS archive with the `WUHB` magic, laid out like devkitPro's wuhbtool
writes it:

```text
code/root.rpx           the program
meta/meta.ini           [menu] longname, shortname and author
meta/iconTex.tga.gz     the icon, a 32-bit TGA
meta/bootTvTex.tga.gz   the TV splash screen, a 24-bit TGA
meta/bootDrcTex.tga.gz  the GamePad splash screen, a 24-bit TGA
content/...             the files of --content
```

The images are gzipped TGA files, because the Wii U Menu reads the TGA images of installed
titles. Unlike devkitPro's wuhbtool, resizing weights the colors by alpha so transparent pixels
don't darken the edges, the splash screens are drawn over black instead of dropping their alpha,
and the files of `content/` are sorted by name, so bundles are reproducible.
