/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The display frames of a window: a tick callback of its frame clock, which runs while
//! animation frames are requested and is removed one frame after.

use std::cell::Cell;
use std::ffi::c_void;
use std::rc::{Rc, Weak};

use super::headers::*;
use crate::NativeWindowHandle;
use crate::content::ContentHost;

unsafe extern "C" {
    fn gtk_widget_add_tick_callback(
        widget: *mut GtkWidget,
        callback: extern "C" fn(*mut GtkWidget, *mut c_void, *const c_void) -> i32,
        data: *const c_void,
        notify: extern "C" fn(*const c_void),
    ) -> u32;
    fn gtk_widget_remove_tick_callback(widget: *mut GtkWidget, id: u32);
}

struct State {
    host: Weak<ContentHost>,
    requested: Cell<bool>,
    callback: Cell<Option<u32>>,
}

pub(crate) struct FrameSource {
    window: *mut GtkWidget,
    state: Rc<State>,
}

impl FrameSource {
    pub(crate) fn new(handle: NativeWindowHandle, host: Weak<ContentHost>) -> Option<Self> {
        let NativeWindowHandle::Gtk(window) = handle else {
            return None;
        };
        Some(Self {
            window: window.cast(),
            state: Rc::new(State {
                host,
                requested: Cell::new(false),
                callback: Cell::new(None),
            }),
        })
    }

    pub(crate) fn request(&self) {
        self.state.requested.set(true);
        if self.state.callback.get().is_none() {
            let data = Rc::into_raw(self.state.clone()).cast();
            let id = unsafe { gtk_widget_add_tick_callback(self.window, tick, data, release) };
            self.state.callback.set(Some(id));
        }
    }
}

impl Drop for FrameSource {
    fn drop(&mut self) {
        if let Some(id) = self.state.callback.take() {
            unsafe { gtk_widget_remove_tick_callback(self.window, id) };
        }
    }
}

extern "C" fn tick(_: *mut GtkWidget, _: *mut c_void, state: *const c_void) -> i32 {
    let state = unsafe { &*state.cast::<State>() };
    if !state.requested.replace(false) {
        state.callback.set(None);
        return 0;
    }
    if let Some(host) = state.host.upgrade() {
        host.animation_frame();
    }
    1
}

extern "C" fn release(state: *const c_void) {
    drop(unsafe { Rc::from_raw(state.cast::<State>()) });
}
