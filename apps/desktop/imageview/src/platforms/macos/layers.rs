/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The Core Animation layers that show the viewport, which the GPU composes and scales: the
//! checkerboard, the media and the crop rectangle or selection above it. Bitmaps are shown as
//! they are, vectors are drawn into a bitmap again at the magnification they settle at.

use std::cell::RefCell;
use std::ffi::c_void;
use std::ptr::{NonNull, null, null_mut};
use std::sync::Arc;

use bcanvas::{CanvasRenderingContext2d, NativeContext, OffscreenCanvas};
use imageview_core::macos::{NS_VIEW_HEIGHT_SIZABLE, NS_VIEW_WIDTH_SIZABLE, Point, Rect, Size};
use imageview_core::{CheckerTiles, Media, Raster};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, define_class, msg_send};

use super::headers::*;
use super::view::rect;
use crate::viewport::Viewport;

// The longest side in device pixels of the bitmap a vector is drawn into, a larger vector is
// only drawn where it is visible.
const MAX_BITMAP: f64 = 8192.0;

define_class!(
    // Hosts the layers, leaving all events to the view below it.
    #[unsafe(super(NSView))]
    #[name = "ImageViewLayerHost"]
    struct Host;

    impl Host {
        #[unsafe(method(hitTest:))]
        const fn _hit_test(&self, _: Point) -> *mut Object {
            null_mut()
        }
    }
);

/// A view hosting the layers that show the viewport.
pub(super) struct Layers {
    host: Retained<Object>,
    checkerboard: Retained<Object>,
    media: Retained<Object>,
    marks: Retained<Object>,
    state: RefCell<State>,
}

#[derive(Default)]
struct State {
    tiles: Option<CheckerTiles>,
    // The images of the frames of the shown bitmap, made for the pixels of its first frame.
    frames: Vec<Option<Image>>,
    first_frame: Option<Arc<[u8]>>,
    vector: Option<Drawing>,
}

// A part of a vector drawn into a bitmap.
struct Drawing {
    vector: Arc<OffscreenCanvas>,
    image: Image,
    // The drawn part in units of the media, and the device pixels per unit.
    part: [f64; 4],
    resolution: f64,
}

impl Layers {
    /// Creates the host view and its layers, with a top left origin.
    pub(super) fn new(frame: Rect) -> Self {
        // SAFETY: All objects are valid instances, the host view retains its root layer, which
        // retains its sublayers.
        unsafe {
            let root: Retained<Object> = msg_send![class!(CALayer), new];
            let _: () = msg_send![&*root, setGeometryFlipped: Bool::YES];
            let [checkerboard, media, marks] = [(); 3].map(|()| {
                let layer: Retained<Object> = msg_send![class!(CALayer), new];
                let _: () = msg_send![&*root, addSublayer: &*layer];
                layer
            });
            let _: () = msg_send![&*checkerboard, setMagnificationFilter: kCAFilterNearest];
            let _: () = msg_send![&*media, setMinificationFilter: kCAFilterTrilinear];
            let host: Allocated<Host> = msg_send![Host::class(), alloc];
            let host: Retained<Object> = msg_send![host, initWithFrame: frame];
            let _: () = msg_send![&*host,
                setAutoresizingMask: NS_VIEW_WIDTH_SIZABLE | NS_VIEW_HEIGHT_SIZABLE
            ];
            // Setting the layer before wanting one makes the view host it.
            let _: () = msg_send![&*host, setLayer: &*root];
            let _: () = msg_send![&*host, setWantsLayer: Bool::YES];
            Self {
                host,
                checkerboard,
                media,
                marks,
                state: RefCell::default(),
            }
        }
    }

    /// The view hosting the layers.
    pub(super) fn host(&self) -> &Object {
        &self.host
    }

    /// Shows the viewport in an area of a size at a display scale. A vector is drawn again
    /// when it `settled` at another magnification or scroll position, until then its bitmap is
    /// scaled along.
    pub(super) fn show(
        &self,
        viewport: &Viewport,
        size: Size,
        scale: f64,
        dark: bool,
        settled: bool,
    ) {
        let mut state = self.state.borrow_mut();
        let tiles = CheckerTiles::new(size.width as f32, size.height as f32, dark);
        let checkerboard = (state.tiles != Some(tiles)).then(|| {
            state.tiles = Some(tiles);
            Image::from_pixels(tiles.columns, tiles.rows, Arc::from(tiles.pixels()))
        });
        let tile = f64::from(CheckerTiles::TILE);
        let (columns, rows) = (f64::from(tiles.columns), f64::from(tiles.rows));
        let checkerboard_frame = rect(
            (size.width - columns * tile) / 2.0,
            (size.height - rows * tile) / 2.0,
            columns * tile,
            rows * tile,
        );

        let (x, y, width, height) = viewport.media_rect();
        let (renderer, frame) = viewport.drawn();
        let (image, media_frame) = match renderer.media() {
            Media::Raster(raster) => {
                state.vector = None;
                (state.frame(raster, frame), rect(x, y, width, height))
            }
            Media::Vector(vector) => {
                let stale = state
                    .vector
                    .as_ref()
                    .is_none_or(|drawing| !Arc::ptr_eq(&drawing.vector, vector));
                state.frames.clear();
                state.first_frame = None;
                if stale || settled {
                    state.draw(vector, viewport, size, scale, stale);
                }
                // The drawn part is scaled along until it is drawn again.
                let magnification = width / f64::from(vector.logical_width());
                state
                    .vector
                    .as_ref()
                    .map_or((None, rect(x, y, width, height)), |drawing| {
                        let [left, top, part_width, part_height] =
                            drawing.part.map(|value| value * magnification);
                        (
                            Some(&drawing.image),
                            rect(x + left, y + top, part_width, part_height),
                        )
                    })
            }
        };
        let pixelated = viewport.has_pixels(scale as f32);
        let marks = viewport
            .has_marks()
            .then(|| {
                Image::draw((size.width, size.height), scale, |context| {
                    viewport.paint_marks(context);
                })
            })
            .flatten();

        // SAFETY: The layers are live and only changed on the main thread, inside a transaction
        // without implicit animations.
        unsafe {
            let _: () = msg_send![class!(CATransaction), begin];
            let _: () = msg_send![class!(CATransaction), setDisableActions: Bool::YES];
            for layer in [&self.checkerboard, &self.media, &self.marks] {
                let _: () = msg_send![&**layer, setContentsScale: scale];
            }
            if let Some(checkerboard) = checkerboard {
                let _: () = msg_send![&*self.checkerboard,
                    setContents: Image::object(checkerboard.as_ref())
                ];
            }
            let _: () = msg_send![&*self.checkerboard, setFrame: checkerboard_frame];
            let _: () = msg_send![&*self.media, setContents: Image::object(image)];
            let _: () = msg_send![&*self.media, setFrame: media_frame];
            let filter = if pixelated {
                kCAFilterNearest
            } else {
                kCAFilterLinear
            };
            let _: () = msg_send![&*self.media, setMagnificationFilter: filter];
            let _: () = msg_send![&*self.marks, setContents: Image::object(marks.as_ref())];
            let _: () = msg_send![&*self.marks,
                setFrame: rect(0.0, 0.0, size.width, size.height)
            ];
            let _: () = msg_send![class!(CATransaction), commit];
        }
    }
}

impl State {
    // The image of a frame of a bitmap, made once.
    fn frame(&mut self, raster: &Raster, frame: usize) -> Option<&Image> {
        let first = &raster.frames.first()?.pixels;
        if !self
            .first_frame
            .as_ref()
            .is_some_and(|key| Arc::ptr_eq(key, first))
        {
            self.first_frame = Some(first.clone());
            self.frames.clear();
        }
        self.frames.resize_with(raster.frames.len(), || None);
        let image = self.frames.get_mut(frame)?;
        if image.is_none() {
            *image = Image::from_pixels(
                raster.width,
                raster.height,
                raster.frames[frame].pixels.clone(),
            );
        }
        image.as_ref()
    }

    // Draws a vector at the magnification and display scale it is shown at, all of it or the
    // visible part of one too large for a bitmap, when it is new or another part is shown.
    fn draw(
        &mut self,
        vector: &Arc<OffscreenCanvas>,
        viewport: &Viewport,
        size: Size,
        scale: f64,
        stale: bool,
    ) {
        let (x, y, width, height) = viewport.media_rect();
        let magnification = width / f64::from(vector.logical_width());
        let [left, top, right, bottom] = if width.max(height) * scale <= MAX_BITMAP {
            [x, y, x + width, y + height]
        } else {
            [
                x.max(0.0),
                y.max(0.0),
                (x + width).min(size.width),
                (y + height).min(size.height),
            ]
        };
        let (part_width, part_height) = (right - left, bottom - top);
        let part = [
            (left - x) / magnification,
            (top - y) / magnification,
            part_width / magnification,
            part_height / magnification,
        ];
        let resolution = (magnification * scale).min(MAX_BITMAP / part[2].max(part[3]));
        let drawn = self
            .vector
            .as_ref()
            .is_some_and(|drawing| drawing.part == part && drawing.resolution == resolution);
        if !stale && drawn {
            return;
        }
        self.vector = Image::draw(
            (part_width, part_height),
            resolution / magnification,
            |context| viewport.paint_media(context, (left, top)),
        )
        .map(|image| Drawing {
            vector: vector.clone(),
            image,
            part,
            resolution,
        });
    }
}

// An owned Core Graphics image, which layers show as their contents.
struct Image(NonNull<c_void>);

impl Image {
    // Makes an image of premultiplied BGRA pixels, which it shares.
    fn from_pixels(width: u32, height: u32, pixels: Arc<[u8]>) -> Option<Self> {
        unsafe extern "C" fn release(info: *mut c_void, _: *const c_void, _: usize) {
            // SAFETY: The data provider passes the box it was created with, once.
            drop(unsafe { Box::from_raw(info.cast::<Arc<[u8]>>()) });
        }
        if width == 0 || height == 0 {
            return None;
        }
        let (data, length) = (pixels.as_ptr(), pixels.len());
        // SAFETY: The data provider owns the pixels until it releases them, the image retains
        // the provider and the color space.
        unsafe {
            let provider = CGDataProviderCreateWithData(
                Box::into_raw(Box::new(pixels)).cast(),
                data.cast(),
                length,
                release,
            );
            let space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
            let image = CGImageCreate(
                width as usize,
                height as usize,
                8,
                32,
                width as usize * 4,
                space,
                CG_BITMAP_PREMULTIPLIED_BGRA,
                provider,
                null(),
                true,
                0,
            );
            CGColorSpaceRelease(space);
            CGDataProviderRelease(provider);
            NonNull::new(image).map(Self)
        }
    }

    // Draws with a canvas of a size in points into a new image of its device pixels.
    fn draw(
        (width, height): (f64, f64),
        scale: f64,
        draw: impl for<'frame> FnOnce(&mut CanvasRenderingContext2d<'frame>),
    ) -> Option<Self> {
        let (pixel_width, pixel_height) = ((width * scale).ceil(), (height * scale).ceil());
        if !(pixel_width >= 1.0 && pixel_height >= 1.0) {
            return None;
        }
        // SAFETY: The bitmap context is live until it is released, after its image is copied.
        unsafe {
            let space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
            let context = CGBitmapContextCreate(
                null_mut(),
                pixel_width as usize,
                pixel_height as usize,
                8,
                0,
                space,
                CG_BITMAP_PREMULTIPLIED_BGRA,
            );
            CGColorSpaceRelease(space);
            if context.is_null() {
                return None;
            }
            CGContextClearRect(context, rect(0.0, 0.0, pixel_width, pixel_height));
            // Canvases have a top left origin.
            CGContextTranslateCTM(context, 0.0, pixel_height);
            CGContextScaleCTM(context, scale, -scale);
            bcanvas::draw_native(
                NativeContext::CoreGraphics(context),
                width as f32,
                height as f32,
                scale as f32,
                draw,
            );
            let image = CGBitmapContextCreateImage(context);
            CGContextRelease(context);
            NonNull::new(image).map(Self)
        }
    }

    // The image as the object of layer contents, or nil.
    fn object(image: Option<&Self>) -> *mut Object {
        image.map_or(null_mut(), |image| image.0.as_ptr().cast())
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        // SAFETY: The image owns its reference.
        unsafe { CGImageRelease(self.0.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use imageview_core::{Frame, LoopCount, decode_media};
    use objc2::rc::autoreleasepool;

    use super::*;

    const SIZE: Size = Size {
        width: 400.0,
        height: 300.0,
    };

    // The contents image of a layer and its pixel width, with the frame of the layer.
    fn shown(layer: &Object) -> (usize, Rect) {
        // SAFETY: The layer is live and its contents is nil or an image.
        unsafe {
            let contents: *mut Object = msg_send![layer, contents];
            assert!(!contents.is_null(), "layer shows nothing");
            (CGImageGetWidth(contents.cast()), msg_send![layer, frame])
        }
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGImageGetWidth(image: *mut c_void) -> usize;
        fn CGBitmapContextGetData(context: *mut c_void) -> *mut u8;
    }

    // Renders the layers of an area of a size into BGRA pixels, row by row from the top.
    fn render(layers: &Layers, (width, height): (usize, usize)) -> Vec<u8> {
        // SAFETY: The bitmap context is live until it is released after copying its pixels.
        unsafe {
            let space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
            let context = CGBitmapContextCreate(
                null_mut(),
                width,
                height,
                8,
                width * 4,
                space,
                CG_BITMAP_PREMULTIPLIED_BGRA,
            );
            CGColorSpaceRelease(space);
            let root: *mut Object = msg_send![layers.host(), layer];
            let _: () = msg_send![root, renderInContext: context];
            let pixels =
                std::slice::from_raw_parts(CGBitmapContextGetData(context), width * height * 4)
                    .to_vec();
            CGContextRelease(context);
            pixels
        }
    }

    #[test]
    fn layers_show_the_media_upright_above_the_centered_checkerboard() {
        autoreleasepool(|_| {
            // A red pixel above a blue one, enlarged to fill the area.
            let media = Media::Raster(Raster {
                width: 1,
                height: 2,
                frames: vec![Frame {
                    pixels: Arc::from([0, 0, 255, 255, 255, 0, 0, 255]),
                    delay: Duration::ZERO,
                }],
                loop_count: LoopCount::Finite(1),
            });
            let size = Size {
                width: 20.0,
                height: 40.0,
            };
            let layers = Layers::new(rect(0.0, 0.0, size.width, size.height));
            layers.show(
                &Viewport::new(Some(media), (20.0, 40.0)),
                size,
                1.0,
                false,
                false,
            );
            let pixels = render(&layers, (20, 40));
            assert_eq!(pixels[(5 * 20 + 10) * 4..][..4], [0, 0, 255, 255]);
            assert_eq!(pixels[(35 * 20 + 10) * 4..][..4], [255, 0, 0, 255]);

            // The tile right below the middle has the alternate color, like the drawn one.
            let size = Size {
                width: 48.0,
                height: 48.0,
            };
            let layers = Layers::new(rect(0.0, 0.0, size.width, size.height));
            layers.show(&Viewport::new(None, (48.0, 48.0)), size, 1.0, false, false);
            let pixels = render(&layers, (48, 48));
            let red = |x: usize, y: usize| pixels[(y * 48 + x) * 4 + 2];
            assert_eq!([red(30, 30), red(18, 30), red(30, 18)], [219, 240, 240]);
        });
    }

    #[test]
    fn layers_show_bitmaps_vectors_and_the_checkerboard() {
        autoreleasepool(|_| {
            let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/examples/ball.gif"))
                .expect("read example");
            // The 100x100 media is shown at 3x, 50 points from the left.
            let viewport = Viewport::new(Some(decode_media(&bytes).expect("GIF")), (400.0, 300.0));
            let layers = Layers::new(rect(0.0, 0.0, SIZE.width, SIZE.height));
            layers.show(&viewport, SIZE, 1.0, false, false);
            let (width, frame) = shown(&layers.media);
            assert_eq!(width, 100);
            assert_eq!(
                (frame.origin.x, frame.origin.y, frame.size.width),
                (50.0, 0.0, 300.0)
            );
            // One pixel for each tile, centered.
            let (width, frame) = shown(&layers.checkerboard);
            assert_eq!(width, 34);
            assert_eq!(frame.origin.x, -4.0);

            // Vectors are drawn at the size they are shown at.
            let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="30"/>"#;
            let viewport = Viewport::new(Some(decode_media(svg).expect("SVG")), (400.0, 300.0));
            layers.show(&viewport, SIZE, 2.0, false, false);
            let (width, frame) = shown(&layers.media);
            assert_eq!((width, frame.size.width), (800, 400.0));
        });
    }
}
