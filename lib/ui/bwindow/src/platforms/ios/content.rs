/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The shared content view of a window: the root UIView of its view controller, with a layer
//! view for each window layer that renderers add their layers to. It delivers native multitouch events with compatibility mouse input.
//! Window children like a web view receive their own touches, so the view only reports
//! touches that start on it and never delivers them twice.

use objc2::rc::{Allocated, Retained};
use objc2::runtime::AnyObject as Object;
use objc2::{class, define_class, msg_send};

use super::headers::{CGPoint, CGRect, CGSize};
use super::input::ios_touches;
use crate::{LogicalRect, NativeContentHandle, TouchEvent, WindowEvent, WindowEventSender};

// UIViewAutoresizingFlexibleWidth and UIViewAutoresizingFlexibleHeight.
const FLEXIBLE_SIZE: u64 = (1 << 1) | (1 << 4);

define_class!(
    #[unsafe(super(UIView))]
    #[name = "BwindowContentView"]
    #[ivars = WindowEventSender]
    pub(super) struct ContentView;

    impl ContentView {
        #[unsafe(method(touchesBegan:withEvent:))]
        fn _touches_began(&self, touches: *mut Object, _: *mut Object) {
            self.touch(touches, WindowEvent::TouchStart);
        }

        #[unsafe(method(touchesMoved:withEvent:))]
        fn _touches_moved(&self, touches: *mut Object, _: *mut Object) {
            self.touch(touches, WindowEvent::TouchMove);
        }

        #[unsafe(method(touchesEnded:withEvent:))]
        fn _touches_ended(&self, touches: *mut Object, _: *mut Object) {
            self.touch(touches, WindowEvent::TouchEnd);
        }

        #[unsafe(method(touchesCancelled:withEvent:))]
        fn _touches_cancelled(&self, touches: *mut Object, _: *mut Object) {
            self.touch(touches, WindowEvent::TouchCancel);
        }
    }
);

impl ContentView {
    pub(super) fn new(sender: WindowEventSender) -> Retained<Self> {
        let view: Allocated<Self> = unsafe { msg_send![Self::class(), alloc] };
        let view: Retained<Self> = unsafe { msg_send![super(view.set_ivars(sender)), init] };
        let _: () = unsafe { msg_send![&*view, setMultipleTouchEnabled:true] };
        view
    }

    fn touch(&self, touches: *mut Object, event: fn(TouchEvent) -> WindowEvent) {
        let view = self as *const Self as *mut Object;
        let changed_touches = unsafe { ios_touches(view, touches) };
        if !changed_touches.is_empty() {
            self.ivars().send(event(TouchEvent {
                changed_touches,
                ..TouchEvent::default()
            }));
        }
    }
}

fn view(handle: NativeContentHandle) -> *mut Object {
    let NativeContentHandle::UIKit(view) = handle else {
        panic!("expected a UIKit view");
    };
    view.cast()
}

/// Add a layer view to the content view, above or below a neighboring layer view, or else
/// below every other child like a web view. It leaves all touches to the content view. The
/// returned view is owned by the caller.
pub(crate) unsafe fn add_layer_view(
    content: NativeContentHandle,
    neighbor: Option<(NativeContentHandle, bool)>,
) -> NativeContentHandle {
    let content = view(content);
    unsafe {
        let bounds: CGRect = msg_send![content, bounds];
        let layer_view: Allocated<Object> = msg_send![class!(UIView), alloc];
        let layer_view: Retained<Object> = msg_send![layer_view, initWithFrame:bounds];
        let _: () = msg_send![&*layer_view, setUserInteractionEnabled:false];
        match neighbor {
            Some((neighbor, true)) => {
                let _: () =
                    msg_send![content, insertSubview:&*layer_view, aboveSubview:view(neighbor)];
            }
            Some((neighbor, false)) => {
                let _: () =
                    msg_send![content, insertSubview:&*layer_view, belowSubview:view(neighbor)];
            }
            None => {
                let _: () = msg_send![content, insertSubview:&*layer_view, atIndex:0usize];
            }
        }
        NativeContentHandle::UIKit(Retained::into_raw(layer_view).cast())
    }
}

/// Whether layers get native views, which every window composites.
pub(crate) const unsafe fn native_layers(_: NativeContentHandle) -> bool {
    true
}

/// Place a layer view at an area of the content view, or fill it.
pub(crate) unsafe fn set_layer_view_frame(
    _: NativeContentHandle,
    layer_view: NativeContentHandle,
    frame: Option<LogicalRect>,
) {
    let layer_view = view(layer_view);
    unsafe {
        let (frame, mask) = match frame {
            Some(frame) => (
                CGRect {
                    origin: CGPoint {
                        x: frame.x.into(),
                        y: frame.y.into(),
                    },
                    size: CGSize {
                        width: frame.width.into(),
                        height: frame.height.into(),
                    },
                },
                0,
            ),
            None => {
                let superview: *mut Object = msg_send![layer_view, superview];
                (msg_send![superview, bounds], FLEXIBLE_SIZE)
            }
        };
        let _: () = msg_send![layer_view, setAutoresizingMask:mask];
        let _: () = msg_send![layer_view, setFrame:frame];
    }
}

pub(crate) unsafe fn set_layer_view_hidden(layer_view: NativeContentHandle, hidden: bool) {
    let _: () = unsafe { msg_send![view(layer_view), setHidden:hidden] };
}

/// Remove a layer view with the layers of its renderer and release it.
pub(crate) unsafe fn remove_layer_view(layer_view: NativeContentHandle) {
    let layer_view = view(layer_view);
    unsafe {
        let _: () = msg_send![layer_view, removeFromSuperview];
        drop(Retained::from_raw(layer_view));
    }
}
