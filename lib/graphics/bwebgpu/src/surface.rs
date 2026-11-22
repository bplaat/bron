/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Window presentation surfaces drawing a bwindow window: its whole content, opaque, or a
//! transparent layer that stacks with canvas and other GPU layers.

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
    owner: Rc<bwindow::WindowAttachment>,
    shared: RefCell<Option<Rc<Shared>>>,
    config: RefCell<Option<SurfaceConfiguration>>,
    _lifetime: PhantomData<&'a ()>,
}

/// What a surface presents to: the whole window content, or a transparent window layer.
#[derive(Clone, Copy)]
pub enum SurfaceTarget<'a> {
    /// The whole window content, opaque.
    Window(&'a bwindow::Window),
    /// A layer of the window, transparent with premultiplied alpha. It needs native window
    /// layers, which every platform with a GPU backend has, except Android 8 and 9 and X11
    /// without the Composite extension.
    Layer(&'a bwindow::WindowLayer),
}

impl<'a> From<&'a bwindow::Window> for SurfaceTarget<'a> {
    fn from(window: &'a bwindow::Window) -> Self {
        Self::Window(window)
    }
}

impl<'a> From<&'a bwindow::WindowLayer> for SurfaceTarget<'a> {
    fn from(layer: &'a bwindow::WindowLayer) -> Self {
        Self::Layer(layer)
    }
}

impl Instance {
    /// Create a presentation surface for a window, or a layer of it to stack with canvases.
    pub fn create_surface<'a>(
        &self,
        target: impl Into<SurfaceTarget<'a>>,
    ) -> Result<Surface<'static>> {
        let (attachment, layer) = match target.into() {
            SurfaceTarget::Window(window) => (window.attach_content(), false),
            SurfaceTarget::Layer(layer) => (layer.attach(), true),
        };
        let attachment =
            attachment.map_err(|error| Error(format!("Cannot attach to the window: {error}")))?;
        if layer && !attachment.is_native_layer() {
            return Err(Error(
                "GPU layers need native window layers, draw the whole window instead".into(),
            ));
        }
        let native = Rc::new(platform::Surface::new(
            &attachment,
            &mut self.factory.borrow_mut(),
        )?);
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
    /// Redraw the surface at the next display frame of the window, without redrawing its
    /// canvas layers.
    pub fn request_animation_frame(&self) {
        self.owner.request_animation_frame();
    }

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
                        _window: Some(self.owner.clone() as Rc<dyn std::any::Any>),
                    }),
                };
                let frame = SurfaceTexture {
                    texture,
                    surface: self.native.clone(),
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
