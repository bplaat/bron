/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The native scroller of a window: a UIScrollView over the content and its layers that
//! scrolls nothing but its offset, as large as the content. Its pan gesture recognizer is on
//! the content view, so swipes scroll with the iOS momentum and bounce and show the scroll
//! indicators, while taps still reach the content.

use std::cell::Cell;

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{define_class, msg_send};

use super::headers::{CGPoint, CGRect, CGSize};
use crate::{LogicalPoint, WindowEvent, WindowEventSender};

// UIViewAutoresizingFlexibleWidth and UIViewAutoresizingFlexibleHeight.
const FLEXIBLE_SIZE: u64 = (1 << 1) | (1 << 4);
// UIScrollViewContentInsetAdjustmentNever, offsets start at the top left of the content.
const INSET_ADJUSTMENT_NEVER: i64 = 2;

pub(super) struct ScrollerIvars {
    sender: WindowEventSender,
    offset: Cell<(f64, f64)>,
}

define_class!(
    #[unsafe(super(UIScrollView))]
    #[name = "BwindowScroller"]
    #[ivars = ScrollerIvars]
    pub(super) struct Scroller;

    impl Scroller {
        // UIKit lays out a scroll view every time its offset changes.
        #[unsafe(method(layoutSubviews))]
        fn _layout_subviews(&self) {
            unsafe {
                let _: () = msg_send![super(self), layoutSubviews];
            }
            let offset = self.offset();
            if self.ivars().offset.replace((offset.x, offset.y)) != (offset.x, offset.y) {
                let offset = LogicalPoint::new(offset.x as f32, offset.y as f32);
                self.ivars().sender.send(WindowEvent::Scroll(offset));
            }
        }
    }
);

impl Scroller {
    /// Add a scroller over the content view and its layers.
    pub(super) fn new(content: *mut Object, sender: WindowEventSender) -> Retained<Self> {
        unsafe {
            let bounds: CGRect = msg_send![content, bounds];
            let scroller: Allocated<Self> = msg_send![Self::class(), alloc];
            let scroller: Retained<Self> = msg_send![
                super(scroller.set_ivars(ScrollerIvars {
                    sender,
                    offset: Cell::new((0.0, 0.0)),
                })),
                initWithFrame: bounds
            ];
            let _: () = msg_send![&*scroller, setAutoresizingMask: FLEXIBLE_SIZE];
            let _: () =
                msg_send![&*scroller, setContentInsetAdjustmentBehavior: INSET_ADJUSTMENT_NEVER];
            // Touches pass through to the content view, which drives the pan gesture.
            let _: () = msg_send![&*scroller, setUserInteractionEnabled: Bool::NO];
            let pan: *mut Object = msg_send![&*scroller, panGestureRecognizer];
            let _: () = msg_send![content, addGestureRecognizer: pan];
            let _: () = msg_send![content, addSubview: &*scroller];
            scroller
        }
    }

    /// The offset from the top left of the content, outside it while bouncing.
    pub(super) fn offset(&self) -> CGPoint {
        unsafe { msg_send![self, contentOffset] }
    }

    pub(super) fn set_size(&self, size: CGSize) {
        unsafe {
            let _: () = msg_send![self, setContentSize: size];
        }
    }

    /// Scroll to an offset within the content.
    pub(super) fn scroll_to(&self, offset: CGPoint) {
        unsafe {
            let bounds: CGRect = msg_send![self, bounds];
            let size: CGSize = msg_send![self, contentSize];
            let point = CGPoint {
                x: offset
                    .x
                    .clamp(0.0, (size.width - bounds.size.width).max(0.0)),
                y: offset
                    .y
                    .clamp(0.0, (size.height - bounds.size.height).max(0.0)),
            };
            let _: () = msg_send![self, setContentOffset: point, animated: Bool::NO];
        }
    }

    /// Remove the scroller and its gesture from the content view.
    pub(super) fn remove(&self) {
        unsafe {
            let pan: *mut Object = msg_send![self, panGestureRecognizer];
            let content: *mut Object = msg_send![self, superview];
            let _: () = msg_send![content, removeGestureRecognizer: pan];
            let _: () = msg_send![self, removeFromSuperview];
        }
    }
}
