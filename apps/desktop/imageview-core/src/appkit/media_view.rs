/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::null_mut;
use std::time::Instant;

use bcanvas::NativeContext;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, define_class, msg_send, sel};

use super::Rect;
use crate::{Media, MediaRenderer};

struct MediaViewIvars {
    renderer: MediaRenderer,
    // When the animation started playing.
    start: Cell<Instant>,
    frame: Cell<usize>,
}

define_class!(
    // Draws media fitted into its bounds and plays its animation while it is in a window.
    #[unsafe(super(NSView))]
    #[name = "ImageViewMediaView"]
    #[ivars = MediaViewIvars]
    struct MediaView;

    impl MediaView {
        #[unsafe(method(isFlipped))]
        const fn _is_flipped(&self) -> Bool {
            Bool::YES
        }

        #[unsafe(method(drawRect:))]
        fn _draw_rect(&self, _: Rect) {
            self.draw();
        }

        #[unsafe(method(viewDidMoveToWindow))]
        fn _view_did_move_to_window(&self) {
            self.ivars().start.set(Instant::now());
            self.ivars().frame.set(0);
            self.advance_frame();
        }

        #[unsafe(method(advanceFrame))]
        fn _advance_frame(&self) {
            self.advance_frame();
        }
    }
);

impl MediaView {
    fn draw(&self) {
        // SAFETY: Drawing occurs during AppKit's draw pass, whose flipped graphics context
        // stays valid for this call.
        unsafe {
            let graphics_context: *mut Object =
                msg_send![class!(NSGraphicsContext), currentContext];
            if graphics_context.is_null() {
                return;
            }
            let context: *mut c_void = msg_send![graphics_context, CGContext];
            if context.is_null() {
                return;
            }
            let bounds: Rect = msg_send![self, bounds];
            let window: *mut Object = msg_send![self, window];
            let scale: f64 = if window.is_null() {
                1.0
            } else {
                msg_send![window, backingScaleFactor]
            };
            let (width, height) = (bounds.size.width as f32, bounds.size.height as f32);
            let ivars = self.ivars();
            bcanvas::draw_native(
                NativeContext::CoreGraphics(context),
                width,
                height,
                scale as f32,
                |canvas| {
                    ivars.renderer.draw_fitted(
                        canvas,
                        ivars.frame.get(),
                        (0.0, 0.0, width, height),
                    );
                },
            );
        }
    }

    // Shows the current animation frame and schedules the next one while in a window.
    fn advance_frame(&self) {
        // SAFETY: Scheduling retains the view until the selector runs, and canceling only
        // removes this view's own pending requests.
        unsafe {
            let _: () = msg_send![class!(NSObject),
                cancelPreviousPerformRequestsWithTarget: self,
                selector: sel!(advanceFrame),
                object: null_mut::<Object>()
            ];
            let window: *mut Object = msg_send![self, window];
            if window.is_null() {
                return;
            }
            let ivars = self.ivars();
            let (frame, next) = ivars.renderer.media().frame_at(ivars.start.get().elapsed());
            if ivars.frame.replace(frame) != frame {
                let _: () = msg_send![self, setNeedsDisplay: Bool::YES];
            }
            if let Some(next) = next {
                let _: () = msg_send![self,
                    performSelector: sel!(advanceFrame),
                    withObject: null_mut::<Object>(),
                    afterDelay: next.as_secs_f64()
                ];
            }
        }
    }
}

/// Creates an owned main-thread view that draws media fitted into its bounds and plays its
/// animation.
pub fn create_media_view(frame: Rect, media: Media) -> Retained<Object> {
    // SAFETY: MediaView is a registered NSView subclass.
    let view: Allocated<MediaView> = unsafe { msg_send![MediaView::class(), alloc] };
    let view = view.set_ivars(MediaViewIvars {
        renderer: MediaRenderer::new(media),
        start: Cell::new(Instant::now()),
        frame: Cell::new(0),
    });
    // SAFETY: MediaView is a registered NSView subclass with initialized ivars.
    let view: Retained<MediaView> = unsafe { msg_send![super(view), initWithFrame: frame] };
    Retained::into_any(view)
}
