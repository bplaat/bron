# Windows XP development toolchain

The XP image build cross-compiles the 32-bit tools from source with the C compiler of the
[Windows XP toolchain](../../../../../tools/winxp-toolchain) (`bin/cc`) and stages them in
`target/winxp-dev-toolchain/Toolchain/`.

- NASM and NDISASM 2.16.03
- TinyCC 0.9.28rc, upstream commit `56b75101ef323cca9e45c925ba17042ce87134de`
- GNU windres 2.42
- GNU Make 4.4.1

The XP image installs the kit to `%ProgramFiles%\Toolchain` and appends its `bin` directory to the system PATH.
Build either project under `%USERPROFILE%\My Documents\My Projects` from an XP command prompt.

```bat
cd /d "%USERPROFILE%\My Documents\My Projects\hello-world"
make
```

Hello World draws centered text with GDI. Clock uses GDI+ and the XP
side-by-side GDI+ assembly.
