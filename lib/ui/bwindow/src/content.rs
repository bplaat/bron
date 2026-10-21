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

type ResizeHandler = Rc<dyn Fn(u32, u32)>;
type CloseHandler = Box<dyn FnOnce()>;

pub(crate) struct ContentHost {
    id: WindowId,
    handle: Cell<Option<NativeWindowHandle>>,
    attached: Cell<bool>,
    closed: Cell<bool>,
    background: Cell<Option<u32>>,
    theme: Cell<Option<crate::Theme>>,
    allow_file_drop: bool,
    resize: RefCell<Option<ResizeHandler>>,
    close: RefCell<Option<CloseHandler>>,
    redraw: RefCell<Option<Rc<dyn Fn()>>>,
    animation_frame: RefCell<Option<Rc<dyn Fn()>>>,
    animation_frame_pending: Cell<bool>,
}

impl ContentHost {
    pub(crate) fn new(id: WindowId, background: Option<u32>, allow_file_drop: bool) -> Rc<Self> {
        Rc::new(Self {
            id,
            handle: Cell::new(None),
            attached: Cell::new(false),
            closed: Cell::new(false),
            background: Cell::new(background),
            theme: Cell::new(None),
            allow_file_drop,
            resize: RefCell::new(None),
            close: RefCell::new(None),
            redraw: RefCell::new(None),
            animation_frame: RefCell::new(None),
            animation_frame_pending: Cell::new(false),
        })
    }

    pub(crate) fn set_handle(&self, handle: NativeWindowHandle) {
        self.handle.set(Some(handle));
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

    /// Whether a content backend draws the window.
    #[cfg(target_os = "android")]
    pub(crate) fn draws_content(&self) -> bool {
        self.redraw.borrow().is_some()
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

    pub(crate) fn close(&self) {
        if self.closed.replace(true) {
            return;
        }
        self.handle.set(None);
        let redraw = self.redraw.borrow_mut().take();
        drop(redraw);
        let animation_frame = self.animation_frame.borrow_mut().take();
        drop(animation_frame);
        self.animation_frame_pending.set(false);
        let resize = self.resize.borrow_mut().take();
        let close = self.close.borrow_mut().take();
        drop(resize);
        if let Some(close) = close {
            close();
        }
    }

    pub(crate) fn request_redraw(&self) {
        if self.closed.get() {
            return;
        }
        let redraw = self.redraw.borrow().clone();
        if let Some(redraw) = redraw {
            redraw();
        }
    }

    pub(crate) fn request_animation_frame(&self) {
        if self.closed.get() || self.animation_frame_pending.replace(true) {
            return;
        }
        let animation_frame = self.animation_frame.borrow().clone();
        if let Some(animation_frame) = animation_frame {
            animation_frame();
        } else {
            self.animation_frame_pending.set(false);
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

    /// Access the native window when initializing or updating a content backend.
    ///
    /// # Safety
    /// Use the handle only on the main thread while this attachment is open.
    /// Do not destroy the native window or replace its delegate/user data.
    pub unsafe fn native_handle(&self) -> Option<NativeWindowHandle> {
        self.0.handle.get()
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

    /// Register teardown to run once when the owning window closes.
    pub fn on_close(&self, handler: impl FnOnce() + 'static) {
        assert!(!self.is_closed(), "window is closed");
        let previous = self.0.close.borrow_mut().replace(Box::new(handler));
        drop(previous);
    }

    /// Register the content backend's coalescing native repaint scheduler.
    pub fn on_redraw(&self, handler: impl Fn() + 'static) {
        assert!(!self.is_closed(), "window is closed");
        let previous = self.0.redraw.borrow_mut().replace(Rc::new(handler));
        drop(previous);
    }

    /// Register the content backend's display-synchronized frame scheduler.
    pub fn on_animation_frame(&self, handler: impl Fn() + 'static) {
        assert!(!self.is_closed(), "window is closed");
        let previous = self
            .0
            .animation_frame
            .borrow_mut()
            .replace(Rc::new(handler));
        drop(previous);
    }

    /// Create a weak sender for native content input and file-drop callbacks.
    pub fn event_sender(&self) -> WindowEventSender {
        self.0.event_sender()
    }

    /// Disconnect callbacks before releasing the native content resources.
    pub fn disconnect(&self) {
        let redraw = self.0.redraw.borrow_mut().take();
        drop(redraw);
        let animation_frame = self.0.animation_frame.borrow_mut().take();
        drop(animation_frame);
        self.0.animation_frame_pending.set(false);
        let resize = self.0.resize.borrow_mut().take();
        let close = self.0.close.borrow_mut().take();
        drop(resize);
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
    /// Queue a redraw through the native Windows message loop.
    #[cfg(windows)]
    pub fn post_redraw(&self) -> bool {
        let Some(host) = self.0.upgrade() else {
            return false;
        };
        let Some(NativeWindowHandle::Win32(window)) = host.handle.get() else {
            return false;
        };
        !host.closed.get() && crate::platforms::windows::post_content_redraw(window)
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
            let animation_frame_pending = host.animation_frame_pending.replace(false);
            let delivered = crate::dispatch::try_paint(
                crate::Event::Window(host.id, WindowEvent::RedrawRequested),
                finish,
            );
            if !delivered && animation_frame_pending {
                host.animation_frame_pending.set(true);
            }
            return delivered;
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
                    _ => attachment.on_animation_frame(|| {}),
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
                    attachment.on_animation_frame(move || {
                        let _keep_alive = &guard;
                    });
                    attachment.on_animation_frame(|| {});
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
    fn animation_frame_requests_coalesce_until_redraw_delivery() {
        let host = ContentHost::new(WindowId::new(), None, false);
        let attachment = host.attach().expect("attachment");
        let count = Rc::new(Cell::new(0));
        let output = count.clone();
        attachment.on_animation_frame(move || output.set(output.get() + 1));

        host.request_animation_frame();
        host.request_animation_frame();
        assert_eq!(count.get(), 1);

        host.animation_frame_pending.set(false);
        host.request_animation_frame();
        assert_eq!(count.get(), 2);
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
