/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The display frames of the window, from the display choreographer.

use std::cell::Cell;
use std::rc::{Rc, Weak};

use super::event_loop::android_post_frame;
use crate::NativeWindowHandle;
use crate::content::ContentHost;

pub(crate) struct FrameSource {
    host: Weak<ContentHost>,
    scheduled: Rc<Cell<bool>>,
}

impl FrameSource {
    pub(crate) fn new(_: NativeWindowHandle, host: Weak<ContentHost>) -> Option<Self> {
        Some(Self {
            host,
            scheduled: Rc::new(Cell::new(false)),
        })
    }

    pub(crate) fn request(&self) {
        if self.scheduled.replace(true) {
            return;
        }
        let (host, scheduled) = (self.host.clone(), self.scheduled.clone());
        android_post_frame(move || {
            scheduled.set(false);
            if let Some(host) = host.upgrade() {
                host.animation_frame();
            }
        });
    }
}
