/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use bwindow::ffi::*;
use bwindow::{ButtonState, NativeWindowHandle, WindowAttachment, WindowEvent, WindowEventSender};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, define_class, msg_send, sel};

use super::context::{PlatformCanvasContext, TextStyleCache};
use super::headers::*;
use crate::CanvasRenderingContext2d;

const UI_VIEW_CONTENT_MODE_REDRAW: i64 = 3;

#[link(name = "Foundation", kind = "framework")]
unsafe extern "C" {
    static NSRunLoopCommonModes: *mut Object;
}

#[derive(Clone, Copy)]
struct Frame {
    context: CGContextRef,
    width: f32,
    height: f32,
    scale: f32,
}

struct ViewState {
    sender: WindowEventSender,
    frame: Cell<Option<Frame>>,
    closed: Cell<bool>,
    painting: Cell<bool>,
    scheduled: Cell<bool>,
    display_link: RefCell<Option<Retained<Object>>>,
}

type ViewIvars = Rc<ViewState>;

define_class!(
    #[unsafe(super(UIView))]
    #[ivars = ViewIvars]
    struct CanvasView;

    impl CanvasView {
        #[unsafe(method(touchesBegan:withEvent:))]
        fn _touches_began(&self, touches: *mut Object, _: *mut Object) {
            self.send_touch(touches, Some(ButtonState::Pressed));
        }

        #[unsafe(method(touchesMoved:withEvent:))]
        fn _touches_moved(&self, touches: *mut Object, _: *mut Object) {
            self.send_touch(touches, None);
        }

        #[unsafe(method(touchesEnded:withEvent:))]
        fn _touches_ended(&self, touches: *mut Object, _: *mut Object) {
            self.send_touch(touches, Some(ButtonState::Released));
            self.ivars().sender.send(WindowEvent::MouseLeave);
        }

        #[unsafe(method(touchesCancelled:withEvent:))]
        fn _touches_cancelled(&self, touches: *mut Object, _: *mut Object) {
            self.send_touch(touches, Some(ButtonState::Released));
            self.ivars().sender.send(WindowEvent::MouseLeave);
        }

        #[unsafe(method(drawRect:))]
        fn _draw_rect(&self, _: CGRect) { self.paint(); }

        #[unsafe(method(displayFrame:))]
        fn _display_frame(&self, _: *mut Object) {
            // Keep the display link running while frames keep being requested.
            if self.ivars().scheduled.get() {
                self.deliver_animation_frame();
            } else {
                self.set_display_link_paused(true);
            }
        }
    }
);

impl CanvasView {
    fn send_touch(&self, touches: *mut Object, state: Option<ButtonState>) {
        let view = self as *const Self as *mut Object;
        self.ivars()
            .sender
            .send(unsafe { ios_touch_event(view, touches, state) });
    }

    fn deliver_animation_frame(&self) {
        self.ivars().scheduled.set(false);
        if !self.ivars().closed.get() {
            let _: () = unsafe { msg_send![self, setNeedsDisplay] };
        }
    }

    fn request_redraw(&self) {
        let state = self.ivars();
        if state.closed.get() {
            return;
        }
        if !state.painting.get() {
            // A running display link pauses itself on its next callback.
            state.scheduled.set(false);
            let _: () = unsafe { msg_send![self, setNeedsDisplay] };
            return;
        }
        if !state.scheduled.replace(true) {
            self.set_display_link_paused(false);
        }
    }

    fn request_animation_frame(&self) {
        let state = self.ivars();
        if state.closed.get() || state.scheduled.replace(true) {
            return;
        }
        self.set_display_link_paused(false);
    }

    fn set_display_link_paused(&self, paused: bool) {
        if let Some(display_link) = &*self.ivars().display_link.borrow() {
            let paused = if paused { Bool::YES } else { Bool::NO };
            let _: () = unsafe { msg_send![display_link, setPaused:paused] };
        }
    }

    fn close(&self) {
        if self.ivars().closed.replace(true) {
            return;
        }
        self.ivars().frame.set(None);
        self.ivars().scheduled.set(false);
        // The display link retains its target, so invalidate it to release the view.
        if let Some(display_link) = self.ivars().display_link.take() {
            let _: () = unsafe { msg_send![&display_link, invalidate] };
        }
        let _: () = unsafe { msg_send![self, removeFromSuperview] };
    }

    fn paint(&self) {
        if self.ivars().closed.get() {
            return;
        }
        // Application code can drop its canvas or close the parent during paint.
        let retained: Retained<Self> = unsafe { msg_send![self, retain] };
        let state = retained.ivars();
        if state.painting.get() {
            self.request_redraw();
            return;
        }
        unsafe {
            let context = UIGraphicsGetCurrentContext();
            if context.is_null() {
                return;
            }
            let bounds: CGRect = msg_send![self, bounds];
            let scale: f64 = msg_send![self, contentScaleFactor];
            state.painting.set(true);
            state.frame.set(Some(Frame {
                context,
                width: bounds.size.width as f32,
                height: bounds.size.height as f32,
                scale: scale as f32,
            }));
            if !state.sender.redraw(|| state.frame.set(None)) {
                state.frame.set(None);
                self.request_redraw();
            }
            state.painting.set(false);
        }
    }
}

pub(crate) struct PlatformCanvas {
    view: Retained<CanvasView>,
    attachment: WindowAttachment,
    text_styles: TextStyleCache,
}

impl PlatformCanvas {
    pub(crate) fn new(attachment: WindowAttachment) -> Self {
        let Some(NativeWindowHandle::UIKit(content)) = (unsafe { attachment.native_handle() })
        else {
            panic!("expected an open UIKit window");
        };
        unsafe {
            let content = content.cast::<Object>();
            let bounds: CGRect = msg_send![content, bounds];
            let view: Allocated<CanvasView> = msg_send![CanvasView::class(), alloc];
            let view: Retained<CanvasView> = msg_send![
                super(view.set_ivars(Rc::new(ViewState {
                    sender: attachment.event_sender(),
                    frame: Cell::new(None),
                    closed: Cell::new(false),
                    painting: Cell::new(false),
                    scheduled: Cell::new(false),
                    display_link: RefCell::new(None),
                }))),
                initWithFrame:bounds
            ];
            let display_link: Retained<Object> = msg_send![class!(CADisplayLink),
                displayLinkWithTarget:&*view, selector:sel!(displayFrame:)];
            let _: () = msg_send![&display_link, setPaused:Bool::YES];
            let run_loop: *mut Object = msg_send![class!(NSRunLoop), mainRunLoop];
            let _: () =
                msg_send![&display_link, addToRunLoop:run_loop, forMode:NSRunLoopCommonModes];
            view.ivars().display_link.replace(Some(display_link));
            let _: () = msg_send![&*view, setOpaque:Bool::NO];
            let _: () = msg_send![&*view, setContentMode:UI_VIEW_CONTENT_MODE_REDRAW];
            let _: () = msg_send![&*view, setAutoresizingMask:UI_VIEW_AUTORESIZING_FLEXIBLE_WIDTH | UI_VIEW_AUTORESIZING_FLEXIBLE_HEIGHT];
            let _: () = msg_send![content, addSubview:&*view];
            let redraw = view.clone();
            attachment.on_redraw(move || redraw.request_redraw());
            let animation_frame = view.clone();
            attachment.on_animation_frame(move || animation_frame.request_animation_frame());
            let close = view.clone();
            attachment.on_close(move || close.close());
            view.request_redraw();
            Self {
                view,
                attachment,
                text_styles: TextStyleCache::default(),
            }
        }
    }

    pub(crate) fn request_redraw(&self) {
        self.view.request_redraw();
    }

    pub(crate) fn request_animation_frame(&self) {
        self.view.request_animation_frame();
    }

    /// Touch screens have no pointer cursor.
    // Not const so the shared Canvas::set_cursor wrapper stays non-const on every platform.
    #[allow(clippy::missing_const_for_fn)]
    pub(crate) fn set_cursor(&mut self, _cursor: crate::CursorIcon) {}

    pub(crate) fn draw(
        &mut self,
        draw: impl for<'frame> FnOnce(&mut CanvasRenderingContext2d<'frame>),
    ) -> bool {
        if self.attachment.is_closed() {
            return false;
        }
        let Some(frame) = self.view.ivars().frame.take() else {
            return false;
        };
        let mut context = CanvasRenderingContext2d::new(
            unsafe { PlatformCanvasContext::new(frame.context, self.text_styles.clone()) },
            frame.width,
            frame.height,
            frame.scale,
        );
        draw(&mut context);
        true
    }
}

impl Drop for PlatformCanvas {
    fn drop(&mut self) {
        self.attachment.disconnect();
        self.view.close();
    }
}
