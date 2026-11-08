/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::Cell;
use std::time::Instant;

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{define_class, msg_send, sel};

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
        let ivars = self.ivars();
        // SAFETY: The view is flipped and drawn in AppKit's draw pass.
        unsafe {
            super::draw_view(self, |canvas| {
                let (width, height) = (canvas.width(), canvas.height());
                ivars
                    .renderer
                    .draw_fitted(canvas, ivars.frame.get(), (0.0, 0.0, width, height));
            });
        }
    }

    // Shows the current animation frame and schedules the next one while in a window.
    fn advance_frame(&self) {
        // SAFETY: Every NSView answers its window, which may be nil.
        let window: *mut Object = unsafe { msg_send![self, window] };
        let next = if window.is_null() {
            None
        } else {
            let ivars = self.ivars();
            let (frame, next) = ivars.renderer.media().frame_at(ivars.start.get().elapsed());
            if ivars.frame.replace(frame) != frame {
                // SAFETY: The view is a live NSView.
                unsafe {
                    let _: () = msg_send![self, setNeedsDisplay: Bool::YES];
                }
            }
            next
        };
        super::perform_after(self, sel!(advanceFrame), next);
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
