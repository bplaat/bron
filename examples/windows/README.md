# Windows Examples

Pure 32-bit and 64-bit win32 assembly windows programs and some gcc compiler to assembly custom win32 header stuff.

## Building

Install [NASM](https://nasm.us/) and assemble an example from this directory:

```sh
nasm -f bin hello.asm -o hello-x86.exe
nasm -DWIN64 -f bin hello.asm -o hello-x64.exe
```

Run the executables on Windows or with Wine.

To build all examples using only NASM and a POSIX shell, run:

```sh
./build.sh
```

The ICO embeds 8-bit-per-channel RGBA PNGs. When updating its images, use `oxipng --nc --nb` to preserve that encoding and update the image offsets and lengths in `redsquare/redsquare.asm`.

## License

Copyright © 2020-2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../LICENSE) license.
