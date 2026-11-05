# ImageView

A simple native image viewer for macOS, Windows and Linux that opens common images, animations and
vector drawings, and follows the system light or dark theme.

## Features

- Decode BMP, GIF, ICNS, ICO, JPEG, JPEG 2000, PNG/APNG, QOI, TGA, and WebP with a custom [image](../../../lib/graphics/image) decoder crate
- Play APNG/GIF/WebP animations with frame timings and loop counts
- Render SVG/SVGZ and TinyVG with the shared [vector](../../../lib/graphics/vector) decoder, drawn
  by [imageview-core](../imageview-core) with [bcanvas](../../../lib/graphics/bcanvas): Core
  Graphics on macOS, Direct2D on Windows and Cairo on Linux
- Zoom, pan, and fit images to the window
- Open slow to decode images in a window with a native loading indicator right away
- Automatically refresh an open image when its file changes, without resetting the zoom
- Browse previous and next images in the same folder
- Drop an image onto a window to show it there
- File manager previews and thumbnails for supported formats

### Editing

An Edit button swaps in an edit toolbar for still bitmaps, with popovers for its tools:

- Rotate and flip, saved to JPEG files by changing their EXIF orientation instead of encoding
  them again
- Crop with aspect ratios and straightening, and resize
- Adjust the light (exposure, brightness, contrast, highlights, shadows, vignette) and colors
  (saturation, vibrance, warmth, tint, hue)
- Auto enhance, grayscale, sepia, invert, threshold, solarize, gaussian, box, motion and radial
  blur, sharpen, noise reduction and pixelate
- Rectangle and ellipse selections that edits only change, with feathered edges, which are
  flipped in place, copied, or cropped to with a transparent background for ellipses
- Previews on a copy as large as the window, edits applied in the background, and the last
  change set up again when its tool is opened again
- Undo and redo within a memory budget, and saving with a JPEG quality, the EXIF metadata of
  photos and the smallest encoding for pictures up to 1024x1024
- Save As, in the file dialog of the platform with controls for the format, JPEG quality and
  size, draws vectors as bitmaps at a chosen size, and saves the first frame of animations,
  which cannot be edited

### macOS

- A toolbar in the titlebar with previous and next image and the zoom commands, and the
  image size below the file name
- Print images at their natural size or scaled to fit, and reopen them from Open Recent
- Quick Look [preview](../imageview-macos-preview) and [thumbnail](../imageview-macos-thumbnail)
  extensions
- Edit tools in popovers below the unified toolbar, and Save As controls in the save sheet

### Windows

- A WinUI 3 window from one XAML island, through handwritten Windows App SDK bindings without
  extra crates: a header bar with the toolbar buttons in the title bar, like a GTK header bar or
  a unified macOS toolbar, with Mica and tall caption buttons, above a ScrollViewer with overlay
  scroll bars, touch and touchpad pinch zoom and panning by dragging. Vector images are drawn
  again at the magnification they settle at, so they stay sharp
- File Explorer thumbnails and preview pane from the [shell extension](../imageview-windows-shell),
  registered by the MSIX package
- Native file icons for each image format, with editable [SVG sources](docs/images/windows/file-types/)
  and artwork for small icons, registered by the MSIX package for Details and other icon views;
  the compressed ICO assets live in `meta/windows/file-types/`
- Light and dark themes, also for the scroll bars and caption buttons
- The edit controls in the header bar, with the controls of a tool in a card over the image and
  the Filters in a menu flyout. Save As uses the file types of the dialog for the format and adds
  combo boxes for the JPEG quality and size

### Linux

- Native GTK3 controls for previous and next image, zoom out, actual size, and zoom in,
  with a header bar on GNOME (including Ubuntu), Budgie, and Pantheon, and image dimensions
  beneath the filename. Other desktops use a normal titlebar showing `filename (widthxheight)`
  and an inline toolbar containing only buttons.
  Detection uses `XDG_CURRENT_DESKTOP` on both X11 and Wayland
- The edit controls in the same place, with GtkPopovers for the tools and Image and Edit menus
  in the application menu. Save As uses a GtkFileChooserDialog for its extra controls, the
  other file dialogs a native chooser
- A freedesktop thumbnailer: `ImageView --thumbnail -s SIZE INPUT OUTPUT` writes a PNG
  thumbnail, without starting GTK
- `cargo xtask install` installs the binary, the [desktop entry](meta/freedesktop/.desktop),
  the [thumbnailer](meta/freedesktop/.thumbnailer) and the [MIME types](meta/freedesktop/mime.xml)
  of QOI and TinyVG into the user's data directory

## Windows file icons

The MSIX package registers custom file icons for each supported image format with editable
[SVG sources](docs/images/windows/file-types/) and shared document bases.
The ICO files are embedded in the Windows executable and referenced by resource ID in the MSIX manifest.
ICO sizes 16, 20, 24, 32, 40, and 48 px use the `.small.svg` artwork;
sizes 64, 96, 128, and 256 px use the regular `.svg` artwork.
The PNG images embedded in `meta/windows/file-types/` ICO assets are losslessly optimized.

## macOS Entitlements

The main app does not use the `com.apple.security.app-sandbox` entitlement because `com.apple.security.files.user-selected.read-only` only permits reading the selected image, not the other images in the same folder. ImageView needs access to those files to browse to the previous and next images.

## Screenshot

![ImageView Screenshot](docs/images/screenshot.png)

## License

Copyright © 2026 [Bastiaan van der Plaat](https://bplaat.nl/)

Licensed under the [MIT](../../../LICENSE) license.
