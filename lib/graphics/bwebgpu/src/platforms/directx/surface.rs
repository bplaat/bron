/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! DXGI swap chain presentation for bwindow content windows.

use std::cell::RefCell;
use std::ptr;
use std::rc::Rc;

use super::*;

pub(crate) type Factory = ();

pub(crate) struct Surface {
    window: HWND,
    swap: RefCell<Option<Swapchain>>,
}

struct Swapchain {
    object: Com,
    buffers: Vec<Com>,
    format: TextureFormat,
    width: u32,
    height: u32,
}

impl Surface {
    pub(crate) fn new(
        attachment: &bwindow::WindowAttachment,
        _: &mut Option<Factory>,
    ) -> Result<Self> {
        let Some(bwindow::NativeContentHandle::Win32(window)) =
            (unsafe { attachment.content_handle() })
        else {
            return Err(Error("D3D12 requires a Win32 window".into()));
        };
        Ok(Self {
            window,
            swap: RefCell::new(None),
        })
    }

    pub(crate) fn size(&self) -> (u32, u32) {
        let mut rect = RECT::default();
        if unsafe { GetClientRect(self.window, &mut rect) } == 0 {
            return (0, 0);
        }
        (
            (rect.right - rect.left).max(0) as u32,
            (rect.bottom - rect.top).max(0) as u32,
        )
    }
}

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
        for frame in self.frames.borrow_mut().iter_mut() {
            self.wait(frame)?;
        }
        self.wait_value(self.signal()?)?;
        let mut current = surface.swap.borrow_mut();
        unsafe {
            if let Some(swap) = current.as_mut() {
                swap.buffers.clear();
                check(SwapChain3_ResizeBuffers(
                    swap.object.0,
                    3,
                    config.width,
                    config.height,
                    87,
                    0,
                ))?;
                swap.format = config.format;
                swap.width = config.width;
                swap.height = config.height;
            } else {
                let desc = DXGI_SWAP_CHAIN_DESC1 {
                    Width: config.width,
                    Height: config.height,
                    Format: 87,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    BufferUsage: 32,
                    BufferCount: 3,
                    SwapEffect: 4,
                    ..Default::default()
                };
                let swap = Com::create(|out| {
                    Factory4_CreateSwapChainForHwnd(
                        self.factory.0,
                        self.queue.0,
                        surface.window,
                        &desc,
                        ptr::null(),
                        ptr::null_mut(),
                        out,
                    )
                })?;
                check(Factory4_MakeWindowAssociation(
                    self.factory.0,
                    surface.window,
                    2,
                ))?;
                *current = Some(Swapchain {
                    object: swap.query(&IDXGISwapChain3)?,
                    buffers: Vec::new(),
                    format: config.format,
                    width: config.width,
                    height: config.height,
                });
            }
            let swap = current.as_mut().expect("swap chain configured");
            for index in 0..3 {
                swap.buffers.push(Com::create(|out| {
                    SwapChain3_GetBuffer(swap.object.0, index, &ID3D12Resource, out)
                })?);
            }
        }
        self.acquired.set(false);
        Ok(())
    }

    pub(crate) fn acquire(
        &self,
        surface: &Surface,
    ) -> std::result::Result<(Texture, bool), CurrentSurfaceTexture> {
        if self.acquired.get() {
            return Err(CurrentSurfaceTexture::Validation);
        }
        self.wait(&mut self.frames.borrow_mut()[self.slot.get()])
            .map_err(|_| CurrentSurfaceTexture::Lost)?;
        let swap = surface.swap.borrow();
        let swap = swap.as_ref().ok_or(CurrentSurfaceTexture::Validation)?;
        unsafe {
            let status = SwapChain3_Present(swap.object.0, 0, 1);
            if status == 0x087a0001 {
                return Err(CurrentSurfaceTexture::Occluded);
            }
            if status < 0 {
                return Err(CurrentSurfaceTexture::Lost);
            }
            let index = SwapChain3_GetCurrentBackBufferIndex(swap.object.0);
            self.acquired.set(true);
            Ok((
                Texture {
                    object: swap.buffers[index as usize].clone(),
                    state: Cell::new(0),
                    format: swap.format,
                    mips: 1,
                    index: Some(index),
                    view: None,
                },
                false,
            ))
        }
    }

    pub(crate) fn present(&self, surface: &Surface, texture: &Texture) -> Result<()> {
        require(
            self.acquired.replace(false),
            "No acquired D3D12 surface texture",
        );
        let swap = surface.swap.borrow();
        let swap = swap
            .as_ref()
            .ok_or_else(|| Error("Surface not configured".into()))?;
        unsafe {
            require(
                texture.index == Some(SwapChain3_GetCurrentBackBufferIndex(swap.object.0)),
                "Wrong swapchain buffer",
            );
            check(SwapChain3_Present(swap.object.0, 1, 0))?;
        }
        self.slot.set((self.slot.get() + 1) % 2);
        Ok(())
    }
}
