/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Vulkan swapchain presentation, shared by the GTK and Android windows.

use std::cell::Cell;
use std::ptr;
use std::rc::Rc;

use super::*;

const fn from_format(value: u32) -> Option<TextureFormat> {
    match value {
        37 => Some(TextureFormat::Rgba8Unorm),
        43 => Some(TextureFormat::Rgba8UnormSrgb),
        44 => Some(TextureFormat::Bgra8Unorm),
        50 => Some(TextureFormat::Bgra8UnormSrgb),
        _ => None,
    }
}

pub(crate) type Factory = Rc<Instance>;

/// A Vulkan surface, kept alive by every swapchain created for it.
pub(super) struct SurfaceOwner {
    pub(super) instance: Rc<Instance>,
    pub(super) handle: u64,
    /// The native window the surface presents to, when the surface does not retain it itself.
    pub(super) _window: Option<Rc<dyn std::any::Any>>,
}

impl Drop for SurfaceOwner {
    fn drop(&mut self) {
        unsafe {
            call!(
                self.instance,
                DestroySurfaceKHR,
                self.instance.handle,
                self.handle,
                ptr::null()
            );
        }
    }
}

pub(super) struct Swapchain {
    surface: Rc<SurfaceOwner>,
    handle: u64,
    device: Rc<NativeDevice>,
    images: Vec<u64>,
    views: Vec<u64>,
    finished: Vec<u64>,
    size: (u32, u32),
}

impl Drop for Swapchain {
    fn drop(&mut self) {
        unsafe {
            let d = &self.device;
            call!(d.instance, DeviceWaitIdle, d.handle);
            for &view in &self.views {
                call!(d.instance, DestroyImageView, d.handle, view, ptr::null());
            }
            for &sem in &self.finished {
                call!(d.instance, DestroySemaphore, d.handle, sem, ptr::null());
            }
            call!(
                d.instance,
                DestroySwapchainKHR,
                d.handle,
                self.handle,
                ptr::null()
            );
        }
    }
}

impl Context {
    pub(crate) fn with_surface(surface: &Surface) -> Result<Rc<Self>> {
        // Without a native window yet (Android), any graphics queue presents.
        let handle = surface.owner().map(|owner| owner.handle);
        Self::create(&surface.instance, true, handle)
    }

    /// Whether this submission consumes the acquisition semaphore.
    ///
    /// A binary semaphore's signal can only be consumed once per acquisition.
    pub(super) fn take_acquisition_wait(&self) -> bool {
        self.acquired.borrow().is_some() && !self.acquisition_waited.replace(true)
    }

    pub(crate) fn formats(&self, surface: &Surface) -> Result<Vec<TextureFormat>> {
        let Some(owner) = surface.owner() else {
            // The formats every Android swapchain supports, in its order.
            return Ok(vec![
                TextureFormat::Rgba8Unorm,
                TextureFormat::Rgba8UnormSrgb,
            ]);
        };
        unsafe {
            let d = &self.device;
            let mut count = 0;
            check(call!(
                d.instance,
                GetPhysicalDeviceSurfaceFormatsKHR,
                d.physical,
                owner.handle,
                &mut count,
                ptr::null_mut()
            ))?;
            let mut formats = vec![vk::SurfaceFormatKHR::default(); count as usize];
            check(call!(
                d.instance,
                GetPhysicalDeviceSurfaceFormatsKHR,
                d.physical,
                owner.handle,
                &mut count,
                formats.as_mut_ptr()
            ))?;
            if formats.len() == 1 && formats[0].format == 0 {
                return Ok(vec![TextureFormat::Bgra8UnormSrgb]);
            }
            Ok(formats
                .iter()
                .filter(|f| f.color_space == 0)
                .filter_map(|f| from_format(f.format))
                .collect())
        }
    }

    pub(crate) fn configure(&self, surface: &Surface, config: &SurfaceConfiguration) -> Result<()> {
        if surface.is_lost() {
            return Err(Error("Presentation surface is no longer available".into()));
        }
        let Some(owner) = surface.owner() else {
            // The swapchain is created once the native window exists, see acquire.
            return Ok(());
        };
        unsafe {
            let d = &self.device;
            require(
                Rc::ptr_eq(&surface.instance, &d.instance),
                "Surface belongs to another Vulkan instance",
            );
            require(
                self.acquired.borrow().is_none(),
                "Present the acquired frame before reconfiguring the surface",
            );
            for frame in self.frames.borrow_mut().iter_mut() {
                self.wait(frame, u64::MAX)?;
            }
            check(call!(d.instance, DeviceWaitIdle, d.handle))?;
            self.surface_status.set(0);
            let mut caps = vk::SurfaceCapabilitiesKHR::default();
            check(call!(
                d.instance,
                GetPhysicalDeviceSurfaceCapabilitiesKHR,
                d.physical,
                owner.handle,
                &mut caps
            ))?;
            // Android may report a pre-rotated extent, its identity swapchain matches the window.
            let extent = if caps.current_extent.width != u32::MAX && !cfg!(target_os = "android") {
                caps.current_extent
            } else {
                vk::Extent2D {
                    width: config
                        .width
                        .clamp(caps.min_image_extent.width, caps.max_image_extent.width),
                    height: config
                        .height
                        .clamp(caps.min_image_extent.height, caps.max_image_extent.height),
                }
            };
            if extent.width != config.width || extent.height != config.height {
                return Err(Error(
                    "Surface extent differs from window extent; retry after resize".into(),
                ));
            }
            #[cfg(not(target_os = "android"))]
            surface.set_buffer_size(extent.width, extent.height);
            let count = if caps.max_image_count == 0 {
                caps.min_image_count.max(3)
            } else {
                caps.min_image_count.max(3).min(caps.max_image_count)
            };
            let old = surface
                .swap
                .borrow()
                .as_ref()
                .filter(|swap| Rc::ptr_eq(&swap.surface, &owner))
                .map_or(0, |swap| swap.handle);
            let info = vk::SwapchainCreateInfoKHR {
                s_type: 1000001000,
                surface: owner.handle,
                min_image_count: count,
                image_format: format(config.format),
                image_color_space: 0,
                image_extent: extent,
                image_array_layers: 1,
                image_usage: 16,
                // Let the compositor rotate, instead of pre-rotating every frame.
                pre_transform: if caps.supported_transforms & 1 != 0 {
                    1
                } else {
                    caps.current_transform
                },
                composite_alpha: 1 << caps.supported_composite_alpha.trailing_zeros(),
                present_mode: 2,
                clipped: 1,
                old_swapchain: old,
                ..Default::default()
            };
            let mut swap = Swapchain {
                surface: owner,
                handle: 0,
                device: d.clone(),
                images: Vec::new(),
                views: Vec::new(),
                finished: Vec::new(),
                size: (extent.width, extent.height),
            };
            check(call!(
                d.instance,
                CreateSwapchainKHR,
                d.handle,
                &info,
                ptr::null(),
                &mut swap.handle
            ))?;
            let mut count = 0;
            check(call!(
                d.instance,
                GetSwapchainImagesKHR,
                d.handle,
                swap.handle,
                &mut count,
                ptr::null_mut()
            ))?;
            swap.images.resize(count as usize, 0);
            check(call!(
                d.instance,
                GetSwapchainImagesKHR,
                d.handle,
                swap.handle,
                &mut count,
                swap.images.as_mut_ptr()
            ))?;
            for &image in &swap.images {
                swap.views.push(self.view(image, config.format, 1, 1)?);
                let info = vk::SemaphoreCreateInfo {
                    s_type: 9,
                    ..Default::default()
                };
                let mut sem = 0;
                check(call!(
                    d.instance,
                    CreateSemaphore,
                    d.handle,
                    &info,
                    ptr::null(),
                    &mut sem
                ))?;
                swap.finished.push(sem);
            }
            *surface.swap.borrow_mut() = Some(Rc::new(swap));
            Ok(())
        }
    }

    pub(crate) fn acquire(
        &self,
        surface: &Surface,
    ) -> std::result::Result<(Texture, bool), CurrentSurfaceTexture> {
        // GTK may have destroyed the native surface while the application still
        // retains its renderer. Never pass its stale Wayland/X11 handle to WSI.
        if surface.is_lost() {
            return Err(CurrentSurfaceTexture::Lost);
        }
        if !surface.is_ready() {
            return Err(CurrentSurfaceTexture::Occluded);
        }
        let Some(owner) = surface.owner() else {
            return Err(CurrentSurfaceTexture::Occluded);
        };
        match self.surface_status.replace(0) {
            -1000001004 | 1000001003 => return Err(CurrentSurfaceTexture::Outdated),
            -1000000000 => return Err(CurrentSurfaceTexture::Lost),
            _ => {}
        }
        if self.acquired.borrow().is_some() {
            return Err(CurrentSurfaceTexture::Validation);
        }
        let mut frames = self.frames.borrow_mut();
        let frame = &mut frames[self.slot.get()];
        // Yield to GTK while the GPU is busy; the caller retries on a later frame tick.
        if !self
            .wait(frame, 0)
            .map_err(|_| CurrentSurfaceTexture::Lost)?
        {
            return Err(CurrentSurfaceTexture::Timeout);
        }
        let swap = surface
            .swap
            .borrow()
            .clone()
            .ok_or(CurrentSurfaceTexture::Outdated)?;
        // Wayland has no fixed surface extent, so the driver need not report
        // OUT_OF_DATE when GTK resizes the content or changes its scale.
        // Android replaces the native window when the app returns to the foreground.
        if swap.size != surface.size() || !Rc::ptr_eq(&swap.surface, &owner) {
            return Err(CurrentSurfaceTexture::Outdated);
        }
        let mut index = 0;
        let result = unsafe {
            call!(
                self.device.instance,
                AcquireNextImageKHR,
                self.device.handle,
                swap.handle,
                0,
                frame.available,
                0,
                &mut index
            )
        };
        match result {
            0 | 1000001003 => {}
            -1000001004 => return Err(CurrentSurfaceTexture::Outdated),
            -1000000000 => return Err(CurrentSurfaceTexture::Lost),
            1 | 2 => return Err(CurrentSurfaceTexture::Timeout),
            _ => return Err(CurrentSurfaceTexture::Validation),
        }
        *self.acquired.borrow_mut() = Some((swap.clone(), index as usize));
        self.acquisition_waited.set(false);
        Ok((
            Texture {
                image: swap.images[index as usize],
                view: swap.views[index as usize],
                memory: None,
                swap: Some(swap),
                device: self.device.clone(),
                layout: Cell::new(0),
                mips: 1,
                layers: 1,
                depth: false,
                index: index as usize,
            },
            result != 0,
        ))
    }

    pub(crate) fn present(&self, surface: &Surface, texture: &Texture) -> Result<()> {
        require(
            self.acquisition_waited.get(),
            "Present requires a submission after acquisition",
        );
        let (swap, index) = self
            .acquired
            .borrow_mut()
            .take()
            .ok_or_else(|| Error("No acquired frame".into()))?;
        require(
            texture.index == index && texture.swap.as_ref().is_some_and(|s| Rc::ptr_eq(s, &swap)),
            "Wrong surface texture",
        );
        if surface.is_lost() {
            self.acquisition_waited.set(false);
            self.surface_status.set(-1000000000);
            self.slot.set((self.slot.get() + 1) % 2);
            return Ok(());
        }
        // Signal only after all submissions for this image. Signaling on every
        // submit would reuse an unconsumed binary semaphore before presentation.
        let ready = vk::SubmitInfo {
            s_type: 4,
            signal_semaphore_count: 1,
            p_signal_semaphores: &swap.finished[index],
            ..Default::default()
        };
        unsafe {
            check(call!(
                self.device.instance,
                QueueSubmit,
                self.device.queue,
                1,
                &ready,
                0
            ))?;
        }
        self.acquisition_waited.set(false);
        let index32 = index as u32;
        let info = vk::PresentInfoKHR {
            s_type: 1000001001,
            wait_semaphore_count: 1,
            p_wait_semaphores: &swap.finished[index],
            swapchain_count: 1,
            p_swapchains: &swap.handle,
            p_image_indices: &index32,
            ..Default::default()
        };
        let result = unsafe {
            call!(
                self.device.instance,
                QueuePresentKHR,
                self.device.queue,
                &info
            )
        };
        self.slot.set((self.slot.get() + 1) % 2);
        if matches!(result, 1000001003 | -1000001004 | -1000000000) {
            self.surface_status.set(result);
            Ok(())
        } else {
            check(result)
        }
    }
}
