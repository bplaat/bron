/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The content of a window: one renderer taking the whole window, or a stack of layers.
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::rc::{Rc, Weak};

use crate::{LogicalPoint, LogicalRect, WindowEvent, WindowId};

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
    /// The DirectComposition visual of one layer of a Win32 window with its
    /// `IDCompositionDesktopDevice`. Renderers set a composition swap chain as its content
    /// and commit the device.
    DirectComposition {
        /// The `IDCompositionDesktopDevice` of the window.
        device: *mut c_void,
        /// The `IDCompositionVisual2` of the layer.
        visual: *mut c_void,
    },
    /// A JNI global reference to the `android.view.Surface` of one layer of a window, a
    /// child surface the system compositor blends with the window.
    AndroidSurface(*mut c_void),
    /// The GDK window of one layer: a subsurface on Wayland, or an ARGB child window on
    /// X11, which the X server composites over its parent. The window keeps it placed over
    /// its content and it leaves all input to the window.
    GdkLayer(*mut c_void),
    /// A JNI global reference to the `android.app.NativeActivity`, whose window surface
    /// renderers draw into.
    Android(*mut c_void),
}

/// A native view that fills the window content area instead of the content host, like a
/// scroll view. It receives its own input, so the window does not report it again.
#[cfg(not(any(target_os = "ios", target_os = "android")))]
#[derive(Clone, Copy, Debug)]
pub enum NativeViewHandle {
    /// An AppKit NSView pointer.
    AppKit(*mut c_void),
    /// A Win32 HWND created as a child of the window.
    Win32(*mut c_void),
    /// A GTK 3 GtkWidget pointer.
    Gtk(*mut c_void),
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

/// A window cannot accept the requested content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachError {
    /// The native window has closed.
    Closed,
    /// This window content or layer already has a renderer.
    Occupied,
    /// The window content and layers cannot be combined: one item takes the whole window,
    /// or layers stack in it.
    Unsupported,
}

impl std::fmt::Display for AttachError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Closed => "window is closed",
            Self::Occupied => "window already has content",
            Self::Unsupported => "window content and layers cannot be combined here",
        })
    }
}

impl std::error::Error for AttachError {}

// The native layer views of the platform, which every layer gets as its own content host
// where the platform composites them.
use crate::platforms::{
    add_layer_view, native_layers, remove_layer_view, set_layer_view_frame, set_layer_view_hidden,
};

mod attachment;
mod host;
mod layer;
mod sender;
#[cfg(test)]
mod tests;

pub use self::attachment::WindowAttachment;
pub(crate) use self::host::ContentHost;
pub(crate) use self::layer::LayerSlot;
pub use self::layer::WindowLayer;
pub use self::sender::WindowEventSender;
