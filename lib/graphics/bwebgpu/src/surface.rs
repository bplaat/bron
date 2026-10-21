/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Window presentation surfaces backed by bwindow content attachments.

use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;

use crate::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PresentMode {
    #[default]
    AutoVsync,
}

#[derive(Clone, Debug)]
pub struct SurfaceConfiguration {
    pub format: TextureFormat,
    pub width: u32,
    pub height: u32,
    pub present_mode: PresentMode,
}

pub struct SurfaceCapabilities {
    pub formats: Vec<TextureFormat>,
}

pub struct Surface<'a> {
    pub(crate) native: Rc<platform::Surface>,
    owner: Rc<dyn std::any::Any>,
    shared: RefCell<Option<Rc<Shared>>>,
    config: RefCell<Option<SurfaceConfiguration>>,
    _lifetime: PhantomData<&'a ()>,
}

impl Instance {
    /// Create a presentation surface from an exclusive bwindow content attachment.
    pub fn create_surface(
        &self,
        attachment: bwindow::WindowAttachment,
    ) -> Result<Surface<'static>> {
        let handle = unsafe { attachment.native_handle() }
            .ok_or_else(|| Error("The bwindow window is closed".into()))?;
        let sender = attachment.event_sender();
        let native = Rc::new(platform::Surface::new(
            handle,
            sender,
            &mut self.factory.borrow_mut(),
        )?);
        let redraw = Rc::downgrade(&native);
        attachment.on_redraw(move || {
            if let Some(surface) = redraw.upgrade() {
                surface.request_redraw();
            }
        });
        let animation_frame = Rc::downgrade(&native);
        attachment.on_animation_frame(move || {
            if let Some(surface) = animation_frame.upgrade() {
                surface.request_animation_frame();
            }
        });
        // The surface retains the attachment for the lifetime of all native presentation objects.
        Ok(Surface {
            native,
            owner: Rc::new(attachment),
            shared: RefCell::new(None),
            config: RefCell::new(None),
            _lifetime: PhantomData,
        })
    }
}

impl Surface<'_> {
    /// Return the current drawable size in physical pixels.
    pub fn size(&self) -> (u32, u32) {
        self.native.size()
    }

    pub fn get_capabilities(&self, adapter: &Adapter) -> SurfaceCapabilities {
        SurfaceCapabilities {
            formats: adapter.formats.clone(),
        }
    }

    pub fn get_default_config(
        &self,
        adapter: &Adapter,
        width: u32,
        height: u32,
    ) -> Option<SurfaceConfiguration> {
        Some(SurfaceConfiguration {
            format: *adapter.formats.first()?,
            width,
            height,
            present_mode: PresentMode::AutoVsync,
        })
    }

    pub fn configure(&self, device: &Device, config: &SurfaceConfiguration) {
        require(
            config.width > 0 && config.height > 0,
            "Surface extent must be nonzero",
        );
        device
            .shared
            .native
            .configure(&self.native, config)
            .expect("Configure surface");
        *self.shared.borrow_mut() = Some(device.shared.clone());
        *self.config.borrow_mut() = Some(config.clone());
    }

    pub fn get_current_texture(&self) -> CurrentSurfaceTexture {
        let shared = self.shared.borrow();
        let Some(shared) = shared.as_ref() else {
            return CurrentSurfaceTexture::Validation;
        };
        let config = self.config.borrow();
        let config = config.as_ref().expect("surface configuration");
        match shared.native.acquire(&self.native) {
            Ok((native, suboptimal)) => {
                let texture = Texture {
                    inner: Rc::new(TextureInner {
                        native,
                        owner: Rc::downgrade(shared),
                        size: Extent3d {
                            width: config.width,
                            height: config.height,
                            depth_or_array_layers: 1,
                        },
                        mips: 1,
                        format: config.format,
                        usage: TextureUsages::RENDER_ATTACHMENT,
                        _window: Some(self.owner.clone()),
                    }),
                };
                let frame = SurfaceTexture {
                    texture,
                    surface: self.native.clone(),
                    _owner: self.owner.clone(),
                };
                if suboptimal {
                    CurrentSurfaceTexture::Suboptimal(frame)
                } else {
                    CurrentSurfaceTexture::Success(frame)
                }
            }
            Err(status) => status,
        }
    }
}

pub enum CurrentSurfaceTexture {
    Success(SurfaceTexture),
    Suboptimal(SurfaceTexture),
    Outdated,
    Lost,
    Timeout,
    Occluded,
    Validation,
}

pub struct SurfaceTexture {
    pub texture: Texture,
    surface: Rc<platform::Surface>,
    _owner: Rc<dyn std::any::Any>,
}

impl Queue {
    pub fn present(&self, frame: SurfaceTexture) {
        self.shared.check(&frame.texture.inner.owner);
        self.shared
            .native
            .present(&frame.surface, &frame.texture.inner.native)
            .expect("Present frame");
    }
}
