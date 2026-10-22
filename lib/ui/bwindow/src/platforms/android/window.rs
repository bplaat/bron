/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::RefCell;
use std::rc::Rc;

use jni::sys::jint;
use jni::{JValue, jni_sig, jni_str};

use super::activity::{
    android_activity, android_with_env, set_dark_mode, set_system_bar_appearance,
};
use super::event_loop::{
    android_density, android_post_local, android_surface, android_window_size, content_size,
    safe_area, send_event, stop, system_theme,
};
use crate::content::ContentHost;
use crate::{
    CloseRequest, Event, LogicalInsets, LogicalPoint, LogicalSize, NativeWindowHandle, Theme,
    WindowBuilder, WindowEvent, WindowId,
};

// An activity hosts exactly one window.
thread_local! {
    #[allow(clippy::missing_const_for_thread_local, reason = "false positive without native TLS")]
    static WINDOW: RefCell<Option<Rc<ContentHost>>> = const { RefCell::new(None) };
}

fn current() -> Option<Rc<ContentHost>> {
    WINDOW.with(|window| window.borrow().clone())
}

fn destroy(host: &ContentHost) {
    host.close();
    let window = WINDOW.with(|window| window.borrow_mut().take());
    drop(window);
    stop();
}

pub(super) fn send(event: WindowEvent) {
    if let Some(host) = current() {
        host.event_sender().send(event);
    }
}

pub(super) fn resized() {
    if let Some(host) = current() {
        let (width, height) = android_window_size();
        host.resize(width, height);
        host.event_sender()
            .send(WindowEvent::Resize(content_size()));
    }
    redraw();
}

pub(super) fn redraw() {
    if let Some(host) = current() {
        if host.draws_content() {
            host.request_redraw();
        } else {
            paint_background(&host);
        }
    }
}

pub(super) fn safe_area_changed() {
    send(WindowEvent::SafeAreaChanged(safe_area()));
}

pub(super) fn configuration_changed(resized: bool, safe_area_changed: bool) {
    if let Some(host) = current() {
        host.theme_changed(system_theme());
        if resized {
            host.scale_factor_changed(android_density());
            host.event_sender()
                .send(WindowEvent::ScaleFactorChanged(android_density()));
        }
    }
    if resized {
        self::resized();
    }
    if safe_area_changed {
        self::safe_area_changed();
    }
}

/// Handle the back gesture of content in another window like the main window, sending
/// CloseRequested, callable from every thread.
pub fn android_back_pressed() {
    super::event_loop::android_post(close_requested);
}

pub(super) fn close_requested() {
    let Some(host) = current() else {
        return;
    };
    let request = CloseRequest::new();
    crate::dispatch::send_then(
        Event::Window(host.id(), WindowEvent::CloseRequested(request.clone())),
        move || {
            if !request.default_prevented() && !host.is_closed() {
                destroy(&host);
            }
        },
    );
}

/// The window background when the app sets no color: white, or black in the dark theme.
pub fn android_default_background() -> u32 {
    match system_theme() {
        Theme::Light => 0xffffff,
        Theme::Dark => 0x000000,
    }
}

// Fill the window surface, with the same hardware canvas producer bcanvas draws with.
fn paint_background(host: &ContentHost) {
    let Some(surface) = android_surface() else {
        return;
    };
    if host.is_closed() {
        return;
    }
    let color = host.background().unwrap_or_else(android_default_background);
    android_with_env(|env| {
        let canvas = env
            .call_method(
                surface.as_obj(),
                jni_str!("lockHardwareCanvas"),
                jni_sig!("()Landroid/graphics/Canvas;"),
                &[],
            )?
            .l()?;
        env.call_method(
            &canvas,
            jni_str!("drawColor"),
            jni_sig!("(I)V"),
            &[JValue::Int((color | 0xff00_0000) as jint)],
        )?
        .v()?;
        env.call_method(
            surface.as_obj(),
            jni_str!("unlockCanvasAndPost"),
            jni_sig!("(Landroid/graphics/Canvas;)V"),
            &[(&canvas).into()],
        )?
        .v()
    });
}

// MARK: Window
pub(crate) struct PlatformWindow {
    id: WindowId,
    host: Rc<ContentHost>,
}

impl PlatformWindow {
    pub(crate) const fn id(&self) -> WindowId {
        self.id
    }

    pub(crate) fn new(builder: &WindowBuilder, host: Rc<ContentHost>) -> Self {
        assert!(current().is_none(), "Android supports only one open window");
        host.set_handle(NativeWindowHandle::Android(
            android_activity().as_raw().cast(),
        ));
        // Renderers draw into the window surface of the activity, which receives the input.
        host.set_content(crate::NativeContentHandle::Android(
            android_activity().as_raw().cast(),
        ));
        host.scale_factor_changed(android_density());
        let mut window = Self {
            id: builder.window_id,
            host: host.clone(),
        };
        if let Some(theme) = builder.theme {
            crate::WindowInterface::set_theme(&mut window, theme);
        }
        if let Some(color) = builder.background_color {
            crate::WindowInterface::set_background_color(&mut window, color);
        }
        WINDOW.with(|window| *window.borrow_mut() = Some(host));
        send_event(Event::Window(builder.window_id, WindowEvent::Create));
        // Report the theme, and the size when the window surface exists already.
        android_post_local(|| {
            let has_surface = android_surface().is_some();
            configuration_changed(has_surface, has_surface);
        });
        window
    }
}

impl crate::WindowInterface for PlatformWindow {
    fn close(&mut self) {
        destroy(&self.host);
    }

    // Phones have no window title, position, size limits, or resizing.
    fn set_title(&mut self, _title: impl AsRef<str>) {}

    fn position(&self) -> LogicalPoint {
        LogicalPoint::new(0.0, 0.0)
    }

    fn size(&self) -> LogicalSize {
        content_size()
    }

    fn safe_area(&self) -> LogicalInsets {
        safe_area()
    }

    fn set_position(&mut self, _point: LogicalPoint) {}

    fn set_size(&mut self, _size: LogicalSize) {}

    fn set_min_size(&mut self, _min_size: LogicalSize) {}

    fn set_resizable(&mut self, _resizable: bool) {}

    fn set_theme(&mut self, theme: Theme) {
        set_dark_mode(theme == Theme::Dark);
    }

    fn set_background_color(&mut self, color: u32) {
        self.host.set_background(color);
        set_system_bar_appearance(color);
        if !self.host.draws_content() {
            paint_background(&self.host);
        }
    }
}
