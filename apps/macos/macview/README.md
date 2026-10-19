# MacView

A simple native macOS image viewer with local raster decoders and support for SVG and TinyVG images.

## Features

- Decode JPEG, PNG/APNG, GIF, BMP, and QOI with a custom [image](../../../lib/graphics/image) decoder crate
- Play GIF/APNG animations with frame timings and loop counts
- Render SVG and TinyVG with the shared [vector](../../../lib/graphics/vector) decoder and AppKit renderer
- Fall back to NSImage when local decoding fails, including 16-bit PNG files
- Zoom, pan, and fit images to the window
- Automatically refresh an open image when its file changes, without resetting the zoom
- Browse previous and next images in the same folder
- Print images at their natural size or scaled to fit
- Quick Look previews and thumbnails for supported formats
- Edit still images with a native toolbar and an Image menu:
    - Select rectangles and ellipses, move and resize them, or select an aspect ratio
    - Crop, rotate, straighten, flip, and resize the image or its canvas
    - Adjust light and color, apply preset filters, blur, sharpen, and add a vignette, to the whole
      image or a feathered selection, with live previews
    - Undo and redo every edit, compare with the original, and reset to it
    - Save and export PNG, JPEG (with adjustable quality), BMP, GIF, and QOI, keeping transparency
      where the format supports it
    - Copy the image or selection, and open a new image from the clipboard

## macOS Entitlements

The main app does not use the `com.apple.security.app-sandbox` entitlement because `com.apple.security.files.user-selected.read-only` only permits reading the selected image, not the other images in the same folder. MacView needs access to those files to browse to the previous and next images.

## Screenshot

![MacView Screenshot](docs/images/screenshot.png)

## License

Copyright © 2026 [Bastiaan van der Plaat](https://bplaat.nl/)

Licensed under the [MIT](../../../LICENSE) license.
