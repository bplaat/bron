/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::rc::{Rc, Weak};

use crate::{LogicalPoint, WindowEvent, WindowId};

/// A native top-level window for implementing a content backend.
#[derive(Clone, Copy, Debug)]
pub enum NativeWindowHandle {
    /// An AppKit NSWindow pointer.
    AppKit(*mut c_void),
    /// A UIKit UIView pointer that content is added to.
    UIKit(*mut c_void),
    /// A Win32 HWND.
    Win32(*mut c_void),
    /// A GTK 3 GtkWindow pointer.
    Gtk(*mut c_void),
    /// A JNI global reference to the `android.app.NativeActivity`.
    Android(*mut c_void),
}

/// The native content host that renderers draw into. The window owns it, delivers its
/// keyboard, pointer, scroll, focus, cursor and file-drop interaction, and lays it out
/// to fill the content area.
#[derive(Clone, Copy, Debug)]
pub enum NativeContentHandle {
    /// The shared bwindow NSView filling the window content area.
    AppKit(*mut c_void),
    /// The shared bwindow UIView filling the window.
    UIKit(*mut c_void),
    /// The client area of the top-level HWND.
    Win32(*mut c_void),
    /// The shared GtkDrawingArea filling the GtkWindow.
    Gtk(*mut c_void),
    /// A JNI global reference to the `android.app.NativeActivity`, whose window surface
    /// renderers draw into.
    Android(*mut c_void),
}

/// A cursor shown over window content; native window decorations retain their own cursors.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CursorIcon {
    /// Standard arrow.
    #[default]
    Default,
    /// Pointing hand for an interactive target.
    Pointer,
    /// Background work in progress.
    Progress,
}

type ResizeHandler = Rc<dyn Fn(u32, u32)>;
type ScaleHandler = Rc<dyn Fn(f32)>;
type CloseHandler = Box<dyn FnOnce()>;
type ContentFactory = Box<dyn FnOnce() -> Option<NativeContentHandle>>;

pub(crate) struct ContentHost {
    id: WindowId,
    handle: Cell<Option<NativeWindowHandle>>,
    content: Cell<Option<NativeContentHandle>>,
    content_factory: RefCell<Option<ContentFactory>>,
    scale_factor: Cell<f32>,
    attached: Cell<bool>,
    ready: Cell<bool>,
    ready_handler: RefCell<Option<CloseHandler>>,
    closed: Cell<bool>,
    background: Cell<Option<u32>>,
    theme: Cell<Option<crate::Theme>>,
    allow_file_drop: bool,
    resize: RefCell<Option<ResizeHandler>>,
    scale: RefCell<Option<ScaleHandler>>,
    close: RefCell<Option<CloseHandler>>,
    redraw: RefCell<Option<Rc<dyn Fn()>>>,
    #[cfg(windows)]
    paint: RefCell<Option<Rc<dyn Fn()>>>,
    frames: RefCell<Option<crate::platforms::FrameSource>>,
    animation_frame_pending: Cell<bool>,
    pointer_locked: Cell<bool>,
    pointer_position: Cell<Option<LogicalPoint>>,
    touch_mouse: RefCell<crate::touch::TouchMouse>,
}

impl ContentHost {
    pub(crate) fn new(id: WindowId, background: Option<u32>, allow_file_drop: bool) -> Rc<Self> {
        Rc::new(Self {
            id,
            handle: Cell::new(None),
            content: Cell::new(None),
            content_factory: RefCell::new(None),
            scale_factor: Cell::new(1.0),
            attached: Cell::new(false),
            ready: Cell::new(!cfg!(target_os = "ios")),
            ready_handler: RefCell::new(None),
            closed: Cell::new(false),
            background: Cell::new(background),
            theme: Cell::new(None),
            allow_file_drop,
            resize: RefCell::new(None),
            scale: RefCell::new(None),
            close: RefCell::new(None),
            redraw: RefCell::new(None),
            #[cfg(windows)]
            paint: RefCell::new(None),
            frames: RefCell::new(None),
            animation_frame_pending: Cell::new(false),
            pointer_locked: Cell::new(false),
            pointer_position: Cell::new(None),
            touch_mouse: RefCell::new(crate::touch::TouchMouse::default()),
        })
    }

    #[cfg(any(target_os = "ios", test))]
    pub(crate) fn ready(&self) {
        if self.closed.get() || self.ready.replace(true) {
            return;
        }
        let handler = self.ready_handler.borrow_mut().take();
        if let Some(handler) = handler {
            handler();
        }
    }

    pub(crate) fn set_handle(&self, handle: NativeWindowHandle) {
        self.handle.set(Some(handle));
    }

    /// Set the content host that exists for the whole window lifetime.
    #[cfg(any(target_vendor = "apple", target_os = "android", windows))]
    pub(crate) fn set_content(&self, content: NativeContentHandle) {
        self.content.set(Some(content));
    }

    /// Create the content host on first use, for hosts that alternative window children
    /// like a web view must not get.
    #[cfg_attr(
        any(target_vendor = "apple", target_os = "android", windows),
        allow(dead_code, reason = "only GTK creates its content host on demand")
    )]
    pub(crate) fn set_content_factory(
        &self,
        factory: impl FnOnce() -> Option<NativeContentHandle> + 'static,
    ) {
        *self.content_factory.borrow_mut() = Some(Box::new(factory));
    }

    fn content_handle(&self) -> Option<NativeContentHandle> {
        if self.closed.get() {
            return None;
        }
        if self.content.get().is_none() {
            let factory = self.content_factory.borrow_mut().take();
            if let Some(factory) = factory {
                self.content.set(factory());
            }
        }
        self.content.get()
    }

    pub(crate) const fn id(&self) -> WindowId {
        self.id
    }

    pub(crate) const fn is_closed(&self) -> bool {
        self.closed.get()
    }

    pub(crate) const fn pointer_locked(&self) -> bool {
        self.pointer_locked.get()
    }

    pub(crate) fn set_pointer_locked(self: &Rc<Self>, locked: bool) {
        if self.pointer_locked.replace(locked) == locked {
            return;
        }
        if !locked {
            self.pointer_position.set(None);
        }
        self.event_sender().send(WindowEvent::PointerLockChange);
    }

    pub(crate) fn set_background(&self, color: u32) {
        self.background.set(Some(color));
    }

    /// The configured background color.
    #[cfg(target_os = "android")]
    pub(crate) const fn background(&self) -> Option<u32> {
        self.background.get()
    }

    /// Whether a content backend draws the window, it owns the window surface once attached.
    #[cfg(target_os = "android")]
    pub(crate) const fn draws_content(&self) -> bool {
        self.attached.get()
    }

    pub(crate) fn theme_changed(self: &Rc<Self>, theme: crate::Theme) {
        if !self.closed.get() && self.theme.replace(Some(theme)) != Some(theme) {
            self.event_sender().send(WindowEvent::ThemeChanged(theme));
        }
    }

    pub(crate) fn attach(self: &Rc<Self>) -> Result<WindowAttachment, AttachError> {
        if self.closed.get() {
            return Err(AttachError::Closed);
        }
        if self.attached.replace(true) {
            return Err(AttachError::Occupied);
        }
        Ok(WindowAttachment(self.clone()))
    }

    pub(crate) fn event_sender(self: &Rc<Self>) -> WindowEventSender {
        WindowEventSender(Rc::downgrade(self))
    }

    pub(crate) fn resize(&self, width: u32, height: u32) {
        if self.closed.get() {
            return;
        }
        let handler = self.resize.borrow().clone();
        if let Some(handler) = handler {
            handler(width, height);
        }
    }

    /// Report a display scale change, returning whether the value changed.
    pub(crate) fn scale_factor_changed(&self, scale_factor: f32) -> bool {
        if self.closed.get() || self.scale_factor.replace(scale_factor) == scale_factor {
            return false;
        }
        let handler = self.scale.borrow().clone();
        if let Some(handler) = handler {
            handler(scale_factor);
        }
        true
    }

    /// Run the content's native paint pass, returns false when no content paints.
    #[cfg(windows)]
    pub(crate) fn paint(&self) -> bool {
        if self.closed.get() {
            return false;
        }
        let paint = self.paint.borrow().clone();
        paint.is_some_and(|paint| {
            paint();
            true
        })
    }

    /// Whether content paints the client area itself.
    #[cfg(windows)]
    pub(crate) fn has_paint(&self) -> bool {
        self.paint.borrow().is_some()
    }

    // Take the hooks out before dropping them, their destructors can register hooks again.
    fn disconnect(&self) -> Option<CloseHandler> {
        let ready = self.ready_handler.borrow_mut().take();
        drop(ready);
        let redraw = self.redraw.borrow_mut().take();
        drop(redraw);
        #[cfg(windows)]
        {
            let paint = self.paint.borrow_mut().take();
            drop(paint);
        }
        self.animation_frame_pending.set(false);
        let resize = self.resize.borrow_mut().take();
        drop(resize);
        let scale = self.scale.borrow_mut().take();
        drop(scale);
        self.close.borrow_mut().take()
    }

    pub(crate) fn close(&self) {
        if self.closed.replace(true) {
            return;
        }
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        if self.pointer_locked.replace(false) {
            crate::platforms::release_pointer_lock();
        }
        self.pointer_position.set(None);
        self.handle.set(None);
        self.content.set(None);
        let factory = self.content_factory.borrow_mut().take();
        drop(factory);
        let frames = self.frames.borrow_mut().take();
        drop(frames);
        if let Some(close) = self.disconnect() {
            close();
        }
    }

    /// Repaint through the content's repaint scheduler, or deliver the redraw directly.
    pub(crate) fn request_redraw(self: &Rc<Self>) {
        if self.closed.get() {
            return;
        }
        let redraw = self.redraw.borrow().clone();
        match redraw {
            Some(redraw) => redraw(),
            None => {
                #[cfg(windows)]
                if let Some(NativeWindowHandle::Win32(hwnd)) = self.handle.get() {
                    // Posted animation messages precede hardware input. Render through
                    // WM_PAINT so a slow continuous renderer cannot starve that input.
                    use crate::platforms::windows::headers::{FALSE, InvalidateRect};
                    unsafe { InvalidateRect(hwnd, std::ptr::null_mut(), FALSE) };
                    return;
                }
                self.event_sender().send(WindowEvent::RedrawRequested);
            }
        }
    }

    /// Redraw at the next display frame of the window. The window paces the frames,
    /// so content only needs a repaint scheduler.
    pub(crate) fn request_animation_frame(self: &Rc<Self>) {
        if self.closed.get() || self.animation_frame_pending.replace(true) {
            return;
        }
        let mut frames = self.frames.borrow_mut();
        if frames.is_none() {
            *frames = self
                .handle
                .get()
                .and_then(|handle| crate::platforms::FrameSource::new(handle, Rc::downgrade(self)));
        }
        match &*frames {
            Some(frames) => frames.request(),
            None => self.animation_frame_pending.set(false),
        }
    }

    /// Deliver a requested animation frame at a display frame, any redraw delivered
    /// before already satisfied it.
    pub(crate) fn animation_frame(self: &Rc<Self>) {
        if !self.closed.get() && self.animation_frame_pending.replace(false) {
            self.request_redraw();
        }
    }

    /// Fill in the pointer movement since the previous event. While locked the position
    /// stays put and native relative motion is passed through.
    fn track_pointer(&self, event: WindowEvent) -> WindowEvent {
        match event {
            WindowEvent::MouseMove {
                mut position,
                movement,
                modifiers,
            } => {
                let previous = self.pointer_position.get();
                let movement = if self.pointer_locked.get() {
                    if let Some(previous) = previous {
                        position = previous;
                    }
                    movement
                } else {
                    previous.map_or(LogicalPoint::default(), |previous| {
                        LogicalPoint::new(position.x - previous.x, position.y - previous.y)
                    })
                };
                self.pointer_position.set(Some(position));
                WindowEvent::MouseMove {
                    position,
                    movement,
                    modifiers,
                }
            }
            WindowEvent::MouseDown(ref mouse) | WindowEvent::MouseUp(ref mouse) => {
                self.pointer_position.set(Some(mouse.position));
                event
            }
            WindowEvent::MouseLeave | WindowEvent::Blur => {
                self.pointer_position.set(None);
                event
            }
            _ => event,
        }
    }
}

/// A window cannot accept the requested content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachError {
    /// The native window has closed.
    Closed,
    /// This window has already had content attached.
    Occupied,
}

impl std::fmt::Display for AttachError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Closed => "window is closed",
            Self::Occupied => "window already has content",
        })
    }
}

impl std::error::Error for AttachError {}

/// An exclusive content attachment whose hooks disconnect when it or its window is dropped.
pub struct WindowAttachment(Rc<ContentHost>);

impl WindowAttachment {
    /// The owning window's identifier.
    pub fn window_id(&self) -> WindowId {
        self.0.id
    }

    /// Whether the native window has closed.
    pub fn is_closed(&self) -> bool {
        self.0.closed.get()
    }

    /// Access the native top-level window, for content that adds its own window child
    /// like a web view.
    ///
    /// # Safety
    /// Use the handle only on the main thread while this attachment is open.
    /// Do not destroy the native window or replace its delegate/user data.
    pub unsafe fn window_handle(&self) -> Option<NativeWindowHandle> {
        self.0.handle.get()
    }

    /// Access the native content host that renderers draw into, creating it on first use.
    /// The window delivers its interaction, so renderers only draw and present.
    ///
    /// # Safety
    /// Use the handle only on the main thread while this attachment is open.
    /// Do not destroy the host, replace its delegate/user data, or handle its input.
    pub unsafe fn content_handle(&self) -> Option<NativeContentHandle> {
        self.0.content_handle()
    }

    /// Native pixels per logical pixel of the window content.
    pub fn scale_factor(&self) -> f32 {
        self.0.scale_factor.get()
    }

    /// The window's configured backing color.
    pub fn background_color(&self) -> Option<u32> {
        self.0.background.get()
    }

    /// Whether the window accepts file drops.
    pub fn allow_file_drop(&self) -> bool {
        self.0.allow_file_drop
    }

    /// Whether native content can be created for this window.
    pub fn is_ready(&self) -> bool {
        self.0.ready.get() && !self.is_closed()
    }

    /// Run once when native content can be created, immediately if already ready.
    /// Pending callbacks are canceled when the attachment disconnects.
    pub fn on_ready(&self, handler: impl FnOnce() + 'static) {
        assert!(!self.is_closed(), "window is closed");
        if self.0.ready.get() {
            handler();
        } else {
            let previous = self.0.ready_handler.borrow_mut().replace(Box::new(handler));
            drop(previous);
        }
    }

    /// Register a callback for native client-area size changes in physical pixels.
    pub fn on_resize(&self, handler: impl Fn(u32, u32) + 'static) {
        assert!(!self.is_closed(), "window is closed");
        let previous = self.0.resize.borrow_mut().replace(Rc::new(handler));
        drop(previous);
    }

    /// Register a callback for display scale changes of the window content.
    pub fn on_scale_factor_changed(&self, handler: impl Fn(f32) + 'static) {
        assert!(!self.is_closed(), "window is closed");
        let previous = self.0.scale.borrow_mut().replace(Rc::new(handler));
        drop(previous);
    }

    /// Register teardown to run once when the owning window closes.
    pub fn on_close(&self, handler: impl FnOnce() + 'static) {
        assert!(!self.is_closed(), "window is closed");
        let previous = self.0.close.borrow_mut().replace(Box::new(handler));
        drop(previous);
    }

    /// Register the content backend's coalescing native repaint scheduler, without one
    /// redraws are delivered as `RedrawRequested`. Queued redraws also use this
    /// scheduler if it is registered before delivery, so they cannot bypass a native frame.
    pub fn on_redraw(&self, handler: impl Fn() + 'static) {
        assert!(!self.is_closed(), "window is closed");
        let previous = self.0.redraw.borrow_mut().replace(Rc::new(handler));
        drop(previous);
    }

    /// Register the native paint pass for content drawing into the client area of the
    /// top-level window. It runs for every WM_PAINT after the window validated the
    /// update region, so it must not invalidate the window unless it wants another frame.
    #[cfg(windows)]
    pub fn windows_on_paint(&self, handler: impl Fn() + 'static) {
        assert!(!self.is_closed(), "window is closed");
        let previous = self.0.paint.borrow_mut().replace(Rc::new(handler));
        drop(previous);
    }

    /// Redraw at the next display frame of the window, through the repaint scheduler.
    pub fn request_animation_frame(&self) {
        self.0.request_animation_frame();
    }

    /// Create a weak sender for native content input and file-drop callbacks.
    pub fn event_sender(&self) -> WindowEventSender {
        self.0.event_sender()
    }

    /// Disconnect callbacks before releasing the native content resources.
    pub fn disconnect(&self) {
        let close = self.0.disconnect();
        drop(close);
    }

    /// Get the application's existing Windows data directory.
    #[cfg(windows)]
    pub fn windows_data_directory(&self) -> std::path::PathBuf {
        crate::platforms::windows_config_dir()
    }
}

impl Drop for WindowAttachment {
    fn drop(&mut self) {
        self.disconnect();
    }
}

/// Sends native content events without keeping its window alive.
#[derive(Clone)]
pub struct WindowEventSender(Weak<ContentHost>);

impl WindowEventSender {
    /// Redraw at the next display frame of the window, through the repaint scheduler.
    pub fn request_animation_frame(&self) {
        if let Some(host) = self.0.upgrade() {
            host.request_animation_frame();
        }
    }

    /// Deliver a native paint event synchronously, without queuing a borrowed frame.
    ///
    /// Returns false if the loop is busy, not running, or the window is closed.
    /// In that case the backend must schedule a later repaint. On success,
    /// `finish` ends the native frame before queued input/user events are delivered.
    pub fn redraw(&self, finish: impl FnOnce()) -> bool {
        if let Some(host) = self.0.upgrade()
            && !host.closed.get()
        {
            // A delivered redraw satisfies a pending animation frame, content that could not
            // deliver requests a new one.
            host.animation_frame_pending.set(false);
            return crate::dispatch::try_paint(
                crate::Event::Window(host.id, WindowEvent::RedrawRequested),
                finish,
            );
        }
        false
    }

    /// Deliver an event while the owning window is open; discard late events.
    pub fn send(&self, event: WindowEvent) {
        if let Some(host) = self.0.upgrade()
            && !host.closed.get()
        {
            if matches!(
                &event,
                WindowEvent::TouchStart(_)
                    | WindowEvent::TouchMove(_)
                    | WindowEvent::TouchEnd(_)
                    | WindowEvent::TouchCancel(_)
            ) {
                let touch = match &event {
                    WindowEvent::TouchStart(touch) => WindowEvent::TouchStart(touch.clone()),
                    WindowEvent::TouchMove(touch) => WindowEvent::TouchMove(touch.clone()),
                    WindowEvent::TouchEnd(touch) => WindowEvent::TouchEnd(touch.clone()),
                    WindowEvent::TouchCancel(touch) => WindowEvent::TouchCancel(touch.clone()),
                    _ => unreachable!(),
                };
                let sender = self.clone();
                crate::dispatch::send_then(crate::Event::Window(host.id, event), move || {
                    if let Some(host) = sender.0.upgrade()
                        && !host.closed.get()
                    {
                        let events = host.touch_mouse.borrow_mut().events(&touch);
                        for event in events {
                            sender.send(event);
                        }
                    }
                });
                return;
            }
            let event = host.track_pointer(event);
            let event = crate::Event::Window(host.id, event);
            if matches!(
                &event,
                crate::Event::Window(_, WindowEvent::RedrawRequested)
            ) {
                let weak = Rc::downgrade(&host);
                crate::dispatch::send_if(event, move || {
                    let Some(host) = weak.upgrade().filter(|host| !host.closed.get()) else {
                        return false;
                    };
                    // Content may have attached since this event was queued. Only
                    // its native paint callback can provide a valid borrowed frame.
                    let redraw = host.redraw.borrow().clone();
                    if let Some(redraw) = redraw {
                        redraw();
                        return false;
                    }
                    host.animation_frame_pending.set(false);
                    true
                });
            } else {
                crate::dispatch::send(event);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queued_redraw_uses_the_renderer_attached_before_delivery() {
        let host = ContentHost::new(WindowId::new(), None, false);
        host.request_redraw();
        let attachment = host.attach().expect("attachment");
        let scheduled = Rc::new(Cell::new(0));
        let output = scheduled.clone();
        attachment.on_redraw(move || output.set(output.get() + 1));
        let painted = Rc::new(Cell::new(false));
        let output = painted.clone();
        crate::dispatch::start(move |event| {
            assert!(matches!(
                event,
                crate::Event::Window(_, WindowEvent::RedrawRequested)
            ));
            output.set(true);
        });
        assert_eq!(scheduled.get(), 1);
        assert!(!painted.get(), "queued redraw bypassed native paint");
        assert!(attachment.event_sender().redraw(|| {}));
        assert!(painted.get(), "native paint was not delivered");
        assert_eq!(
            scheduled.get(),
            1,
            "native paint must not reschedule itself"
        );
    }

    #[test]
    fn queued_redraw_is_discarded_after_close() {
        let host = ContentHost::new(WindowId::new(), None, false);
        host.request_redraw();
        host.close();
        crate::dispatch::start(|_| panic!("redraw delivered after close"));
    }

    #[test]
    fn locked_motion_keeps_the_position_even_when_relative_motion_is_zero() {
        let host = ContentHost::new(WindowId::new(), None, false);
        let at = LogicalPoint::new(10.0, 20.0);
        let moved = |position, movement| WindowEvent::MouseMove {
            position,
            movement,
            modifiers: crate::Modifiers::NONE,
        };
        host.track_pointer(moved(at, LogicalPoint::default()));
        host.pointer_locked.set(true);
        for movement in [LogicalPoint::new(3.0, -4.0), LogicalPoint::default()] {
            let WindowEvent::MouseMove {
                position,
                movement: actual,
                ..
            } = host.track_pointer(moved(LogicalPoint::default(), movement))
            else {
                panic!("expected mouse move");
            };
            assert_eq!(position, at);
            assert_eq!(actual, movement);
        }
        host.pointer_locked.set(false);
    }

    #[test]
    fn queued_touch_start_can_cancel_compatibility_mouse_events() {
        let host = ContentHost::new(WindowId::new(), None, false);
        let sender = host.event_sender();
        let calls = Rc::new(Cell::new(0));
        let output = calls.clone();
        // Queue before installing the handler so cancellation must be checked at delivery.
        sender.send(WindowEvent::TouchStart(crate::TouchEvent {
            changed_touches: vec![crate::Touch {
                identifier: 1,
                position: LogicalPoint::default(),
            }],
            ..crate::TouchEvent::default()
        }));
        crate::dispatch::start(move |event| {
            if let crate::Event::Window(_, WindowEvent::TouchStart(touch)) = event {
                touch.prevent_default();
                output.set(output.get() + 1);
            } else {
                panic!("unexpected compatibility mouse event");
            }
        });
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn readiness_runs_once_and_late_registration_runs_immediately() {
        let host = ContentHost::new(WindowId::new(), None, false);
        host.ready.set(false);
        let attachment = host.attach().expect("attachment");
        let calls = Rc::new(Cell::new(0));
        let output = calls.clone();
        attachment.on_ready(move || output.set(output.get() + 1));
        assert_eq!(calls.get(), 0);
        host.ready();
        host.ready();
        assert_eq!(calls.get(), 1);
        assert!(attachment.is_ready());
        let output = calls.clone();
        attachment.on_ready(move || output.set(output.get() + 1));
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn pending_readiness_is_canceled_on_disconnect_or_close() {
        for close in [false, true] {
            let host = ContentHost::new(WindowId::new(), None, false);
            host.ready.set(false);
            let attachment = host.attach().expect("attachment");
            attachment.on_ready(|| panic!("canceled readiness callback"));
            if close {
                host.close();
                assert!(!attachment.is_ready());
            } else {
                drop(attachment);
            }
            host.ready();
        }
    }

    #[test]
    fn replacing_hooks_allows_captured_destructors_to_register_again() {
        struct OnDrop(Box<dyn Fn()>);
        impl Drop for OnDrop {
            fn drop(&mut self) {
                (self.0)();
            }
        }

        for kind in 0..4 {
            let host = ContentHost::new(WindowId::new(), None, false);
            let attachment = Rc::new(host.attach().expect("attachment"));
            let weak = Rc::downgrade(&attachment);
            let dropped = Rc::new(Cell::new(false));
            let output = dropped.clone();
            let guard = OnDrop(Box::new(move || {
                let attachment = weak.upgrade().expect("live attachment");
                match kind {
                    0 => attachment.on_resize(|_, _| {}),
                    1 => attachment.on_close(|| {}),
                    2 => attachment.on_redraw(|| {}),
                    _ => attachment.on_scale_factor_changed(|_| {}),
                }
                output.set(true);
            }));
            match kind {
                0 => {
                    attachment.on_resize(move |_, _| {
                        let _keep_alive = &guard;
                    });
                    attachment.on_resize(|_, _| {});
                }
                1 => {
                    attachment.on_close(move || {
                        let _keep_alive = &guard;
                    });
                    attachment.on_close(|| {});
                }
                2 => {
                    attachment.on_redraw(move || {
                        let _keep_alive = &guard;
                    });
                    attachment.on_redraw(|| {});
                }
                _ => {
                    attachment.on_scale_factor_changed(move |_| {
                        let _keep_alive = &guard;
                    });
                    attachment.on_scale_factor_changed(|_| {});
                }
            }
            assert!(dropped.get());
        }
    }

    #[test]
    fn repaint_hooks_are_disconnected_on_close_and_drop() {
        let host = ContentHost::new(WindowId::new(), None, false);
        let attachment = host.attach().expect("attachment");
        let count = Rc::new(Cell::new(0));
        let output = count.clone();
        attachment.on_redraw(move || output.set(output.get() + 1));
        host.request_redraw();
        assert_eq!(count.get(), 1);
        host.close();
        host.request_redraw();
        assert_eq!(count.get(), 1);

        let host = ContentHost::new(WindowId::new(), None, false);
        let attachment = host.attach().expect("attachment");
        attachment.on_redraw(|| panic!("disconnected"));
        drop(attachment);
        host.request_redraw();
    }

    #[test]
    fn animation_frames_coalesce_into_one_redraw_per_display_frame() {
        let host = ContentHost::new(WindowId::new(), None, false);
        let attachment = host.attach().expect("attachment");
        let count = Rc::new(Cell::new(0));
        let output = count.clone();
        attachment.on_redraw(move || output.set(output.get() + 1));

        // Without a native window there is no frame source, so requests are dropped.
        host.request_animation_frame();
        assert!(!host.animation_frame_pending.get());

        // Display frames redraw once per pending request.
        host.animation_frame_pending.set(true);
        host.animation_frame();
        host.animation_frame();
        assert_eq!(count.get(), 1);

        // A redraw delivered before the display frame satisfies the request.
        host.animation_frame_pending.set(true);
        attachment.event_sender().redraw(|| {});
        host.animation_frame();
        assert_eq!(count.get(), 1);

        host.animation_frame_pending.set(true);
        host.close();
        host.animation_frame();
        assert_eq!(count.get(), 1);
    }

    #[test]
    fn scale_changes_notify_once_per_value() {
        let host = ContentHost::new(WindowId::new(), None, false);
        let attachment = host.attach().expect("attachment");
        let count = Rc::new(Cell::new(0));
        let output = count.clone();
        attachment.on_scale_factor_changed(move |_| output.set(output.get() + 1));
        assert!(host.scale_factor_changed(2.0));
        assert!(!host.scale_factor_changed(2.0));
        assert_eq!(attachment.scale_factor(), 2.0);
        assert_eq!(count.get(), 1);
    }

    #[test]
    fn content_host_is_created_once_on_first_use() {
        let host = ContentHost::new(WindowId::new(), None, false);
        let count = Rc::new(Cell::new(0));
        let output = count.clone();
        host.set_content_factory(move || {
            output.set(output.get() + 1);
            Some(NativeContentHandle::Gtk(std::ptr::null_mut()))
        });
        let attachment = host.attach().expect("attachment");
        assert!(unsafe { attachment.content_handle() }.is_some());
        assert!(unsafe { attachment.content_handle() }.is_some());
        assert_eq!(count.get(), 1);
        host.close();
        assert!(unsafe { attachment.content_handle() }.is_none());
    }

    #[test]
    fn attachment_is_exclusive_even_after_drop() {
        let host = ContentHost::new(WindowId::new(), None, false);
        let attachment = host.attach().expect("first attachment");
        assert!(matches!(host.attach(), Err(AttachError::Occupied)));
        drop(attachment);
        assert!(matches!(host.attach(), Err(AttachError::Occupied)));
    }

    #[test]
    fn closing_invalidates_handles_and_runs_teardown_once() {
        let host = ContentHost::new(WindowId::new(), None, false);
        let attachment = host.attach().expect("first attachment");
        let count = Rc::new(Cell::new(0));
        let output = count.clone();
        attachment.on_close(move || output.set(output.get() + 1));
        host.close();
        host.close();
        assert!(attachment.is_closed());
        assert_eq!(count.get(), 1);
        assert!(matches!(host.attach(), Err(AttachError::Closed)));
    }
}
