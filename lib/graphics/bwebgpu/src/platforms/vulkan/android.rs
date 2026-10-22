/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Vulkan surfaces for the bwindow Android activity window.

use std::cell::{Cell, RefCell};
use std::ptr;
use std::rc::Rc;

use bwindow::ffi::{android_native_window, android_post_frame};

use super::surface::{SurfaceOwner, Swapchain};
use super::*;

#[link(name = "android")]
unsafe extern "C" {
    fn ANativeWindow_getWidth(window: *mut vk::ANativeWindow) -> i32;
    fn ANativeWindow_getHeight(window: *mut vk::ANativeWindow) -> i32;
}

// The framework replaces the native window when the app returns to the foreground,
// so the Vulkan surface is created for every new window, and only once it exists.
pub(crate) struct Surface {
    pub(super) instance: Rc<Instance>,
    current: RefCell<Option<(Rc<SurfaceOwner>, u64)>>,
    pub(super) swap: RefCell<Option<Rc<Swapchain>>>,
    frames: Rc<FrameTimer>,
}

struct FrameTimer {
    sender: bwindow::WindowEventSender,
    scheduled: Cell<bool>,
}

impl Surface {
    pub(crate) fn new(
        attachment: &bwindow::WindowAttachment,
        factory: &mut Option<Factory>,
    ) -> Result<Self> {
        let sender = attachment.event_sender();
        let Some(bwindow::NativeContentHandle::Android(_)) =
            (unsafe { attachment.content_handle() })
        else {
            return Err(Error("Vulkan requires an Android window".into()));
        };
        let instance = if let Some(instance) = factory {
            instance.clone()
        } else {
            let instance = Instance::new(Some(c"VK_KHR_android_surface"))?;
            *factory = Some(instance.clone());
            instance
        };
        Ok(Self {
            instance,
            current: RefCell::new(None),
            swap: RefCell::new(None),
            frames: Rc::new(FrameTimer {
                sender,
                scheduled: Cell::new(false),
            }),
        })
    }

    pub(crate) fn request_redraw(&self) {
        self.frames
            .sender
            .send(bwindow::WindowEvent::RedrawRequested);
    }

    /// Redraw on the next display frame.
    pub(crate) fn request_animation_frame(&self) {
        if self.frames.scheduled.replace(true) {
            return;
        }
        let frames = Rc::downgrade(&self.frames);
        android_post_frame(move || {
            if let Some(frames) = frames.upgrade() {
                frames.scheduled.set(false);
                frames.sender.send(bwindow::WindowEvent::RedrawRequested);
            }
        });
    }

    /// The size of the whole native window, the swapchain draws under the system bars.
    pub(crate) fn size(&self) -> (u32, u32) {
        let (window, _) = android_native_window();
        if window.is_null() {
            return (0, 0);
        }
        unsafe {
            (
                ANativeWindow_getWidth(window).max(0) as u32,
                ANativeWindow_getHeight(window).max(0) as u32,
            )
        }
    }

    // Closing the only window ends the app, a missing native window is reported by owner.
    pub(super) const fn is_lost(&self) -> bool {
        false
    }

    pub(super) const fn is_ready(&self) -> bool {
        true
    }

    /// The Vulkan surface of the current native window, none while the window has no surface.
    pub(super) fn owner(&self) -> Option<Rc<SurfaceOwner>> {
        let (window, generation) = android_native_window();
        let mut current = self.current.borrow_mut();
        if window.is_null() {
            // Disconnect from the destroyed window, the next one gets a new swapchain.
            current.take();
            self.swap.borrow_mut().take();
            return None;
        }
        if let Some((owner, current_generation)) = &*current
            && *current_generation == generation
        {
            return Some(owner.clone());
        }
        let info = vk::AndroidSurfaceCreateInfoKHR {
            s_type: 1000008000,
            window,
            ..Default::default()
        };
        let mut handle = 0;
        let result = unsafe {
            call!(
                self.instance,
                CreateAndroidSurfaceKHR,
                self.instance.handle,
                &info,
                ptr::null(),
                &mut handle
            )
        };
        if check(result).is_err() {
            current.take();
            return None;
        }
        // The Vulkan surface holds its own reference to the native window.
        let owner = Rc::new(SurfaceOwner {
            instance: self.instance.clone(),
            handle,
            _window: None,
        });
        *current = Some((owner.clone(), generation));
        Some(owner)
    }
}
