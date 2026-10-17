/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use crate::platforms::{PlatformMonitor, PlatformWindow};
use crate::{LogicalPoint, LogicalSize};

/// An opaque identifier that is unique for the lifetime of the process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WindowId(u64);

impl WindowId {
    pub(crate) fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(
            NEXT.try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .expect("window identifiers exhausted"),
        )
    }
}

// MARK: Theme
/// Theme
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Theme {
    /// Light theme
    Light,
    /// Dark theme
    Dark,
}

// MARK: MacosTitlebarStyle
/// macOS titlebar style
#[cfg(target_os = "macos")]
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum MacosTitlebarStyle {
    /// Default titlebar style
    Default,
    /// Transparent titlebar
    Transparent,
    /// Hidden titlebar
    Hidden,
}

// MARK: WindowsProgressBarState
/// Windows taskbar progress state
#[cfg(all(windows, feature = "progress_bar"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WindowsProgressBarState {
    /// Show normal progress
    #[default]
    Normal,
    /// Show failed progress
    Error,
    /// Show paused progress
    Paused,
    /// Show progress without a known completion percentage
    Indeterminate,
}

// MARK: WindowBuilder
/// Window builder
pub struct WindowBuilder<'a> {
    pub(crate) window_id: WindowId,
    pub(crate) title: String,
    pub(crate) position: Option<LogicalPoint>,
    pub(crate) size: LogicalSize,
    pub(crate) min_size: Option<LogicalSize>,
    pub(crate) resizable: bool,
    pub(crate) theme: Option<Theme>,
    pub(crate) background_color: Option<u32>,
    #[cfg(feature = "remember_window_state")]
    pub(crate) remember_window_state: bool,
    #[cfg(feature = "file_drop")]
    pub(crate) allow_file_drop: bool,
    pub(crate) monitor: Option<&'a PlatformMonitor>,
    pub(crate) should_center: bool,
    pub(crate) should_fullscreen: bool,
    #[cfg(target_os = "macos")]
    pub(crate) macos_titlebar_style: MacosTitlebarStyle,
    #[cfg(target_os = "macos")]
    pub(crate) macos_traffic_light_position: Option<LogicalPoint>,
}

impl<'a> Default for WindowBuilder<'a> {
    fn default() -> Self {
        Self {
            window_id: WindowId::new(),
            title: "Untitled".to_string(),
            position: None,
            size: LogicalSize {
                width: 1024.0,
                height: 768.0,
            },
            min_size: None,
            resizable: true,
            theme: None,
            background_color: None,
            #[cfg(feature = "remember_window_state")]
            remember_window_state: false,
            #[cfg(feature = "file_drop")]
            allow_file_drop: false,
            monitor: None,
            should_center: false,
            should_fullscreen: false,
            #[cfg(target_os = "macos")]
            macos_titlebar_style: MacosTitlebarStyle::Default,
            #[cfg(target_os = "macos")]
            macos_traffic_light_position: None,
        }
    }
}

impl<'a> WindowBuilder<'a> {
    /// Create new window builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Set title
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Set position
    pub const fn position(mut self, position: LogicalPoint) -> Self {
        self.position = Some(position);
        self
    }

    /// Set size
    pub const fn size(mut self, size: LogicalSize) -> Self {
        self.size = size;
        self
    }

    /// Set minimum size
    pub const fn min_size(mut self, min_size: LogicalSize) -> Self {
        self.min_size = Some(min_size);
        self
    }

    /// Set resizable
    pub const fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// Set theme
    pub const fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }

    /// Set window background color
    pub const fn background_color(mut self, color: u32) -> Self {
        self.background_color = Some(color);
        self
    }

    /// Set remember window state
    #[cfg(feature = "remember_window_state")]
    pub const fn remember_window_state(mut self) -> Self {
        self.remember_window_state = true;
        self
    }

    /// Allow files to be dropped onto the window
    #[cfg(feature = "file_drop")]
    pub const fn allow_file_drop(mut self, allow: bool) -> Self {
        self.allow_file_drop = allow;
        self
    }

    /// Set monitor
    pub const fn monitor(mut self, monitor: &'a crate::Monitor) -> Self {
        self.monitor = Some(&monitor.0);
        self
    }

    /// Center window on monitor
    pub const fn center(mut self) -> Self {
        self.should_center = true;
        self
    }

    /// Set fullscreen
    pub const fn fullscreen(mut self) -> Self {
        self.should_fullscreen = true;
        self
    }

    /// Set macOS titlebar style
    #[cfg(target_os = "macos")]
    pub const fn macos_titlebar_style(mut self, style: MacosTitlebarStyle) -> Self {
        self.macos_titlebar_style = style;
        self
    }

    /// Set the macOS window buttons (traffic lights) position from the top left corner for
    /// transparent or hidden titlebars. The content then draws the titlebar itself and starts
    /// window drags with [`Window::macos_start_window_drag`].
    #[cfg(target_os = "macos")]
    pub const fn macos_traffic_light_position(mut self, position: LogicalPoint) -> Self {
        self.macos_traffic_light_position = Some(position);
        self
    }

    /// Build window
    pub fn build(self) -> Window {
        #[cfg(feature = "file_drop")]
        let allow_file_drop = self.allow_file_drop;
        #[cfg(not(feature = "file_drop"))]
        let allow_file_drop = false;
        let host = crate::content::ContentHost::new(
            self.window_id,
            self.background_color,
            allow_file_drop,
        );
        let platform = PlatformWindow::new(&self, host.clone());
        Window { platform, host }
    }
}

// MARK: WindowInterface
pub(crate) trait WindowInterface {
    fn close(&mut self);
    fn set_title(&mut self, title: impl AsRef<str>);
    fn position(&self) -> LogicalPoint;
    fn size(&self) -> LogicalSize;
    fn set_position(&mut self, point: LogicalPoint);
    fn set_size(&mut self, size: LogicalSize);
    fn set_min_size(&mut self, min_size: LogicalSize);
    fn set_resizable(&mut self, resizable: bool);
    fn set_theme(&mut self, theme: Theme);
    fn set_background_color(&mut self, color: u32);
    #[cfg(all(
        feature = "progress_bar",
        any(
            target_os = "linux",
            target_os = "freebsd",
            target_os = "dragonfly",
            target_os = "openbsd",
            target_os = "netbsd",
        )
    ))]
    fn gtk_set_progress_bar(&mut self, progress: Option<f32>);
    #[cfg(target_os = "macos")]
    fn macos_titlebar_size(&self) -> LogicalSize;
    #[cfg(target_os = "macos")]
    fn macos_set_document_edited(&mut self, edited: bool);
    #[cfg(target_os = "macos")]
    fn macos_start_window_drag(&mut self);
    #[cfg(target_os = "macos")]
    fn macos_perform_titlebar_double_click(&mut self);
    #[cfg(all(windows, feature = "progress_bar"))]
    fn windows_set_progress_bar(&mut self, progress: Option<f32>, state: WindowsProgressBarState);
}

// MARK: Window
/// Window
pub struct Window {
    pub(crate) platform: PlatformWindow,
    host: std::rc::Rc<crate::content::ContentHost>,
}

impl Window {
    /// Whether this window has closed. Setters become no-ops after close.
    pub fn is_closed(&self) -> bool {
        self.host.is_closed()
    }

    /// Schedule a repaint of attached drawable content. Repeated requests coalesce.
    /// Does nothing after close or when the content has no repaint scheduler.
    pub fn request_redraw(&self) {
        self.host.request_redraw();
    }

    /// Schedule one redraw at the next native display frame.
    /// Repeated requests before delivery coalesce into one frame.
    pub fn request_animation_frame(&self) {
        self.host.request_animation_frame();
    }

    /// Attach one native content backend to this window.
    pub fn attach_content(&self) -> Result<crate::WindowAttachment, crate::AttachError> {
        self.host.attach()
    }

    /// Get this window's stable identifier.
    pub const fn id(&self) -> WindowId {
        self.platform.id()
    }

    /// Close the window
    pub fn close(&mut self) {
        if self.host.is_closed() {
            return;
        }
        self.host.close();
        self.platform.close()
    }

    /// Set title
    pub fn set_title(&mut self, title: impl AsRef<str>) {
        if self.is_closed() {
            return;
        }
        self.platform.set_title(title)
    }

    /// Get position, or the origin after close.
    pub fn position(&self) -> LogicalPoint {
        if self.is_closed() {
            return LogicalPoint::new(0.0, 0.0);
        }
        self.platform.position()
    }

    /// Get size, or zero dimensions after close.
    pub fn size(&self) -> LogicalSize {
        if self.is_closed() {
            return LogicalSize::new(0.0, 0.0);
        }
        self.platform.size()
    }

    /// Set position
    pub fn set_position(&mut self, point: LogicalPoint) {
        if self.is_closed() {
            return;
        }
        self.platform.set_position(point)
    }

    /// Set size
    pub fn set_size(&mut self, size: LogicalSize) {
        if self.is_closed() {
            return;
        }
        self.platform.set_size(size)
    }

    /// Set minimum size
    pub fn set_min_size(&mut self, min_size: LogicalSize) {
        if self.is_closed() {
            return;
        }
        self.platform.set_min_size(min_size)
    }

    /// Set resizable
    pub fn set_resizable(&mut self, resizable: bool) {
        if self.is_closed() {
            return;
        }
        self.platform.set_resizable(resizable)
    }

    /// Set theme
    pub fn set_theme(&mut self, theme: Theme) {
        if self.is_closed() {
            return;
        }
        self.platform.set_theme(theme)
    }

    /// Set window background color
    pub fn set_background_color(&mut self, color: u32) {
        if self.is_closed() {
            return;
        }
        self.host.set_background(color);
        self.platform.set_background_color(color)
    }

    /// Set GTK application launcher progress, use a value above `1.0` for indeterminate progress,
    /// or hide it with `None`
    #[cfg(all(
        feature = "progress_bar",
        any(
            target_os = "linux",
            target_os = "freebsd",
            target_os = "dragonfly",
            target_os = "openbsd",
            target_os = "netbsd"
        )
    ))]
    pub fn gtk_set_progress_bar(&mut self, progress: Option<f32>) {
        if self.is_closed() {
            return;
        }
        self.platform.gtk_set_progress_bar(progress)
    }

    /// Get macOS titlebar size
    #[cfg(target_os = "macos")]
    pub fn macos_titlebar_size(&self) -> LogicalSize {
        if self.is_closed() {
            return LogicalSize::new(0.0, 0.0);
        }
        self.platform.macos_titlebar_size()
    }

    /// Set whether the macOS window represents a document with unsaved changes
    #[cfg(target_os = "macos")]
    pub fn macos_set_document_edited(&mut self, edited: bool) {
        if self.is_closed() {
            return;
        }
        self.platform.macos_set_document_edited(edited)
    }

    /// Start dragging the macOS window with the current mouse down event, used by content
    /// that draws its own titlebar
    #[cfg(target_os = "macos")]
    pub fn macos_start_window_drag(&mut self) {
        if self.is_closed() {
            return;
        }
        self.platform.macos_start_window_drag()
    }

    /// Perform the user's titlebar double click action (zoom or minimize)
    #[cfg(target_os = "macos")]
    pub fn macos_perform_titlebar_double_click(&mut self) {
        if self.is_closed() {
            return;
        }
        self.platform.macos_perform_titlebar_double_click()
    }

    /// Set Windows taskbar progress for this window, or hide it with `None`
    #[cfg(all(windows, feature = "progress_bar"))]
    pub fn windows_set_progress_bar(
        &mut self,
        progress: Option<f32>,
        state: WindowsProgressBarState,
    ) {
        if self.is_closed() {
            return;
        }
        self.platform.windows_set_progress_bar(progress, state)
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::WindowBuilder;

    #[test]
    fn window_ids_distinguish_identical_builders() {
        let first = WindowBuilder::new().window_id;
        let second = WindowBuilder::new().window_id;
        let windows = HashMap::from([(first, "first"), (second, "second")]);
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[&first], "first");
        assert_eq!(windows[&second], "second");
    }
}
