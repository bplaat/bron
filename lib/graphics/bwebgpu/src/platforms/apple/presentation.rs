/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use super::*;

impl Context {
    pub(crate) fn with_surface(_: &Surface) -> Result<Rc<Self>> {
        Self::new()
    }

    pub(crate) fn formats(&self, _: &Surface) -> Result<Vec<TextureFormat>> {
        Ok(vec![
            TextureFormat::Bgra8UnormSrgb,
            TextureFormat::Bgra8Unorm,
        ])
    }

    pub(crate) fn configure(&self, surface: &Surface, config: &SurfaceConfiguration) -> Result<()> {
        self.wait(0)?;
        self.wait(1)?;
        unsafe {
            let _: () = msg_send![&*surface.layer, setDevice: &*self.device];
            let _: () = msg_send![&*surface.layer, setPixelFormat: format(config.format)];
            let _: () = msg_send![&*surface.layer, setFramebufferOnly: Bool::YES];
            #[cfg(target_os = "macos")]
            let _: () = msg_send![&*surface.layer, setDisplaySyncEnabled: Bool::YES];
            #[cfg(target_os = "macos")]
            let scale: f64 = {
                let window: *mut AnyObject = msg_send![&*surface.view, window];
                if window.is_null() {
                    1.0
                } else {
                    msg_send![window, backingScaleFactor]
                }
            };
            #[cfg(target_os = "ios")]
            let scale: f64 = msg_send![&*surface.view, contentScaleFactor];
            layout_layer(&surface.view, &surface.layer);
            let _: () = msg_send![&*surface.layer, setContentsScale: scale];
            let _: () = msg_send![&*surface.layer, setDrawableSize: Size { width: config.width as f64, height: config.height as f64 }];
        }
        Ok(())
    }

    pub(crate) fn acquire(
        &self,
        surface: &Surface,
    ) -> std::result::Result<(Texture, bool), CurrentSurfaceTexture> {
        self.wait(self.slot.get())
            .map_err(|_| CurrentSurfaceTexture::Lost)?;
        autoreleasepool(|_| unsafe {
            let drawable = retained(msg_send![&*surface.layer, nextDrawable])
                .map_err(|_| CurrentSurfaceTexture::Timeout)?;
            let object = retained(msg_send![&*drawable, texture])
                .map_err(|_| CurrentSurfaceTexture::Lost)?;
            Ok((
                Texture {
                    object,
                    drawable: Some(drawable),
                },
                false,
            ))
        })
    }

    pub(crate) fn present(&self, _: &Surface, texture: &Texture) -> Result<()> {
        autoreleasepool(|_| unsafe {
            let drawable = texture
                .drawable
                .as_ref()
                .ok_or_else(|| Error("Texture is not a drawable".into()))?;
            // A separate command buffer orders presentation after the submitted rendering.
            let command = retained(msg_send![&*self.queue, commandBuffer])?;
            let _: () = msg_send![&*command, presentDrawable: &**drawable];
            let _: () = msg_send![&*command, commit];
            self.frames.borrow_mut()[self.slot.get()]
                .as_mut()
                .ok_or_else(|| Error("Present requires a submission".into()))?
                .presentation = Some(command);
            self.slot.set((self.slot.get() + 1) % 2);
            Ok(())
        })
    }
}
