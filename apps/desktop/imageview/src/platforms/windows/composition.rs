/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The media on the GPU: a sprite visual inside the media element, which the scroll viewer zooms
//! and pans on the compositor. Its brush shows a composition surface of premultiplied BGRA
//! pixels, uploaded once with Direct3D, sampled nearest neighbor for enlarged raster pixels and
//! linearly otherwise. The interop layouts follow `Microsoft.UI.Composition.Interop.h`
//! and `d3d11.h`.

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
// SAFETY: This is the SDK IID for IVisual.
unsafe impl ComInterface for IVisual {
    const IID: GUID = GUID::from_u128(0xc0eeab6c_c897_5ac6_a1c9_63abd5055b9b);
}
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
pub(super) struct Vector2 {
    x: f32,
    y: f32,
}

/// `POINT`
#[repr(C)]
#[derive(Default)]
pub(super) struct POINT {
    x: i32,
    y: i32,
}

/// `D3D11_BOX`
#[repr(C)]
pub(super) struct TextureBox {
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
    graphics: ComPtr<ICompositionGraphicsDevice>,
    context: ComPtr<ID3D11DeviceContext>,
    visual: ComPtr<IVisual>,
    brush: ComPtr<ICompositionSurfaceBrush>,
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
        // SAFETY: The element and compositor are used on their XAML thread. Each
        // requested interface uses its IID or the documented return type, and the
        // owned pointers keep the device, visual, and brush alive through the calls.
        unsafe {
            let composition_object = out(|raw| {
                call!(
                    statics,
                    IElementCompositionPreviewStatics::GetElementVisual,
                    element.as_raw(),
                    raw
                )
            })?
            .cast(&IID_ICOMPOSITION_OBJECT)?;
            let compositor =
                out(|raw| call!(composition_object, ICompositionObject::GetCompositor, raw))?;
            let mut device = null_mut();
            call!(context, ID3D11DeviceChild::GetDevice, &mut device);
            let device = ComPtr::from_raw(device).ok_or(E_POINTER)?;
            let interop = compositor.cast(&IID_ICOMPOSITOR_INTEROP)?;
            let graphics = ComPtr::create(|raw| {
                call!(
                    interop,
                    ICompositorInterop::CreateGraphicsDevice,
                    device.as_raw(),
                    raw
                )
            })?;
            let sprite = out(|raw| call!(compositor, ICompositor::CreateSpriteVisual, raw))?;
            let brush = ComPtr::<ICompositionSurfaceBrush>::create(|raw| {
                call!(compositor, ICompositor::CreateSurfaceBrush, raw)
            })?;
            check(call!(
                sprite,
                ISpriteVisual::SetBrush,
                brush.cast(&IID_ICOMPOSITION_BRUSH)?.as_raw()
            ))?;
            let visual = sprite.query::<IVisual>()?;
            check(call!(
                statics,
                IElementCompositionPreviewStatics::SetElementChildVisual,
                element.as_raw(),
                visual.as_raw()
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
        // SAFETY: The graphics device retains ICompositionGraphicsDevice and out supplies writable storage.
        let surface = out(|raw| unsafe {
            call!(
                self.graphics,
                ICompositionGraphicsDevice::CreateDrawingSurface,
                size,
                PIXEL_FORMAT_BGRA,
                ALPHA_MODE_PREMULTIPLIED,
                raw
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
        // SAFETY: The queried interop returns an owned texture and the offset of the
        // drawing region. The byte count was checked for that region, and EndDraw
        // completes the update before the surface is used again.
        unsafe {
            let texture = out(|raw| {
                call!(
                    interop,
                    ICompositionDrawingSurfaceInterop::BeginDraw,
                    null::<c_void>(),
                    &IID_ID3D11_TEXTURE_2D,
                    raw,
                    &mut offset
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
            call!(
                self.context,
                ID3D11DeviceContext::UpdateSubresource,
                texture.as_raw(),
                0,
                &area,
                pixels.as_ptr().cast(),
                width * 4,
                0,
            );
            drop(texture);
            check(call!(interop, ICompositionDrawingSurfaceInterop::EndDraw))
        }
    }

    /// Shows a surface stretched to a size in logical pixels.
    pub(crate) fn show(&self, surface: &Surface, (width, height): (f64, f64)) {
        let size = Vector2 {
            x: width as f32,
            y: height as f32,
        };
        // SAFETY: The visual, brush, and brush surface retain their matching interfaces
        // through the calls on this XAML thread.
        unsafe {
            _ = check(call!(
                self.brush,
                ICompositionSurfaceBrush::SetSurface,
                surface.surface.as_raw()
            ));
            call!(self.visual, IVisual::SetSize, size);
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
        // SAFETY: The brush owns ICompositionSurfaceBrush and mode is a valid interpolation enum.
        unsafe {
            call!(
                self.brush,
                ICompositionSurfaceBrush::SetBitmapInterpolationMode,
                mode
            )
        };
    }

    #[cfg(all(test, feature = "edit"))]
    pub(crate) fn is_pixelated(&self) -> bool {
        self.pixelated.get() == Some(true)
    }
}

// The immediate context of a new Direct3D device for composition, on the GPU or else in
// software.
fn d3d11_context() -> Result<ComPtr<ID3D11DeviceContext>> {
    let mut result = E_POINTER;
    for driver in [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP] {
        // SAFETY: The device is released again, the context keeps it alive.
        let context = unsafe {
            ComPtr::create(|raw| {
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
            })
        };
        match context {
            Ok(context) => {
                // The compositor can use the device from its own thread too.
                if let Ok(multithread) = context.cast(&IID_ID3D11_MULTITHREAD) {
                    // SAFETY: The context was queried for ID3D11Multithread.
                    unsafe {
                        call!(
                            multithread,
                            ID3D11Multithread::SetMultithreadProtected,
                            1i32
                        )
                    };
                }
                return Ok(context);
            }
            Err(error) => result = error,
        }
    }
    Err(result)
}

interface! {
    ICompositionDrawingSurfaceInterop, ICompositionDrawingSurfaceInteropVtbl: IUnknownVtbl {
        BeginDraw: unsafe extern "system" fn(*mut ICompositionDrawingSurfaceInterop, *const c_void, *const GUID, *mut *mut c_void, *mut POINT) -> HRESULT,
        EndDraw: unsafe extern "system" fn(*mut ICompositionDrawingSurfaceInterop) -> HRESULT,
    }
}

interface! {
    ICompositionGraphicsDevice, ICompositionGraphicsDeviceVtbl: IInspectableVtbl {
        CreateDrawingSurface: unsafe extern "system" fn(*mut ICompositionGraphicsDevice, Vector2, i32, i32, *mut *mut c_void) -> HRESULT,
    }
}

interface! {
    ICompositionObject, ICompositionObjectVtbl: IInspectableVtbl {
        GetCompositor: unsafe extern "system" fn(*mut ICompositionObject, *mut *mut c_void) -> HRESULT,
    }
}

interface! {
    ICompositionSurfaceBrush, ICompositionSurfaceBrushVtbl: IInspectableVtbl {
        _unused_6: [*const c_void; 1],
        SetBitmapInterpolationMode: unsafe extern "system" fn(*mut ICompositionSurfaceBrush, i32) -> HRESULT,
        _unused_8_12: [*const c_void; 5],
        SetSurface: unsafe extern "system" fn(*mut ICompositionSurfaceBrush, *mut c_void) -> HRESULT,
    }
}

interface! {
    ICompositor, ICompositorVtbl: IInspectableVtbl {
        _unused_6_21: [*const c_void; 16],
        CreateSpriteVisual: unsafe extern "system" fn(*mut ICompositor, *mut *mut c_void) -> HRESULT,
        CreateSurfaceBrush: unsafe extern "system" fn(*mut ICompositor, *mut *mut c_void) -> HRESULT,
    }
}

interface! {
    ICompositorInterop, ICompositorInteropVtbl: IUnknownVtbl {
        CreateGraphicsDevice: unsafe extern "system" fn(*mut ICompositorInterop, *mut c_void, *mut *mut c_void) -> HRESULT,
    }
}

interface! {
    ID3D11DeviceContext, ID3D11DeviceContextVtbl: ID3D11DeviceChildVtbl {
        _unused_7_47: [*const c_void; 41],
        UpdateSubresource: unsafe extern "system" fn(*mut ID3D11DeviceContext, *mut c_void, u32, *const TextureBox, *const c_void, u32, u32),
    }
}

interface! {
    ID3D11Multithread, ID3D11MultithreadVtbl: IUnknownVtbl {
        _unused_3_4: [*const c_void; 2],
        SetMultithreadProtected: unsafe extern "system" fn(*mut ID3D11Multithread, i32) -> HRESULT,
    }
}

interface! {
    IElementCompositionPreviewStatics, IElementCompositionPreviewStaticsVtbl: IInspectableVtbl {
        GetElementVisual: unsafe extern "system" fn(*mut IElementCompositionPreviewStatics, *mut c_void, *mut *mut c_void) -> HRESULT,
        _unused_7: [*const c_void; 1],
        SetElementChildVisual: unsafe extern "system" fn(*mut IElementCompositionPreviewStatics, *mut c_void, *mut c_void) -> HRESULT,
    }
}

interface! {
    ISpriteVisual, ISpriteVisualVtbl: IInspectableVtbl {
        _unused_6: [*const c_void; 1],
        SetBrush: unsafe extern "system" fn(*mut ISpriteVisual, *mut c_void) -> HRESULT,
    }
}

interface! {
    IVisual, IVisualVtbl: IInspectableVtbl {
        _unused_6_35: [*const c_void; 30],
        SetSize: unsafe extern "system" fn(*mut IVisual, Vector2) -> HRESULT,
    }
}

interface! {
    ID3D11DeviceChild, ID3D11DeviceChildVtbl: IUnknownVtbl {
        GetDevice: unsafe extern "system" fn(*mut ID3D11DeviceChild, *mut *mut c_void),
        _unused_4_6: [*const c_void; 3],
    }
}
