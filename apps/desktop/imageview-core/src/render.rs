/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::RefCell;

use bcanvas::{CanvasImage, CanvasRenderingContext2d, Color as CanvasColor, OffscreenCanvas};

use crate::Media;

/// Draws media with a canvas, keeping the canvas images of its frames.
pub struct MediaRenderer {
    media: Media,
    images: RefCell<Vec<Option<CanvasImage>>>,
}

impl MediaRenderer {
    /// Creates a renderer that owns the media.
    pub const fn new(media: Media) -> Self {
        Self {
            media,
            images: RefCell::new(Vec::new()),
        }
    }

    /// Returns the media.
    pub const fn media(&self) -> &Media {
        &self.media
    }

    /// Draws a frame of the media scaled into a rectangle.
    pub fn draw(
        &self,
        context: &mut CanvasRenderingContext2d<'_>,
        frame: usize,
        rect: (f32, f32, f32, f32),
    ) {
        let (x, y, width, height) = rect;
        match &self.media {
            Media::Raster(raster) => {
                let Some(pixels) = raster.frames.get(frame).map(|frame| &frame.pixels) else {
                    return;
                };
                let mut images = self.images.borrow_mut();
                images.resize(raster.frames.len(), None);
                let image = images[frame].get_or_insert_with(|| {
                    CanvasImage::from_premultiplied_bgra(
                        raster.width,
                        raster.height,
                        pixels.clone(),
                    )
                });
                // Enlarged pixels are drawn as sharp squares, like those of a small icon or a
                // zoomed in photo, shrunk bitmaps are smoothed.
                context.save();
                context.set_image_smoothing_enabled(
                    !self.is_pixelated(width * context.scale_factor()),
                );
                context.draw_image(image, x, y, width, height);
                context.restore();
            }
            Media::Vector(vector) => context.draw_canvas(vector, x, y, width, height),
        }
    }

    /// Whether the media is a bitmap drawn `device_width` device pixels wide with its pixels as
    /// sharp squares, instead of smoothed.
    pub fn is_pixelated(&self, device_width: f32) -> bool {
        matches!(&self.media, Media::Raster(raster) if is_pixelated(device_width, raster.width))
    }

    /// Draws the media scaled to fit inside a rectangle, centered and keeping its shape.
    pub fn draw_fitted(
        &self,
        context: &mut CanvasRenderingContext2d<'_>,
        frame: usize,
        rect: (f32, f32, f32, f32),
    ) {
        let (width, height) = self.media.size();
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        let scale = (f64::from(rect.2) / width).min(f64::from(rect.3) / height);
        let (fitted_width, fitted_height) = ((width * scale) as f32, (height * scale) as f32);
        self.draw(
            context,
            frame,
            (
                rect.0 + (rect.2 - fitted_width) / 2.0,
                rect.1 + (rect.3 - fitted_height) / 2.0,
                fitted_width,
                fitted_height,
            ),
        );
    }
}

// The device pixels per bitmap pixel from which pixels are drawn as squares. Smaller
// enlargements are smoothed, as nearest neighbor scaling would make some pixels wider than
// others.
const PIXELATED_SCALE: f32 = 2.0;

// Whether a bitmap of a width drawn at a width in device pixels shows its pixels as squares.
fn is_pixelated(device_width: f32, width: u32) -> bool {
    device_width >= PIXELATED_SCALE * width as f32
}

/// Renders media fitted within a square of `size` pixels, without enlarging it, as a bitmap of
/// straight-alpha RGBA pixels.
pub fn render_thumbnail(media: Media, size: u32) -> Option<image::Bitmap> {
    let (width, height) = media.size();
    if width <= 0.0 || height <= 0.0 || size == 0 {
        return None;
    }
    let scale = (f64::from(size) / width.max(height)).min(1.0);
    render_media(media, scale)
}

/// Renders the first frame of media at a scale of its natural size, as a bitmap of
/// straight-alpha RGBA pixels. Vectors are drawn at that size, so they stay sharp.
pub fn render_media(media: Media, scale: f64) -> Option<image::Bitmap> {
    let (width, height) = media.size();
    if width <= 0.0 || height <= 0.0 || scale <= 0.0 {
        return None;
    }
    let (width, height) = (
        (width * scale).round().max(1.0) as u32,
        (height * scale).round().max(1.0) as u32,
    );
    let renderer = MediaRenderer::new(media);
    let mut canvas = OffscreenCanvas::new(width, height);
    canvas.draw(|context| {
        renderer.draw(context, 0, (0.0, 0.0, width as f32, height as f32));
    });
    image::Bitmap::new(width, height, canvas.pixels().to_vec()).ok()
}

// The width and height of a single checkerboard tile.
const TILE: f32 = 12.0;

/// Draws the checkerboard shown behind transparent media, in light or dark colors. Its tiles
/// start at the center, so they do not move while the area is resized.
pub fn draw_checkerboard(
    context: &mut CanvasRenderingContext2d<'_>,
    width: f32,
    height: f32,
    dark: bool,
) {
    draw_checkerboard_clipped(context, (width, height), (0.0, 0.0, width, height), dark);
}

/// Draws only the part of the checkerboard of an area inside a clip rectangle, for an area much
/// larger than what is shown, like a scrolled document.
pub fn draw_checkerboard_clipped(
    context: &mut CanvasRenderingContext2d<'_>,
    (width, height): (f32, f32),
    (clip_x, clip_y, clip_width, clip_height): (f32, f32, f32, f32),
    dark: bool,
) {
    let ([red, green, blue], alternate) = checker_colors(dark);
    context.set_fill_style(CanvasColor::rgb(red, green, blue));
    context.fill_rect(clip_x, clip_y, clip_width, clip_height);
    // The alternate tiles are filled as one path, which is a single draw call instead of one
    // for each tile.
    context.begin_path();
    let (half_columns, half_rows) = checker_grid(width, height);
    let tiles = |start: f32, length: f32, center: f32, half: i32| {
        let first = ((start - center) / TILE).floor() as i32;
        let last = ((start + length - center) / TILE).ceil() as i32;
        first.max(-half)..last.min(half)
    };
    for row in tiles(clip_y, clip_height, height / 2.0, half_rows) {
        for column in tiles(clip_x, clip_width, width / 2.0, half_columns) {
            if tile_is_filled(row, column) {
                context.rect(
                    width / 2.0 + column as f32 * TILE,
                    height / 2.0 + row as f32 * TILE,
                    TILE,
                    TILE,
                );
            }
        }
    }
    let [red, green, blue] = alternate;
    context.set_fill_style(CanvasColor::rgb(red, green, blue));
    context.fill();
}

/// The tiles of the checkerboard of an area as an image with one pixel per tile, for a platform
/// that enlarges it `TILE` times without smoothing on the GPU instead of drawing each tile. The
/// image is centered in the area, like the tiles of `draw_checkerboard`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckerTiles {
    /// The number of tile columns, as many on each side of the center.
    pub columns: u32,
    /// The number of tile rows, as many on each side of the center.
    pub rows: u32,
    dark: bool,
}

impl CheckerTiles {
    /// The width and height of a single tile.
    pub const TILE: f32 = TILE;

    /// The tiles covering an area, in light or dark colors.
    pub fn new(width: f32, height: f32, dark: bool) -> Self {
        let (half_columns, half_rows) = checker_grid(width, height);
        Self {
            columns: 2 * half_columns as u32,
            rows: 2 * half_rows as u32,
            dark,
        }
    }

    /// One opaque BGRA pixel for each tile, row by row from the top.
    pub fn pixels(&self) -> Vec<u8> {
        let (base, alternate) = checker_colors(self.dark);
        let (half_columns, half_rows) = ((self.columns / 2) as i32, (self.rows / 2) as i32);
        (-half_rows..half_rows)
            .flat_map(|row| (-half_columns..half_columns).map(move |column| (row, column)))
            .flat_map(|(row, column)| {
                let [red, green, blue] = if tile_is_filled(row, column) {
                    alternate
                } else {
                    base
                };
                [blue, green, red, 255]
            })
            .collect()
    }
}

// Returns the base and alternate tile colors of the light or dark checkerboard.
const fn checker_colors(dark: bool) -> ([u8; 3], [u8; 3]) {
    if dark {
        ([41, 41, 43], [59, 59, 64])
    } else {
        ([240, 240, 240], [219, 219, 219])
    }
}

// Returns the number of tiles on each side of the center that cover the area.
fn checker_grid(width: f32, height: f32) -> (i32, i32) {
    (
        (width / (2.0 * TILE)).ceil().max(0.0) as i32,
        (height / (2.0 * TILE)).ceil().max(0.0) as i32,
    )
}

// Returns whether the tile at the given center-relative indices gets the alternate color.
const fn tile_is_filled(row: i32, column: i32) -> bool {
    (row + column).rem_euclid(2) == 0
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{Frame, LoopCount, Raster, decode_media};

    #[test]
    fn enlarged_pixels_are_squares_from_twice_their_size() {
        assert!(!is_pixelated(16.0, 16));
        assert!(!is_pixelated(31.0, 16));
        assert!(is_pixelated(32.0, 16));
        // An image at actual size on a Retina display is drawn sharp.
        assert!(is_pixelated(800.0 * 2.0, 800));
    }

    #[test]
    fn enlarged_bitmaps_are_drawn_without_smoothing() {
        let media = Media::Raster(Raster {
            width: 2,
            height: 1,
            frames: vec![Frame {
                pixels: Arc::from([0, 0, 0, 255, 255, 255, 255, 255]),
                delay: std::time::Duration::ZERO,
            }],
            loop_count: LoopCount::Finite(1),
        });
        let pixels = render(media, 8, 4, false);
        for row in pixels.chunks(8 * 4) {
            let values: Vec<u8> = row.chunks(4).map(|pixel| pixel[0]).collect();
            assert_eq!(values, [0, 0, 0, 0, 255, 255, 255, 255]);
        }
    }

    // Renders media fitted into a canvas.
    fn render(media: Media, width: u32, height: u32, white_background: bool) -> Vec<u8> {
        let renderer = MediaRenderer::new(media);
        let mut canvas = OffscreenCanvas::new(width, height);
        canvas.draw(|context| {
            if white_background {
                context.set_fill_style(CanvasColor::rgb(255, 255, 255));
                context.fill_rect(0.0, 0.0, width as f32, height as f32);
            }
            renderer.draw_fitted(context, 0, (0.0, 0.0, width as f32, height as f32));
        });
        canvas.pixels().to_vec()
    }

    const SIZES: [f32; 6] = [1.0, 23.0, 24.0, 25.0, 48.0, 137.0];

    #[test]
    fn checker_grid_covers_the_area_from_the_center() {
        for size in SIZES {
            let (half_columns, half_rows) = checker_grid(size, 70.0);
            assert!(half_columns as f32 * TILE >= size / 2.0);
            assert!(half_rows as f32 * TILE >= 35.0);
        }
    }

    #[test]
    fn tile_colors_do_not_flip_while_resizing() {
        for size in SIZES {
            let (half_columns, half_rows) = checker_grid(size, size);
            // Tile 0 is always next to the center, so center colors do not depend on the size.
            assert!((-half_columns..half_columns).contains(&0));
            assert!((-half_rows..half_rows).contains(&0));
            assert!(tile_is_filled(0, 0));
            assert!(!tile_is_filled(0, -1));
            assert!(tile_is_filled(-1, -1));
        }
    }

    #[test]
    fn checkerboard_alternates_tiles_from_the_center() {
        let mut canvas = OffscreenCanvas::new(48, 48);
        canvas.draw(|context| draw_checkerboard(context, 48.0, 48.0, false));
        let pixels = canvas.pixels();
        let red = |x: usize, y: usize| pixels[(y * 48 + x) * 4 + 2];
        // The tile right below the center has the alternate color, its neighbors the base one.
        assert_eq!(red(30, 30), 219);
        assert_eq!(red(18, 30), 240);
        assert_eq!(red(30, 18), 240);
        assert_eq!(red(18, 18), 219);
        // A clipped part draws the same tiles there.
        let mut clipped = OffscreenCanvas::new(48, 48);
        clipped.draw(|context| {
            draw_checkerboard_clipped(context, (48.0, 48.0), (24.0, 12.0, 24.0, 24.0), false);
        });
        let clipped = clipped.pixels();
        for (x, y) in [(30, 18), (30, 30), (42, 30)] {
            assert_eq!(clipped[(y * 48 + x) * 4 + 2], red(x, y));
        }
        assert_eq!(clipped[(6 * 48 + 6) * 4 + 3], 0);
        // The tile image has a pixel for each of the same tiles.
        let tiles = CheckerTiles::new(48.0, 48.0, false);
        assert_eq!((tiles.columns, tiles.rows), (4, 4));
        let tile_pixels = tiles.pixels();
        for (x, y) in [(30, 30), (18, 30), (30, 18), (18, 18)] {
            let tile = y / TILE as usize * 4 + x / TILE as usize;
            assert_eq!(
                &tile_pixels[tile * 4..][..4],
                [red(x, y), red(x, y), red(x, y), 255]
            );
        }
    }

    #[test]
    fn thumbnails_fit_the_size_without_enlarging() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="200">
            <rect width="400" height="200" fill="red"/></svg>"#;
        let thumbnail = render_thumbnail(decode_media(svg).expect("SVG"), 100).expect("thumbnail");
        assert_eq!((thumbnail.width(), thumbnail.height()), (100, 50));
        assert_eq!(&thumbnail.data()[..4], [255, 0, 0, 255]);
        let small = render_thumbnail(decode_media(svg).expect("SVG"), 1000).expect("thumbnail");
        assert_eq!((small.width(), small.height()), (400, 200));
    }

    #[test]
    fn vectors_render_at_any_scale() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="200">
            <rect width="400" height="200" fill="red"/></svg>"#;
        let large = render_media(decode_media(svg).expect("SVG"), 1.5).expect("render");
        assert_eq!((large.width(), large.height()), (600, 300));
        assert_eq!(&large.data()[..4], [255, 0, 0, 255]);
        assert!(render_media(decode_media(svg).expect("SVG"), 0.0).is_none());
    }

    #[test]
    fn rasters_draw_the_requested_frame_fitted_and_centered() {
        let frame = |pixel: [u8; 4]| Frame {
            pixels: Arc::from(pixel),
            delay: std::time::Duration::from_millis(100),
        };
        let renderer = MediaRenderer::new(Media::Raster(Raster {
            width: 1,
            height: 1,
            frames: vec![frame([0, 0, 255, 255]), frame([255, 0, 0, 255])],
            loop_count: LoopCount::Infinite,
        }));
        let mut canvas = OffscreenCanvas::new(4, 2);
        canvas.draw(|context| renderer.draw_fitted(context, 1, (0.0, 0.0, 4.0, 2.0)));
        let pixels = canvas.pixels();
        assert_eq!(pixels[3], 0);
        assert_eq!(&pixels[(4 + 1) * 4..][..4], [0, 0, 255, 255]);
    }
}
