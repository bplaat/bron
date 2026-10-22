/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::rc::{Rc, Weak};

use crate::{WindowEvent, WindowId};

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
        })
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

    /// Report a display scale change of the window content.
    pub(crate) fn scale_factor_changed(&self, scale_factor: f32) {
        if self.closed.get() || self.scale_factor.replace(scale_factor) == scale_factor {
            return;
        }
        let handler = self.scale.borrow().clone();
        if let Some(handler) = handler {
            handler(scale_factor);
        }
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
            None => self.event_sender().send(WindowEvent::RedrawRequested),
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
    /// redraws are delivered directly as `RedrawRequested`.
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
            let event = crate::Event::Window(host.id, event);
            if matches!(
                &event,
                crate::Event::Window(_, WindowEvent::RedrawRequested)
            ) {
                let weak = Rc::downgrade(&host);
                crate::dispatch::send_before(event, move || {
                    if let Some(host) = weak.upgrade() {
                        host.animation_frame_pending.set(false);
                    }
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
        host.scale_factor_changed(2.0);
        host.scale_factor_changed(2.0);
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
