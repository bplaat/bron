/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The native scroller of a window: a transparent NSScrollView over the content and its layers
//! that scrolls an empty document as large as the content. It shows the system overlay
//! scrollers and gives the wheel and trackpad their momentum and edge bounce, while every other
//! hit passes through to the content view below, which forwards its wheel events here.

use std::cell::Cell;
use std::ptr::null_mut;

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, define_class, msg_send, sel};

use super::headers::*;
use crate::{LogicalPoint, WindowEvent, WindowEventSender};

const NS_VIEW_WIDTH_SIZABLE: u64 = 2;
const NS_VIEW_HEIGHT_SIZABLE: u64 = 16;

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {
    static NSViewBoundsDidChangeNotification: *mut Object;
    static NSViewFrameDidChangeNotification: *mut Object;
}

pub(super) struct ScrollerIvars {
    sender: WindowEventSender,
    size: Cell<NSSize>,
    offset: Cell<(f64, f64)>,
}

define_class!(
    #[unsafe(super(NSScrollView))]
    #[name = "BwindowScroller"]
    #[ivars = ScrollerIvars]
    pub(super) struct Scroller;

    impl Scroller {
        // Only the scroller takes hits, the content view below gets the others.
        #[unsafe(method(hitTest:))]
        fn _hit_test(&self, point: NSPoint) -> *mut Object {
            unsafe {
                let hit: *mut Object = msg_send![super(self), hitTest: point];
                let clip: *mut Object = msg_send![self, contentView];
                let document: *mut Object = msg_send![self, documentView];
                if hit == clip || hit == document { null_mut() } else { hit }
            }
        }

        // The clip view scrolled or resized.
        #[unsafe(method(clipChanged:))]
        fn _clip_changed(&self, _: *mut Object) {
            self.update_document();
            let offset = self.offset();
            let point = (offset.x, offset.y);
            if self.ivars().offset.replace(point) != point {
                let offset = LogicalPoint::new(offset.x as f32, offset.y as f32);
                self.ivars().sender.send(WindowEvent::Scroll(offset));
            }
        }
    }
);

define_class!(
    // The empty document, flipped so it scrolls from the top.
    #[unsafe(super(NSView))]
    #[name = "BwindowScrollDocument"]
    struct Document;

    impl Document {
        #[unsafe(method(isFlipped))]
        const fn _is_flipped(&self) -> Bool { Bool::YES }
    }
);

impl Scroller {
    /// Add a scroller over the content view and its layers.
    pub(super) fn new(content: *mut Object, sender: WindowEventSender) -> Retained<Self> {
        unsafe {
            let bounds: NSRect = msg_send![content, bounds];
            let scroller: Allocated<Self> = msg_send![Self::class(), alloc];
            let scroller: Retained<Self> = msg_send![
                super(scroller.set_ivars(ScrollerIvars {
                    sender,
                    size: Cell::new(NSSize { width: 0.0, height: 0.0 }),
                    offset: Cell::new((0.0, 0.0)),
                })),
                initWithFrame: bounds
            ];
            let _: () = msg_send![&*scroller, setAutoresizingMask: NS_VIEW_WIDTH_SIZABLE | NS_VIEW_HEIGHT_SIZABLE];
            let _: () = msg_send![&*scroller, setDrawsBackground: Bool::NO];
            let _: () = msg_send![&*scroller, setHasVerticalScroller: Bool::YES];
            let _: () = msg_send![&*scroller, setHasHorizontalScroller: Bool::YES];
            let _: () = msg_send![&*scroller, setAutohidesScrollers: Bool::YES];
            let document: Allocated<Document> = msg_send![Document::class(), alloc];
            let document: Retained<Document> = msg_send![document, initWithFrame: bounds];
            let _: () = msg_send![&*scroller, setDocumentView: &*document];
            let clip: *mut Object = msg_send![&*scroller, contentView];
            let _: () = msg_send![clip, setDrawsBackground: Bool::NO];
            let _: () = msg_send![clip, setPostsBoundsChangedNotifications: Bool::YES];
            let _: () = msg_send![clip, setPostsFrameChangedNotifications: Bool::YES];
            let center: *mut Object = msg_send![class!(NSNotificationCenter), defaultCenter];
            for name in [
                NSViewBoundsDidChangeNotification,
                NSViewFrameDidChangeNotification,
            ] {
                let _: () = msg_send![center, addObserver: &*scroller,
                    selector: sel!(clipChanged:), name: name, object: clip];
            }
            let _: () = msg_send![content, addSubview: &*scroller];
            scroller
        }
    }

    fn clip(&self) -> *mut Object {
        unsafe { msg_send![self, contentView] }
    }

    fn clip_bounds(&self) -> NSRect {
        unsafe { msg_send![self.clip(), bounds] }
    }

    /// The offset from the top left of the content, outside it while bouncing.
    pub(super) fn offset(&self) -> NSPoint {
        self.clip_bounds().origin
    }

    pub(super) fn set_size(&self, size: NSSize) {
        self.ivars().size.set(size);
        self.update_document();
    }

    /// Scroll to an offset within the content.
    pub(super) fn scroll_to(&self, offset: NSPoint) {
        let (bounds, size) = (self.clip_bounds(), self.ivars().size.get());
        let point = NSPoint {
            x: offset
                .x
                .clamp(0.0, (size.width - bounds.size.width).max(0.0)),
            y: offset
                .y
                .clamp(0.0, (size.height - bounds.size.height).max(0.0)),
        };
        unsafe {
            let _: () = msg_send![self.clip(), scrollToPoint: point];
            let _: () = msg_send![self, reflectScrolledClipView: self.clip()];
        }
    }

    // Size the document to the content, at least as large as the visible area.
    fn update_document(&self) {
        let (bounds, content) = (self.clip_bounds(), self.ivars().size.get());
        let size = NSSize {
            width: content.width.max(bounds.size.width),
            height: content.height.max(bounds.size.height),
        };
        unsafe {
            let document: *mut Object = msg_send![self, documentView];
            let frame: NSRect = msg_send![document, frame];
            if frame.size.width != size.width || frame.size.height != size.height {
                let _: () = msg_send![document, setFrameSize: size];
            }
        }
    }

    /// Remove the scroller from the content view.
    pub(super) fn remove(&self) {
        unsafe {
            let center: *mut Object = msg_send![class!(NSNotificationCenter), defaultCenter];
            let _: () = msg_send![center, removeObserver: self];
            let _: () = msg_send![self, removeFromSuperview];
        }
    }
}
