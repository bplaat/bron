/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

pub(crate) use crate::platforms::android::event_loop::{
    PlatformEventLoop, PlatformEventLoopProxy, PlatformMonitor,
};
pub(crate) use crate::platforms::android::headers as native_headers;
pub(crate) use crate::platforms::android::window::PlatformWindow;

mod activity;
mod event_loop;
pub(crate) mod headers;
pub(crate) mod input;
mod jni;
mod window;

pub(crate) mod ffi {
    pub use super::activity::{android_activity, android_run_on_ui_thread};
    pub use super::event_loop::{
        android_density, android_native_window, android_post, android_post_frame,
        android_post_local, android_surface, android_window_size,
    };
    pub use super::jni::{DexClassLoader, GlobalRef, JniEnv, native_method};
    pub use super::window::{android_back_pressed, android_default_background};
}
