/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The media on the GPU: a sprite visual inside the media element, which the scroll viewer zooms
//! and pans on the compositor. Its brush shows a composition surface of premultiplied BGRA
//! pixels, uploaded once with Direct3D, sampled nearest neighbor for enlarged raster pixels and
//! linearly otherwise. Interop slots are listed in `Microsoft.UI.Composition.Interop.h` after the
//! three `IUnknown` slots, D3D11 slots in `d3d11.h`.

#![allow(non_snake_case, clippy::upper_case_acronyms)]

use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::{null, null_mut};

use super::headers::w;
use super::winrt::*;

const IID_IELEMENT_COMPOSITION_PREVIEW_STATICS: GUID =
    GUID::from_u128(0x84da5a6c_0cfa_532b_9b15_ccd986374342);
const IID_ICOMPOSITION_OBJECT: GUID = GUID::from_u128(0x0e583d49_fb5e_5481_a426_d3c41e059a5a);
const IID_ICOMPOSITION_BRUSH: GUID = GUID::from_u128(0x483924e7_99a5_5377_968b_dec6d40bbccd);
const IID_ICOMPOSITION_SURFACE: GUID = GUID::from_u128(0x9ec612c3_a5d2_4f97_9df3_6b49ce736215);
const IID_IVISUAL: GUID = GUID::from_u128(0xc0eeab6c_c897_5ac6_a1c9_63abd5055b9b);
const IID_ICOMPOSITOR_INTEROP: GUID = GUID::from_u128(0xfab19398_6d19_4d8a_b752_8f096c396069);
const IID_ICOMPOSITION_DRAWING_SURFACE_INTEROP: GUID =
    GUID::from_u128(0x2d6355c2_ad57_4eae_92e4_4c3eff65d578);
const IID_ID3D11_TEXTURE_2D: GUID = GUID::from_u128(0x6f15aaf2_d208_4e89_9ab4_489535d34f9c);
const IID_ID3D11_MULTITHREAD: GUID = GUID::from_u128(0x9b7e4e00_342c_4106_a19f_4f2704f689f0);

const D3D_DRIVER_TYPE_HARDWARE: i32 = 1;
const D3D_DRIVER_TYPE_WARP: i32 = 5;
const D3D11_CREATE_DEVICE_BGRA_SUPPORT: u32 = 0x20;
const D3D11_SDK_VERSION: u32 = 7;
// DirectXPixelFormat.B8G8R8A8UIntNormalized and DirectXAlphaMode.Premultiplied.
const PIXEL_FORMAT_BGRA: i32 = 87;
const ALPHA_MODE_PREMULTIPLIED: i32 = 1;
// CompositionBitmapInterpolationMode.NearestNeighbor and Linear.
const INTERPOLATION_NEAREST: i32 = 0;
const INTERPOLATION_LINEAR: i32 = 1;

/// `Windows.Foundation.Numerics.Vector2` and `Windows.Foundation.Size`
#[repr(C)]
struct Vector2 {
    x: f32,
    y: f32,
}

/// `POINT`
#[repr(C)]
#[derive(Default)]
struct POINT {
    x: i32,
    y: i32,
}

/// `D3D11_BOX`
#[repr(C)]
struct TextureBox {
    left: u32,
    top: u32,
    front: u32,
    right: u32,
    bottom: u32,
    back: u32,
}

#[link(name = "d3d11")]
unsafe extern "system" {
    fn D3D11CreateDevice(
        adapter: *mut c_void,
        driver_type: i32,
        software: *mut c_void,
        flags: u32,
        feature_levels: *const i32,
        feature_level_count: u32,
        sdk_version: u32,
        device: *mut *mut c_void,
        feature_level: *mut i32,
        context: *mut *mut c_void,
    ) -> HRESULT;
}

/// Pixels of the media on the GPU.
pub(crate) struct Surface {
    surface: ComPtr,
    size: (u32, u32),
}

/// The visual that shows a surface stretched over the media element.
pub(crate) struct MediaVisual {
    graphics: ComPtr,
    context: ComPtr,
    visual: ComPtr,
    brush: ComPtr,
    // Whether the brush samples nearest neighbor, once set.
    pixelated: Cell<Option<bool>>,
}

impl MediaVisual {
    /// Puts a visual inside a XAML element, drawn over its content.
    pub(crate) fn new(element: &ComPtr) -> Result<Self> {
        let element = element.cast(&IID_IUI_ELEMENT)?;
        let statics = factory(
            w!("Microsoft.UI.Xaml.Hosting.ElementCompositionPreview"),
            &IID_IELEMENT_COMPOSITION_PREVIEW_STATICS,
        )?;
        let context = d3d11_context()?;
        // SAFETY: Slot 6 of IElementCompositionPreviewStatics is GetElementVisual and slot 8
        // SetElementChildVisual, slot 6 of ICompositionObject is get_Compositor, slot 3 of
        // ICompositorInterop is CreateGraphicsDevice and slot 3 of ID3D11DeviceChild is
        // GetDevice, slots 22 and 23 of ICompositor are CreateSpriteVisual and CreateSurfaceBrush
        // and slot 7 of ISpriteVisual is put_Brush.
        unsafe {
            let compositor = out(
                |raw| call!(statics, 6, element.as_raw() => *mut c_void, raw => *mut *mut c_void),
            )?
            .cast(&IID_ICOMPOSITION_OBJECT)?
            .get(6)?;
            let mut device = null_mut();
            context.method::<unsafe extern "system" fn(*mut c_void, *mut *mut c_void)>(3)(
                context.as_raw(),
                &mut device,
            );
            let device = ComPtr::from_raw(device).ok_or(E_POINTER)?;
            let interop = compositor.cast(&IID_ICOMPOSITOR_INTEROP)?;
            let graphics = out(
                |raw| call!(interop, 3, device.as_raw() => *mut c_void, raw => *mut *mut c_void),
            )?;
            let sprite = compositor.get(22)?;
            let brush = compositor.get(23)?;
            sprite.put(7, brush.cast(&IID_ICOMPOSITION_BRUSH)?.as_raw())?;
            let visual = sprite.cast(&IID_IVISUAL)?;
            check(call!(
                statics,
                8,
                element.as_raw() => *mut c_void,
                visual.as_raw() => *mut c_void,
            ))?;
            Ok(Self {
                graphics,
                context,
                visual,
                brush,
                pixelated: Cell::new(None),
            })
        }
    }

    /// Creates a transparent surface of a size in pixels.
    pub(crate) fn surface(&self, (width, height): (u32, u32)) -> Result<Surface> {
        let size = Vector2 {
            x: width as f32,
            y: height as f32,
        };
        // SAFETY: Slot 6 of ICompositionGraphicsDevice is CreateDrawingSurface.
        let surface = out(|raw| unsafe {
            call!(
                self.graphics,
                6,
                size => Vector2,
                PIXEL_FORMAT_BGRA => i32,
                ALPHA_MODE_PREMULTIPLIED => i32,
                raw => *mut *mut c_void,
            )
        })?;
        Ok(Surface {
            surface: surface.cast(&IID_ICOMPOSITION_SURFACE)?,
            size: (width, height),
        })
    }

    /// Replaces the pixels of a surface, which are as many as it has.
    pub(crate) fn upload(&self, surface: &Surface, pixels: &[u8]) -> Result<()> {
        let (width, height) = surface.size;
        assert_eq!(
            pixels.len(),
            width as usize * height as usize * 4,
            "surface pixels have the wrong size"
        );
        let interop = surface
            .surface
            .cast(&IID_ICOMPOSITION_DRAWING_SURFACE_INTEROP)?;
        let mut offset = POINT::default();
        // SAFETY: Slot 3 of ICompositionDrawingSurfaceInterop is BeginDraw, which gives the
        // texture the surface is a part of at an offset, slot 4 is EndDraw. Slot 48 of
        // ID3D11DeviceContext is UpdateSubresource.
        unsafe {
            let texture = out(|raw| {
                call!(
                    interop,
                    3,
                    null::<c_void>() => *const c_void,
                    &IID_ID3D11_TEXTURE_2D => *const GUID,
                    raw => *mut *mut c_void,
                    &mut offset => *mut POINT,
                )
            })?;
            let (x, y) = (offset.x as u32, offset.y as u32);
            let area = TextureBox {
                left: x,
                top: y,
                front: 0,
                right: x + width,
                bottom: y + height,
                back: 1,
            };
            self.context.method::<unsafe extern "system" fn(
                *mut c_void,
                *mut c_void,
                u32,
                *const TextureBox,
                *const c_void,
                u32,
                u32,
            )>(48)(
                self.context.as_raw(),
                texture.as_raw(),
                0,
                &area,
                pixels.as_ptr().cast(),
                width * 4,
                0,
            );
            drop(texture);
            check(call!(interop, 4))
        }
    }

    /// Shows a surface stretched to a size in logical pixels.
    pub(crate) fn show(&self, surface: &Surface, (width, height): (f64, f64)) {
        let size = Vector2 {
            x: width as f32,
            y: height as f32,
        };
        // SAFETY: Slot 13 of ICompositionSurfaceBrush is put_Surface, slot 36 of IVisual is
        // put_Size.
        unsafe {
            _ = self.brush.put(13, surface.surface.as_raw());
            call!(self.visual, 36, size => Vector2);
        }
    }

    /// Samples the surface nearest neighbor for sharp enlarged pixels, or linearly.
    pub(crate) fn set_pixelated(&self, pixelated: bool) {
        if self.pixelated.replace(Some(pixelated)) == Some(pixelated) {
            return;
        }
        let mode = if pixelated {
            INTERPOLATION_NEAREST
        } else {
            INTERPOLATION_LINEAR
        };
        // SAFETY: Slot 7 of ICompositionSurfaceBrush is put_BitmapInterpolationMode.
        unsafe { call!(self.brush, 7, mode => i32) };
    }

    #[cfg(test)]
    pub(crate) fn is_pixelated(&self) -> bool {
        self.pixelated.get() == Some(true)
    }
}

// The immediate context of a new Direct3D device for composition, on the GPU or else in
// software.
fn d3d11_context() -> Result<ComPtr> {
    let mut result = E_POINTER;
    for driver in [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP] {
        // SAFETY: The device is released again, the context keeps it alive.
        let context = out(|raw| unsafe {
            let mut device = null_mut();
            let result = D3D11CreateDevice(
                null_mut(),
                driver,
                null_mut(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                null(),
                0,
                D3D11_SDK_VERSION,
                &mut device,
                null_mut(),
                raw,
            );
            drop(ComPtr::from_raw(device));
            result
        });
        match context {
            Ok(context) => {
                // The compositor can use the device from its own thread too.
                if let Ok(multithread) = context.cast(&IID_ID3D11_MULTITHREAD) {
                    // SAFETY: Slot 5 of ID3D11Multithread is SetMultithreadProtected.
                    unsafe { call!(multithread, 5, 1i32 => i32) };
                }
                return Ok(context);
            }
            Err(error) => result = error,
        }
    }
    Err(result)
}
