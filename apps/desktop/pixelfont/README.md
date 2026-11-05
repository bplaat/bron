# Pixel Font Editor

An 8x8 pixel font editor built with [bwebview](../../../lib/ui/bwebview), a clone of the [8x8 Pixel Font Editor](https://www.min.at/prinz/o/software/pixelfont/) by Richard Prinz.

## Features

- Open and save `.pf` font files (flat binary, 256 chars x 8 bytes)
- 16x16 character grid showing all 256 characters
- Pixel-by-pixel editing with click and drag
- Per-character operations: Clear, Invert, Rotate (90 CW), Mirror (Horizontal/Vertical)
- Bulk operations: apply Clear/Invert/Rotate/Mirror to all 256 characters
- Copy hex bytes to clipboard / paste from clipboard
- Export as assembler file (`.asm`) or C header file (`.h`)

## Windows file icons

The MSIX package registers a custom `.pf` file icon with editable
[SVG sources](docs/images/windows/file-types/) and copies of the shared document bases.
The ICO files are embedded in the Windows executable and referenced by resource ID in the MSIX manifest.
ICO sizes 16, 20, 24, 32, 40, and 48 px use the `.small.svg` artwork;
sizes 64, 96, 128, and 256 px use the regular `.svg` artwork.

## Screenshot

![Pixel Font Editor Screenshot](docs/images/screenshot.png)

## License

Copyright © 2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../../LICENSE) license.
