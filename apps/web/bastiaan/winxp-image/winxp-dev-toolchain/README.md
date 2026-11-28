# Windows XP development toolchain

The XP image build cross-compiles the 32-bit tools from source with the
[Windows XP toolchain](../../../../../tools/winxp-toolchain) and stages them in `target/winxp-dev-toolchain/Toolchain/`.

- NASM 2.16.03
- TinyCC 0.9.28rc, upstream commit `56b75101ef323cca9e45c925ba17042ce87134de`
- GNU windres 2.42
- GNU Make 4.4.1

The XP image installs the kit to `%ProgramFiles%\Toolchain` and appends its `bin` directory to the system PATH.
Build any project under `%USERPROFILE%\My Documents\My Projects` from an XP command prompt.

```bat
cd /d "%USERPROFILE%\My Documents\My Projects\hello-world"
make
```

## Headers and libraries

TinyCC's own Windows headers cover the Win32 API and GDI. The kit adds, from the mingw-w64 headers the Windows XP
toolchain builds, configured like XP SP3 with Internet Explorer 6 (`_WIN32_IE` 0x0600):

- GDI+ (`gdiplus.h`), its flat C API
- Direct3D 8 (`d3d8.h`) and DirectDraw (`ddraw.h`), on the COM base of `include/` (`objbase.h`, `unknwn.h`,
  `ole2.h`): interfaces are C structs with the `IDirect3D8_CreateDevice(...)` style macros
- Common controls (`commctrl.h`), common dialogs (`commdlg.h`) and multimedia (`mmsystem.h`)

Import definitions: kernel32, user32, gdi32, msvcrt, ws2_32, gdiplus, advapi32, comctl32, comdlg32, d3d8, ddraw,
msimg32, ole32, oleaut32, shell32 and winmm, link with `-l<name>`. `-luuid` and `-ldxguid` have the interface IDs of
COM, DirectDraw and Direct3D 8, so programs need no `INITGUID`.

## TinyCC patches

`patches/` holds the changes to the sources the kit builds from, applied when they are unpacked:

- `#embed` (C23) with `limit()`, `prefix()`, `suffix()` and `if_empty()`; unknown preprocessing directives are errors
- `printf`, `fprintf`, `sprintf`, `snprintf` and their `v` forms format like C99 (`%lld`, `%hhd`, `%jd`, `%zu`, `%td`,
  a terminating `snprintf` that returns the full length), which XP's `msvcrt.dll` doesn't. Floating point and the wide
  functions keep msvcrt's formatting, `__TCC_MSVCRT_PRINTF` keeps msvcrt's for everything.
- Windows XP as the default target of the headers

## Projects

- Hello World: centered text with GDI
- Clock: an analog clock with GDI+ and the XP side-by-side GDI+ assembly
- Triangle: a rotating colored triangle with Direct3D 8 fixed function
- Cube: a rotating textured crate with Direct3D 8, the texture built in with `#embed` and decoded with GDI+
- Blocks: a random voxel world with Direct3D 8, its textures built in with `#embed` and decoded with GDI+

Triangle, Cube and Blocks need a Direct3D 8 device, which the paravirtualized graphics adapter doesn't provide yet;
without one they say so.
