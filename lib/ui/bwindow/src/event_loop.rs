/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::marker::PhantomData;
use std::rc::Rc;

use crate::platforms::{PlatformEventLoop, PlatformEventLoopProxy, PlatformMonitor};
use crate::{Event, LogicalPoint, LogicalSize, NativeEvent, Theme};

// MARK: AppId
#[cfg_attr(
    any(target_vendor = "apple", target_os = "android", windows),
    allow(dead_code, reason = "only GTK names the application")
)]
pub(crate) struct AppId {
    pub(crate) qualifier: String,
    pub(crate) organization: String,
    pub(crate) application: String,
}

// MARK: EventLoopBuilder
/// EventLoop builder
pub struct EventLoopBuilder<T = ()> {
    user_event: PhantomData<fn(T)>,
    pub(crate) app_id: Option<AppId>,
    pub(crate) single_instance: bool,
    #[cfg(feature = "menu")]
    pub(crate) menu: Option<crate::MenuBarBuilder>,
}

impl<T> Default for EventLoopBuilder<T> {
    fn default() -> Self {
        Self {
            user_event: PhantomData,
            app_id: None,
            single_instance: true,
            #[cfg(feature = "menu")]
            menu: None,
        }
    }
}

impl EventLoopBuilder {
    /// Create new event loop builder
    pub fn new() -> Self {
        Self::default()
    }
}

impl<T: Send + 'static> EventLoopBuilder<T> {
    /// Select the application's event type.
    pub fn with_user_event<U: Send + 'static>(self) -> EventLoopBuilder<U> {
        EventLoopBuilder {
            user_event: PhantomData,
            app_id: self.app_id,
            single_instance: self.single_instance,
            #[cfg(feature = "menu")]
            menu: self.menu,
        }
    }

    /// App id used for storing window state and other platform specific features
    pub fn app_id(
        mut self,
        qualifier: impl AsRef<str>,
        organization: impl AsRef<str>,
        application: impl AsRef<str>,
    ) -> Self {
        self.app_id = Some(AppId {
            qualifier: qualifier.as_ref().to_string(),
            organization: organization.as_ref().to_string(),
            application: application.as_ref().to_string(),
        });
        self
    }

    /// Set whether only one instance of the application may run (enabled by default)
    pub const fn single_instance(mut self, single_instance: bool) -> Self {
        self.single_instance = single_instance;
        self
    }

    /// Set menus and application shortcuts.
    /// macOS displays a native menu bar; GTK exports it for compatible global menus
    /// while hiding it inside the window. Windows registers shortcuts only.
    #[cfg(feature = "menu")]
    pub fn menu(mut self, menu: crate::MenuBarBuilder) -> Self {
        self.menu = Some(menu);
        self
    }

    /// Create new event loop
    pub fn build(self) -> EventLoop<T> {
        EventLoop::from_platform(PlatformEventLoop::new(self.with_user_event::<()>()))
    }
}

// MARK: EventLoop
pub(crate) trait EventLoopInterface {
    fn theme(&self) -> Theme;
    fn primary_monitor(&self) -> PlatformMonitor;
    fn available_monitors(&self) -> Vec<PlatformMonitor>;
    fn create_proxy(&self) -> PlatformEventLoopProxy;
    fn run(self, event_handler: impl FnMut(NativeEvent) + 'static) -> i32;
}

/// Event loop
pub struct EventLoop<T = ()> {
    platform: PlatformEventLoop,
    user_event: PhantomData<fn(T)>,
    main_thread: PhantomData<Rc<()>>,
}

impl EventLoop {
    /// Create new event loop
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        EventLoopBuilder::new().build()
    }
}

impl<T: Send + 'static> EventLoop<T> {
    pub(crate) const fn from_platform(platform: PlatformEventLoop) -> Self {
        Self {
            platform,
            user_event: PhantomData,
            main_thread: PhantomData,
        }
    }

    /// Get the current system theme
    pub fn theme(&self) -> Theme {
        self.platform.theme()
    }

    /// Get primary monitor
    pub fn primary_monitor(&self) -> Monitor {
        Monitor::new(self.platform.primary_monitor())
    }

    /// List available monitors
    pub fn available_monitors(&self) -> Vec<Monitor> {
        self.platform
            .available_monitors()
            .into_iter()
            .map(Monitor::new)
            .collect()
    }

    /// Create new event loop proxy
    pub fn create_proxy(&self) -> EventLoopProxy<T> {
        EventLoopProxy::new(self.platform.create_proxy())
    }

    /// Run until shutdown, release event-loop resources, then exit the process.
    ///
    /// This never returns. Move resources requiring cleanup into the handler.
    /// `Event::LoopExiting` is delivered before the handler is dropped.
    pub fn run(self, mut event_handler: impl FnMut(Event<T>) + 'static) -> ! {
        let status = self.platform.run(move |event| {
            event_handler(event.map_user_event(|value| {
                *value
                    .downcast::<T>()
                    .expect("event proxy payload type mismatch")
            }));
        });
        std::process::exit(status);
    }
}

// MARK: EventLoopProxy
pub(crate) trait EventLoopProxyInterface {
    fn send_user_event(&self, data: Box<dyn std::any::Any + Send>) -> bool;
    fn exit(&self) -> bool;
}

/// Event loop proxy
pub struct EventLoopProxy<T = ()> {
    platform: PlatformEventLoopProxy,
    user_event: PhantomData<fn(T)>,
}

impl<T: Send + 'static> EventLoopProxy<T> {
    pub(crate) const fn new(proxy: PlatformEventLoopProxy) -> Self {
        Self {
            platform: proxy,
            user_event: PhantomData,
        }
    }

    /// Send user event to the event loop
    pub fn send_user_event(&self, data: T) -> Result<(), EventLoopClosed> {
        self.platform
            .send_user_event(Box::new(data))
            .then_some(())
            .ok_or(EventLoopClosed)
    }

    /// Ask the event loop to stop without requesting window close confirmation.
    pub fn exit(&self) -> Result<(), EventLoopClosed> {
        self.platform.exit().then_some(()).ok_or(EventLoopClosed)
    }
}

/// The event loop has closed or could not be woken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventLoopClosed;

impl std::fmt::Display for EventLoopClosed {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("event loop is closed")
    }
}

impl std::error::Error for EventLoopClosed {}

// MARK: Monitor
pub(crate) trait MonitorInterface {
    fn name(&self) -> String;
    fn position(&self) -> LogicalPoint;
    fn size(&self) -> LogicalSize;
    fn scale_factor(&self) -> f32;
    fn is_primary(&self) -> bool;
}

/// Monitor
pub struct Monitor(pub(crate) PlatformMonitor);

impl Monitor {
    pub(crate) const fn new(monitor: PlatformMonitor) -> Self {
        Self(monitor)
    }

    /// Get monitor name
    pub fn name(&self) -> String {
        self.0.name()
    }

    /// Get monitor position
    ///
    /// Primary monitor is 0x0 position all other monitors are relative to the primary monitor.
    pub fn position(&self) -> LogicalPoint {
        self.0.position()
    }

    /// Get monitor size
    pub fn size(&self) -> LogicalSize {
        self.0.size()
    }

    /// Get monitor scale factor
    pub fn scale_factor(&self) -> f32 {
        self.0.scale_factor()
    }

    /// Get if monitor is primary
    pub fn is_primary(&self) -> bool {
        self.0.is_primary()
    }
}
