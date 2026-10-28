/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Quick Look thumbnail extension for the image formats ImageView displays.

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::Mutex;

use bcanvas::Color;
use block2::{Block, RcBlock};
use imageview_core::MediaRenderer;
use imageview_core::appkit::{
    Point, Size, dispatch_async, draw_bottom_up, extension_main, load_media, make_error, ns_string,
};
use objc2::rc::Retained;
use objc2::runtime::AnyObject as Object;
use objc2::{class, define_class, msg_send};

mod headers;

define_class!(
    #[unsafe(super(QLThumbnailProvider))]
    #[name = "ImageViewThumbnailProvider"]
    struct ThumbnailProvider;

    impl ThumbnailProvider {
        #[unsafe(method(provideThumbnailForFileRequest:completionHandler:))]
        fn _provide_thumbnail(
            &self,
            request: *mut Object,
            completion: &Block<dyn Fn(*mut Object, *mut Object)>,
        ) {
            provide_thumbnail(request, completion);
        }
    }
);

struct ThumbnailCompletion(RcBlock<dyn Fn(*mut Object, *mut Object)>);

struct SendableUrl(Retained<Object>);

// SAFETY: NSURL is immutable and this wrapper only carries it to the worker that reads it.
unsafe impl Send for SendableUrl {}

impl SendableUrl {
    const fn as_ptr(&self) -> *mut Object {
        self.0.as_ptr()
    }
}

// SAFETY: Quick Look completion blocks are escaping blocks intended for asynchronous use. This
// wrapper moves its owned copy to a GCD worker and invokes it there exactly once.
unsafe impl Send for ThumbnailCompletion {}

impl ThumbnailCompletion {
    fn call(&self, reply: *mut Object, error: *mut Object) {
        self.0.call((reply, error));
    }
}

fn provide_thumbnail(request: *mut Object, completion: &Block<dyn Fn(*mut Object, *mut Object)>) {
    // SAFETY: Quick Look supplies a valid request with a file URL and size constraints.
    let (url, minimum_size, maximum_size, scale): (*mut Object, Size, Size, f64) = unsafe {
        (
            msg_send![request, fileURL],
            msg_send![request, minimumSize],
            msg_send![request, maximumSize],
            msg_send![request, scale],
        )
    };
    // SAFETY: Keep the Quick Look URL alive after this callback returns.
    let (url, completion) = unsafe {
        let url = SendableUrl(Retained::retain(url).expect("cannot retain a null URL"));
        let completion = ThumbnailCompletion(completion.copy());
        (url, completion)
    };
    dispatch_async(move || {
        // SAFETY: url remains retained throughout the synchronous load.
        let media = unsafe { load_media(url.as_ptr()) };
        let media = match media {
            Ok(media) => media,
            Err(description) => {
                // SAFETY: Foundation error creation is safe on this worker queue.
                let error = unsafe {
                    make_error(ns_string!("nl.bplaat.ImageView.Thumbnail"), &description)
                };
                completion.call(null_mut(), error);
                return;
            }
        };

        let (width, height) = media.size();
        let media_size = Size { width, height };
        let context_size = thumbnail_context_size(media_size, minimum_size, maximum_size);
        let drawing_size = scaled(context_size, scale);
        let fitted_size = scaled(aspect_fit(media_size, context_size), scale);
        let fitted_origin = Point {
            x: (drawing_size.width - fitted_size.width) / 2.0,
            y: (drawing_size.height - fitted_size.height) / 2.0,
        };
        let media = Mutex::new(media);
        let drawing = RcBlock::new_ret::<*mut c_void, bool>(move |context| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let media = media.lock().unwrap_or_else(|error| error.into_inner());
                let renderer = MediaRenderer::new(media.clone());
                // SAFETY: Quick Look owns the drawing context for this call. The copied drawing
                // block owns the media and drawing is synchronous within the block invocation.
                unsafe {
                    draw_bottom_up(context, drawing_size, |canvas| {
                        canvas.set_fill_style(Color::rgb(255, 255, 255));
                        canvas.fill_rect(
                            0.0,
                            0.0,
                            drawing_size.width as f32,
                            drawing_size.height as f32,
                        );
                        renderer.draw(
                            canvas,
                            0,
                            (
                                fitted_origin.x as f32,
                                fitted_origin.y as f32,
                                fitted_size.width as f32,
                                fitted_size.height as f32,
                            ),
                        );
                    });
                }
                true
            }))
            .unwrap_or(false)
        });

        // SAFETY: QLThumbnailReply copies the drawing block for deferred rendering.
        let reply: *mut Object = unsafe {
            msg_send![class!(QLThumbnailReply),
                replyWithContextSize: context_size,
                drawingBlock: &*drawing
            ]
        };
        completion.call(reply, null_mut());
    });
}

// Returns a Quick Look context size clamped to its accepted range.
fn thumbnail_context_size(image: Size, minimum: Size, maximum: Size) -> Size {
    let fitted = aspect_fit(image, maximum);
    Size {
        width: fitted.width.max(minimum.width).min(maximum.width),
        height: fitted.height.max(minimum.height).min(maximum.height),
    }
}

// Returns `image` fitted inside `bounds`, or `bounds` when the image has no size.
fn aspect_fit(image: Size, bounds: Size) -> Size {
    if image.width <= 0.0 || image.height <= 0.0 {
        return bounds;
    }
    let scale = (bounds.width / image.width).min(bounds.height / image.height);
    Size {
        width: image.width * scale,
        height: image.height * scale,
    }
}

fn scaled(size: Size, scale: f64) -> Size {
    Size {
        width: size.width * scale,
        height: size.height * scale,
    }
}

fn main() {
    let _ = ThumbnailProvider::class();
    extension_main();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnail_context_uses_the_image_aspect_ratio() {
        let size = thumbnail_context_size(
            Size {
                width: 400.0,
                height: 200.0,
            },
            Size {
                width: 0.0,
                height: 0.0,
            },
            Size {
                width: 100.0,
                height: 100.0,
            },
        );
        assert_eq!(size.width, 100.0);
        assert_eq!(size.height, 50.0);
    }

    #[test]
    fn thumbnail_context_honors_the_minimum_without_exceeding_the_maximum() {
        let size = thumbnail_context_size(
            Size {
                width: 1000.0,
                height: 10.0,
            },
            Size {
                width: 32.0,
                height: 32.0,
            },
            Size {
                width: 256.0,
                height: 256.0,
            },
        );
        assert_eq!(size.width, 256.0);
        assert_eq!(size.height, 32.0);
    }

    #[test]
    fn media_without_a_size_keeps_the_requested_bounds() {
        let bounds = Size {
            width: 128.0,
            height: 96.0,
        };
        let size = aspect_fit(
            Size {
                width: 0.0,
                height: 0.0,
            },
            bounds,
        );
        assert_eq!(size.width, bounds.width);
        assert_eq!(size.height, bounds.height);
    }
}
