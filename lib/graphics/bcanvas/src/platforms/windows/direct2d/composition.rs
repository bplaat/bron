/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A Direct2D target for a native window layer: a premultiplied composition swap chain of a
//! Direct3D 11 device, which is the content of the DirectComposition visual of the layer and
//! is drawn by a Direct2D device context. The vtable layouts follow `d3d11.h`, `dxgi.h`,
//! `dxgi1_2.h`, `d2d1_1.h` and `dcomp.h`.

#![allow(non_snake_case)]

use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::rc::Rc;

use super::context::{Context, DrawTarget, Resources};
use super::headers::*;
use crate::platforms::windows::com::{ComInterface, ComPtr};
use crate::platforms::windows::headers::*;

// SAFETY: This is the SDK IID for IDXGIDevice.
unsafe impl ComInterface for IDXGIDevice {
    const IID: GUID = GUID {
        data1: 0x54ec_77fa,
        data2: 0x1377,
        data3: 0x44e6,
        data4: [0x8c, 0x32, 0x88, 0xfd, 0x5f, 0x44, 0xc8, 0x4c],
    };
}
const IID_IDXGI_FACTORY2: GUID = GUID {
    data1: 0x50c8_3a1c,
    data2: 0xe072,
    data3: 0x4c48,
    data4: [0x87, 0xb0, 0x36, 0x30, 0xfa, 0x36, 0xa6, 0xd0],
};
const IID_IDXGI_SURFACE: GUID = GUID {
    data1: 0xcafc_b56c,
    data2: 0x6ac3,
    data3: 0x4889,
    data4: [0xbf, 0x47, 0x9e, 0x23, 0xbb, 0xd2, 0x60, 0xec],
};

const D3D_DRIVER_TYPE_HARDWARE: u32 = 1;
const D3D_DRIVER_TYPE_WARP: u32 = 5;
const D3D11_CREATE_DEVICE_BGRA_SUPPORT: u32 = 0x20;
const D3D11_SDK_VERSION: u32 = 7;
const DXGI_USAGE_RENDER_TARGET_OUTPUT: u32 = 0x20;
const DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL: u32 = 3;
const DXGI_ALPHA_MODE_PREMULTIPLIED: u32 = 1;
const D2D1_BITMAP_OPTIONS_TARGET_CANNOT_DRAW: u32 = 0x1 | 0x2;

#[repr(C)]
struct ID3D11Device {
    vtbl: *const IUnknownVtbl,
}

#[repr(C)]
struct IDXGIObjectVtbl {
    base: IUnknownVtbl,
    _unused_3_5: [*const c_void; 3],
    GetParent: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32,
}

#[repr(C)]
struct IDXGIAdapter {
    vtbl: *const IDXGIObjectVtbl,
}

#[repr(C)]
struct IDXGIDevice {
    vtbl: *const IDXGIDeviceVtbl,
}

#[repr(C)]
struct IDXGIDeviceVtbl {
    base: IDXGIObjectVtbl,
    GetAdapter: unsafe extern "system" fn(*mut IDXGIDevice, *mut *mut IDXGIAdapter) -> i32,
}

#[repr(C)]
struct IDXGIFactory2 {
    vtbl: *const IDXGIFactory2Vtbl,
}

#[repr(C)]
struct IDXGIFactory2Vtbl {
    base: IDXGIObjectVtbl,
    _unused_7_23: [*const c_void; 17],
    CreateSwapChainForComposition: unsafe extern "system" fn(
        *mut IDXGIFactory2,
        *mut ID3D11Device,
        *const SwapChainDesc1,
        *mut c_void,
        *mut *mut IDXGISwapChain1,
    ) -> i32,
}

#[repr(C)]
struct IDXGISwapChain1 {
    vtbl: *const IDXGISwapChainVtbl,
}

#[repr(C)]
struct IDXGISwapChainVtbl {
    base: IDXGIObjectVtbl,
    _unused_7: *const c_void,
    Present: unsafe extern "system" fn(*mut IDXGISwapChain1, u32, u32) -> i32,
    GetBuffer:
        unsafe extern "system" fn(*mut IDXGISwapChain1, u32, *const GUID, *mut *mut c_void) -> i32,
    _unused_10_12: [*const c_void; 3],
    ResizeBuffers: unsafe extern "system" fn(*mut IDXGISwapChain1, u32, u32, u32, u32, u32) -> i32,
}

#[repr(C)]
struct ID2D1Factory1 {
    vtbl: *const ID2D1Factory1Vtbl,
}

// SAFETY: This is the SDK IID for ID2D1Factory1.
unsafe impl ComInterface for ID2D1Factory1 {
    const IID: GUID = IID_ID2D1_FACTORY1;
}

#[repr(C)]
struct ID2D1Factory1Vtbl {
    base: ID2D1FactoryVtbl,
    CreateDevice: unsafe extern "system" fn(
        *mut ID2D1Factory1,
        *mut IDXGIDevice,
        *mut *mut ID2D1Device,
    ) -> i32,
}

#[repr(C)]
struct ID2D1Device {
    vtbl: *const ID2D1DeviceVtbl,
}

#[repr(C)]
struct ID2D1DeviceVtbl {
    base: IUnknownVtbl,
    _unused_3: *const c_void,
    CreateDeviceContext:
        unsafe extern "system" fn(*mut ID2D1Device, u32, *mut *mut ID2D1DeviceContext) -> i32,
}

#[repr(C)]
struct IDCompositionDevice2 {
    vtbl: *const IDCompositionDevice2Vtbl,
}

#[repr(C)]
struct IDCompositionDevice2Vtbl {
    base: IUnknownVtbl,
    Commit: unsafe extern "system" fn(*mut IDCompositionDevice2) -> i32,
}

#[repr(C)]
struct IDCompositionVisual {
    vtbl: *const IDCompositionVisualVtbl,
}

#[repr(C)]
struct IDCompositionVisualVtbl {
    base: IUnknownVtbl,
    _unused_3_14: [*const c_void; 12],
    SetContent: unsafe extern "system" fn(*mut IDCompositionVisual, *mut c_void) -> i32,
}

#[repr(C)]
struct SwapChainDesc1 {
    width: u32,
    height: u32,
    format: u32,
    stereo: i32,
    sample_count: u32,
    sample_quality: u32,
    buffer_usage: u32,
    buffer_count: u32,
    scaling: u32,
    swap_effect: u32,
    alpha_mode: u32,
    flags: u32,
}

type D3D11CreateDeviceFn = unsafe extern "system" fn(
    *mut c_void,
    u32,
    *mut c_void,
    u32,
    *const u32,
    u32,
    u32,
    *mut *mut c_void,
    *mut u32,
    *mut *mut c_void,
) -> i32;

// The Direct3D 11 device creation function, loaded where Direct2D is.
fn create_device() -> Option<D3D11CreateDeviceFn> {
    #[cfg(not(target_vendor = "winxp"))]
    {
        #[link(name = "d3d11")]
        unsafe extern "system" {
            fn D3D11CreateDevice(
                adapter: *mut c_void,
                driver_type: u32,
                software: *mut c_void,
                flags: u32,
                levels: *const u32,
                level_count: u32,
                sdk_version: u32,
                device: *mut *mut c_void,
                level: *mut u32,
                context: *mut *mut c_void,
            ) -> i32;
        }
        Some(D3D11CreateDevice)
    }
    #[cfg(target_vendor = "winxp")]
    unsafe {
        let library = load_system_library("d3d11.dll");
        let function = if library.is_null() {
            null()
        } else {
            GetProcAddress(library, c"D3D11CreateDevice".as_ptr())
        };
        (!function.is_null())
            .then(|| std::mem::transmute::<*const c_void, D3D11CreateDeviceFn>(function))
    }
}

/// A composition swap chain drawn by Direct2D.
pub(crate) struct CompositionTarget {
    resources: Rc<Resources>,
    context: DrawTarget<ID2D1DeviceContext>,
    swap_chain: ComPtr<IDXGISwapChain1>,
    _d2d_device: ComPtr<ID2D1Device>,
    _d3d_device: ComPtr<ID3D11Device>,
    size: (u32, u32),
    drawing: bool,
}

impl CompositionTarget {
    /// Create a swap chain as the content of a layer visual, none when the device fails.
    pub(crate) unsafe fn new(
        composition: *mut c_void,
        visual: *mut c_void,
        width: u32,
        height: u32,
    ) -> Option<Self> {
        let (width, height) = (width.max(1), height.max(1));
        unsafe {
            let create = create_device()?;
            let d3d = [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP]
                .into_iter()
                .find_map(|driver| {
                    ComPtr::create(|out: *mut *mut ID3D11Device| {
                        create(
                            null_mut(),
                            driver,
                            null_mut(),
                            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                            null(),
                            0,
                            D3D11_SDK_VERSION,
                            out.cast(),
                            null_mut(),
                            null_mut(),
                        )
                    })
                    .ok()
                })?;
            let dxgi = d3d.query::<IDXGIDevice>().ok()?;
            // The swap chain comes from the factory of the adapter of the device.
            let adapter =
                ComPtr::create(|out| ((*dxgi.vtbl).GetAdapter)(dxgi.as_ptr(), out)).ok()?;
            let factory = ComPtr::create(|out: *mut *mut IDXGIFactory2| {
                ((*adapter.vtbl).GetParent)(
                    adapter.as_ptr().cast(),
                    &IID_IDXGI_FACTORY2,
                    out.cast(),
                )
            })
            .ok()?;
            let desc = SwapChainDesc1 {
                width,
                height,
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                stereo: 0,
                sample_count: 1,
                sample_quality: 0,
                buffer_usage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                buffer_count: 2,
                scaling: 0, // DXGI_SCALING_STRETCH
                swap_effect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                alpha_mode: DXGI_ALPHA_MODE_PREMULTIPLIED,
                flags: 0,
            };
            let swap_chain = ComPtr::create(|out| {
                ((*factory.vtbl).CreateSwapChainForComposition)(
                    factory.as_ptr(),
                    d3d.as_ptr(),
                    &desc,
                    null_mut(),
                    out,
                )
            })
            .ok()?;
            // The device context draws with the factory of the canvas resources.
            let resources = Resources::new();
            let factory = resources.factory.query::<ID2D1Factory1>().ok()?;
            let d2d_device = ComPtr::create(|out| {
                ((*factory.vtbl).CreateDevice)(factory.as_ptr(), dxgi.as_ptr(), out)
            })
            .ok()?;
            let context = ComPtr::create(|out| {
                ((*d2d_device.vtbl).CreateDeviceContext)(d2d_device.as_ptr(), 0, out)
            })
            .ok()?;
            let context = DrawTarget::new(context).ok()?;
            let visual = visual.cast::<IDCompositionVisual>();
            if ((*(*visual).vtbl).SetContent)(visual, swap_chain.as_ptr().cast()) < 0 {
                return None;
            }
            let composition = composition.cast::<IDCompositionDevice2>();
            ((*(*composition).vtbl).Commit)(composition);
            Some(Self {
                resources,
                context,
                swap_chain,
                _d2d_device: d2d_device,
                _d3d_device: d3d,
                size: (width, height),
                drawing: false,
            })
        }
    }

    /// Begin drawing a frame of the size of the layer, none when the device failed.
    pub(crate) fn begin(&mut self, width: u32, height: u32, scale: f32) -> Option<Context> {
        let (width, height) = (width.max(1), height.max(1));
        let native = &self.context.native;
        let swap_chain = self.swap_chain.as_ptr();
        unsafe {
            let vtbl = &*self.swap_chain.vtbl;
            if self.size != (width, height) {
                native.SetTarget(null_mut());
                if (vtbl.ResizeBuffers)(swap_chain, 0, width, height, 0, 0) < 0 {
                    return None;
                }
                self.size = (width, height);
            }
            let surface = ComPtr::create(|out: *mut *mut IDXGISurface| {
                (vtbl.GetBuffer)(swap_chain, 0, &IID_IDXGI_SURFACE, out.cast())
            })
            .ok()?;
            let dpi = 96.0 * scale;
            let properties = BitmapProperties1 {
                pixel_format: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpi_x: dpi,
                dpi_y: dpi,
                options: D2D1_BITMAP_OPTIONS_TARGET_CANNOT_DRAW,
                color_context: null_mut(),
            };
            let bitmap = native
                .CreateBitmapFromDxgiSurface(surface.as_ptr(), &properties)
                .ok()?;
            native.SetTarget(bitmap.as_ptr().cast());
            self.context.native.SetDpi(dpi, dpi);
            self.context.native.BeginDraw();
            // Every frame starts transparent, the layers below show through.
            self.context.native.Clear(&D2D1_COLOR_F::default());
            self.drawing = true;
        }
        Some(Context::new(&self.context, self.resources.clone()))
    }

    /// End the frame after its context is dropped and present it when drawn, false when the
    /// device was lost.
    pub(crate) fn end(&mut self, present: bool) -> bool {
        if !std::mem::take(&mut self.drawing) {
            return true;
        }
        let native = &self.context.native;
        unsafe {
            let drawn = native.EndDraw(null_mut(), null_mut()).is_ok();
            native.SetTarget(null_mut());
            // The desktop compositor shows the frame at its next frame, without waiting here.
            drawn
                && (!present
                    || ((*self.swap_chain.vtbl).Present)(self.swap_chain.as_ptr(), 0, 0) >= 0)
        }
    }
}
