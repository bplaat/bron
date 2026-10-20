/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use crate::{LogicalPoint, LogicalSize};

/// A close request that can be prevented by the event handler
#[derive(Clone)]
pub struct CloseRequest(Rc<Cell<bool>>);

impl CloseRequest {
    pub(crate) fn new() -> Self {
        Self(Rc::new(Cell::new(false)))
    }

    /// Prevent the default close action. Other window events are not cancelable.
    pub fn prevent_default(&self) {
        self.0.set(true);
    }

    /// Whether a handler has prevented the default close action.
    pub fn default_prevented(&self) -> bool {
        self.0.get()
    }
}

/// Window event
pub enum WindowEvent {
    /// The window's effective light/dark appearance changed.
    ThemeChanged(crate::Theme),
    /// Paint the window's content. Native drawing is scoped to this callback.
    RedrawRequested,
    /// The window gained keyboard focus.
    Focus,
    /// The window lost keyboard focus.
    Blur,
    /// Pointer position in logical client-area pixels.
    MouseMove {
        /// Current position.
        position: LogicalPoint,
        /// Active keyboard modifiers.
        modifiers: crate::Modifiers,
    },
    /// Pointer left the client area.
    MouseLeave,
    /// A mouse button was pressed.
    MouseDown(crate::MouseEvent),
    /// A mouse button was released.
    MouseUp(crate::MouseEvent),
    /// Wheel or trackpad scroll.
    Wheel(crate::ScrollDelta, crate::Modifiers),
    /// A keyboard key was pressed, including auto-repeat presses.
    KeyDown(crate::KeyboardEvent),
    /// A keyboard key was released.
    KeyUp(crate::KeyboardEvent),
    /// Window create
    Create,
    /// Window move
    Move(LogicalPoint),
    /// Window resize
    Resize(LogicalSize),
    /// Window close requested; closes normally unless prevented
    CloseRequested(CloseRequest),
    /// File dropped
    #[cfg(feature = "file_drop")]
    DroppedFile(PathBuf),
    /// macOS window fullscreen change
    #[cfg(target_os = "macos")]
    MacosFullscreenChange(bool),
}

/// Event
pub enum Event<T = ()> {
    /// Window event
    Window(crate::WindowId, WindowEvent),
    /// The event loop is shutting down. No further events will be delivered.
    LoopExiting,
    /// User event
    UserEvent(T),
    /// Files opened through macOS Launch Services
    #[cfg(target_os = "macos")]
    MacosOpenFiles(Vec<PathBuf>),
    /// A custom macOS menu item was selected
    #[cfg(all(target_os = "macos", feature = "menu"))]
    MacosMenuItem(String),
}

pub(crate) type NativeEvent = Event<Box<dyn std::any::Any + Send>>;

impl<T> Event<T> {
    pub(crate) fn map_user_event<U>(self, map: impl FnOnce(T) -> U) -> Event<U> {
        match self {
            Self::Window(id, event) => Event::Window(id, event),
            Self::UserEvent(value) => Event::UserEvent(map(value)),
            Self::LoopExiting => Event::LoopExiting,
            #[cfg(target_os = "macos")]
            Self::MacosOpenFiles(paths) => Event::MacosOpenFiles(paths),
            #[cfg(all(target_os = "macos", feature = "menu"))]
            Self::MacosMenuItem(action) => Event::MacosMenuItem(action),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prevent_default_is_shared_and_idempotent() {
        let request = CloseRequest::new();
        let observed = request.clone();
        assert!(!request.default_prevented());
        observed.prevent_default();
        observed.prevent_default();
        assert!(request.default_prevented());
    }
}
