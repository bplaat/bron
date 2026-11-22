/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Vulkan surfaces for the bwindow Android activity window and the surfaces of native
//! window layers.

use std::cell::RefCell;
use std::ffi::c_void;
use std::ptr;
use std::rc::Rc;

use bwindow::ffi::{android_native_window, android_with_env};

use super::surface::{SurfaceOwner, Swapchain};
use super::*;

#[link(name = "android")]
unsafe extern "C" {
    fn ANativeWindow_getWidth(window: *mut vk::ANativeWindow) -> i32;
    fn ANativeWindow_getHeight(window: *mut vk::ANativeWindow) -> i32;
    fn ANativeWindow_fromSurface(env: *mut c_void, surface: *mut c_void) -> *mut vk::ANativeWindow;
    fn ANativeWindow_release(window: *mut vk::ANativeWindow);
}

// The framework replaces the native window when the app returns to the foreground,
// so the Vulkan surface is created for every new window, and only once it exists.
pub(crate) struct Surface {
    pub(super) instance: Rc<Instance>,
    current: RefCell<Option<(Rc<SurfaceOwner>, u64)>>,
    pub(super) swap: RefCell<Option<Rc<Swapchain>>>,
    // The native window of a layer surface, which it holds a reference to.
    layer: Option<*mut vk::ANativeWindow>,
}

impl Drop for Surface {
    fn drop(&mut self) {
        // The swapchain and surface go before the native window they present to.
        self.swap.borrow_mut().take();
        self.current.borrow_mut().take();
        if let Some(window) = self.layer {
            unsafe { ANativeWindow_release(window) };
        }
    }
}

impl Surface {
    pub(crate) fn new(
        attachment: &bwindow::WindowAttachment,
        factory: &mut Option<Factory>,
    ) -> Result<Self> {
        let layer = match unsafe { attachment.content_handle() } {
            Some(bwindow::NativeContentHandle::Android(_)) => None,
            Some(bwindow::NativeContentHandle::AndroidSurface(surface)) => {
                let window = android_with_env(|env| {
                    Ok(unsafe { ANativeWindow_fromSurface(env.get_raw().cast(), surface) })
                });
                if window.is_null() {
                    return Err(Error("Cannot get the native window of a layer".into()));
                }
                Some(window)
            }
            _ => return Err(Error("Vulkan requires an Android window".into())),
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
            layer,
        })
    }

    // The native window the surface presents to with its generation, a layer keeps one.
    fn native_window(&self) -> (*mut vk::ANativeWindow, u64) {
        match self.layer {
            Some(window) => (window, 0),
            None => {
                let (window, generation) = android_native_window();
                (window.cast(), generation)
            }
        }
    }

    /// Whether the surface presents to a window layer.
    pub(super) const fn is_layer(&self) -> bool {
        self.layer.is_some()
    }

    /// The size of the whole native window, the swapchain draws under the system bars.
    pub(crate) fn size(&self) -> (u32, u32) {
        let (window, _) = self.native_window();
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
        let (window, generation) = self.native_window();
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
