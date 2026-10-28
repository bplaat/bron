# ImageView

A simple native image viewer for macOS, Windows and Linux with local raster decoders and support
for SVG and TinyVG images. Windows and the event loop come from [bwindow](../../../lib/ui/bwindow),
which hosts a native scroll view: an AppKit NSScrollView, a Win32 window with scroll bars, or a
GtkScrolledWindow, all following the system light or dark theme.

## Features

- Decode BMP, GIF, JPEG, PNG/APNG, ICO, and QOI with a custom [image](../../../lib/graphics/image) decoder crate
- Play GIF/APNG animations with frame timings and loop counts
- Render SVG and TinyVG with the shared [vector](../../../lib/graphics/vector) decoder, drawn
  by [imageview-core](../imageview-core) with [bcanvas](../../../lib/graphics/bcanvas): Core
  Graphics on macOS, Direct2D on Windows and Cairo on Linux
- Zoom, pan, and fit images to the window
- Automatically refresh an open image when its file changes, without resetting the zoom
- Browse previous and next images in the same folder
- File manager previews and thumbnails for supported formats

### macOS

- Print images at their natural size or scaled to fit, and reopen them from Open Recent
- Quick Look [preview](../imageview-macos-preview) and [thumbnail](../imageview-macos-thumbnail)
  extensions

### Windows

- File Explorer thumbnails and preview pane from the [shell extension](../imageview-windows-shell),
  which the MSIX package registers without changing the registry
- Dark scroll bars in the dark theme

### Linux

- A freedesktop thumbnailer: `ImageView --thumbnail -s SIZE INPUT OUTPUT` writes a PNG
  thumbnail, without starting GTK
- `cargo xtask install` installs the binary, the [desktop entry](meta/freedesktop/.desktop),
  the [thumbnailer](meta/freedesktop/.thumbnailer) and the [MIME types](meta/freedesktop/mime.xml)
  of QOI and TinyVG into the user's data directory

## macOS Entitlements

The main app does not use the `com.apple.security.app-sandbox` entitlement because `com.apple.security.files.user-selected.read-only` only permits reading the selected image, not the other images in the same folder. ImageView needs access to those files to browse to the previous and next images.

## Screenshot

![ImageView Screenshot](docs/images/screenshot.png)

## License

Copyright © 2026 [Bastiaan van der Plaat](https://bplaat.nl/)

Licensed under the [MIT](../../../LICENSE) license.
