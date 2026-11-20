# Windows Examples

Pure 32-bit and 64-bit win32 assembly windows programs and some gcc compiler to assembly custom win32 header stuff.

## Building

Install [NASM](https://nasm.us/) and assemble an example from this directory:

```sh
nasm -f bin hello.asm -o hello-x86.exe
nasm -DWIN64 -f bin hello.asm -o hello-x64.exe
```

Run the executables on Windows or with Wine.

To build all examples with NASM and Make, run:

```sh
./build.sh
```

To build only RedSquare for x86 and x64, run `make -C redsquare`.

## License

Copyright © 2020-2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../LICENSE) license.
