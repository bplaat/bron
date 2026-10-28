/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// Minimal Direct2D 1.0 and DirectWrite COM ABI. Unused inherited methods retain
// their SDK vtable slots; only the methods used by the canvas are callable.
#![allow(non_camel_case_types, non_snake_case)]
#![allow(clippy::too_many_arguments, clippy::upper_case_acronyms)]

use std::ffi::c_void;
use std::ops::Deref;

use super::com::{ComPtr, check};

pub(super) type BOOL = i32;
pub(super) type HANDLE = *mut c_void;
pub(super) type HDC = HANDLE;
pub(super) type HWND = HANDLE;
pub(super) type HRESULT = i32;

#[repr(C)]
#[derive(Default, Clone)]
pub(super) struct RECT {
    pub(super) left: i32,
    pub(super) top: i32,
    pub(super) right: i32,
    pub(super) bottom: i32,
}

#[repr(C)]
#[derive(PartialEq, Eq)]
pub(super) struct GUID {
    pub(super) data1: u32,
    pub(super) data2: u16,
    pub(super) data3: u16,
    pub(super) data4: [u8; 8],
}

/// Encode a string as a null-terminated UTF-16 string.
pub(super) fn wide_string(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

#[repr(C)]
pub(super) struct IUnknownVtbl {
    pub(super) QueryInterface:
        unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    pub(super) AddRef: unsafe extern "system" fn(*mut c_void) -> u32,
    pub(super) Release: unsafe extern "system" fn(*mut c_void) -> u32,
}

#[repr(C)]
pub(super) struct ID2D1Factory {
    lpVtbl: *const ID2D1FactoryVtbl,
}

#[repr(C)]
struct ID2D1FactoryVtbl {
    base: IUnknownVtbl,
    _unused_3_9: [*const c_void; 7],
    CreatePathGeometry:
        unsafe extern "system" fn(*mut ID2D1Factory, *mut *mut ID2D1PathGeometry) -> i32,
    CreateStrokeStyle: unsafe extern "system" fn(
        *mut ID2D1Factory,
        *const D2D1_STROKE_STYLE_PROPERTIES,
        *const f32,
        u32,
        *mut *mut ID2D1StrokeStyle,
    ) -> i32,
    _unused_12_13: [*const c_void; 2],
    CreateHwndRenderTarget: unsafe extern "system" fn(
        *mut ID2D1Factory,
        *const D2D1_RENDER_TARGET_PROPERTIES,
        *const D2D1_HWND_RENDER_TARGET_PROPERTIES,
        *mut *mut ID2D1HwndRenderTarget,
    ) -> i32,
    _unused_15_15: [*const c_void; 1],
    CreateDCRenderTarget: unsafe extern "system" fn(
        *mut ID2D1Factory,
        *const D2D1_RENDER_TARGET_PROPERTIES,
        *mut *mut ID2D1DCRenderTarget,
    ) -> i32,
}

impl ID2D1Factory {
    pub(super) unsafe fn CreatePathGeometry(&self) -> Result<ComPtr<ID2D1PathGeometry>, i32> {
        unsafe {
            ComPtr::create(|out| {
                ((*self.lpVtbl).CreatePathGeometry)(self as *const Self as *mut Self, out)
            })
        }
    }
    pub(super) unsafe fn CreateStrokeStyle(
        &self,
        properties: *const D2D1_STROKE_STYLE_PROPERTIES,
        dashes: *const f32,
        count: u32,
    ) -> Result<ComPtr<ID2D1StrokeStyle>, i32> {
        unsafe {
            ComPtr::create(|out| {
                ((*self.lpVtbl).CreateStrokeStyle)(
                    self as *const Self as *mut Self,
                    properties,
                    dashes,
                    count,
                    out,
                )
            })
        }
    }
    pub(super) unsafe fn CreateHwndRenderTarget(
        &self,
        properties: *const D2D1_RENDER_TARGET_PROPERTIES,
        hwnd_properties: *const D2D1_HWND_RENDER_TARGET_PROPERTIES,
    ) -> Result<ComPtr<ID2D1HwndRenderTarget>, i32> {
        unsafe {
            ComPtr::create(|out| {
                ((*self.lpVtbl).CreateHwndRenderTarget)(
                    self as *const Self as *mut Self,
                    properties,
                    hwnd_properties,
                    out,
                )
            })
        }
    }

    pub(super) unsafe fn CreateDCRenderTarget(
        &self,
        properties: *const D2D1_RENDER_TARGET_PROPERTIES,
    ) -> Result<ComPtr<ID2D1DCRenderTarget>, i32> {
        unsafe {
            ComPtr::create(|out| {
                ((*self.lpVtbl).CreateDCRenderTarget)(
                    self as *const Self as *mut Self,
                    properties,
                    out,
                )
            })
        }
    }
}

#[repr(C)]
pub(super) struct ID2D1RenderTarget {
    lpVtbl: *const ID2D1RenderTargetVtbl,
}

#[repr(C)]
struct ID2D1RenderTargetVtbl {
    base: IUnknownVtbl,
    _unused_3_3: [*const c_void; 1],
    CreateBitmap: unsafe extern "system" fn(
        *mut ID2D1RenderTarget,
        D2D_SIZE_U,
        *const c_void,
        u32,
        *const D2D1_BITMAP_PROPERTIES,
        *mut *mut ID2D1Bitmap,
    ) -> i32,
    _unused_5_6: [*const c_void; 2],
    CreateBitmapBrush: unsafe extern "system" fn(
        *mut ID2D1RenderTarget,
        *mut ID2D1Bitmap,
        *const c_void,
        *const D2D1_BRUSH_PROPERTIES,
        *mut *mut ID2D1Brush,
    ) -> i32,
    CreateSolidColorBrush: unsafe extern "system" fn(
        *mut ID2D1RenderTarget,
        *const D2D1_COLOR_F,
        *const c_void,
        *mut *mut ID2D1SolidColorBrush,
    ) -> i32,
    CreateGradientStopCollection: unsafe extern "system" fn(
        *mut ID2D1RenderTarget,
        *const D2D1_GRADIENT_STOP,
        u32,
        u32,
        u32,
        *mut *mut ID2D1GradientStopCollection,
    ) -> i32,
    CreateLinearGradientBrush: unsafe extern "system" fn(
        *mut ID2D1RenderTarget,
        *const D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES,
        *const D2D1_BRUSH_PROPERTIES,
        *mut ID2D1GradientStopCollection,
        *mut *mut ID2D1Brush,
    ) -> i32,
    CreateRadialGradientBrush: unsafe extern "system" fn(
        *mut ID2D1RenderTarget,
        *const D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES,
        *const D2D1_BRUSH_PROPERTIES,
        *mut ID2D1GradientStopCollection,
        *mut *mut ID2D1Brush,
    ) -> i32,
    CreateCompatibleRenderTarget: unsafe extern "system" fn(
        *mut ID2D1RenderTarget,
        *const c_void,
        *const c_void,
        *const c_void,
        u32,
        *mut *mut ID2D1BitmapRenderTarget,
    ) -> i32,
    _unused_13_15: [*const c_void; 3],
    DrawRectangle: unsafe extern "system" fn(
        *mut ID2D1RenderTarget,
        *const D2D_RECT_F,
        *mut ID2D1Brush,
        f32,
        *mut ID2D1StrokeStyle,
    ),
    FillRectangle:
        unsafe extern "system" fn(*mut ID2D1RenderTarget, *const D2D_RECT_F, *mut ID2D1Brush),
    _unused_18_21: [*const c_void; 4],
    DrawGeometry: unsafe extern "system" fn(
        *mut ID2D1RenderTarget,
        *mut ID2D1PathGeometry,
        *mut ID2D1Brush,
        f32,
        *mut ID2D1StrokeStyle,
    ),
    FillGeometry: unsafe extern "system" fn(
        *mut ID2D1RenderTarget,
        *mut ID2D1PathGeometry,
        *mut ID2D1Brush,
        *mut ID2D1Brush,
    ),
    _unused_24_25: [*const c_void; 2],
    DrawBitmap: unsafe extern "system" fn(
        *mut ID2D1RenderTarget,
        *mut ID2D1Bitmap,
        *const D2D_RECT_F,
        f32,
        u32,
        *const D2D_RECT_F,
    ),
    _unused_27_27: [*const c_void; 1],
    DrawTextLayout: unsafe extern "system" fn(
        *mut ID2D1RenderTarget,
        D2D_POINT_2F,
        *mut IDWriteTextLayout,
        *mut ID2D1Brush,
        u32,
    ),
    _unused_29_29: [*const c_void; 1],
    SetTransform: unsafe extern "system" fn(*mut ID2D1RenderTarget, *const D2D_MATRIX_3X2_F),
    _unused_31_39: [*const c_void; 9],
    PushLayer: unsafe extern "system" fn(
        *mut ID2D1RenderTarget,
        *const D2D1_LAYER_PARAMETERS,
        *mut c_void,
    ),
    PopLayer: unsafe extern "system" fn(*mut ID2D1RenderTarget),
    _unused_42_44: [*const c_void; 3],
    PushAxisAlignedClip: unsafe extern "system" fn(*mut ID2D1RenderTarget, *const D2D_RECT_F, u32),
    PopAxisAlignedClip: unsafe extern "system" fn(*mut ID2D1RenderTarget),
    Clear: unsafe extern "system" fn(*mut ID2D1RenderTarget, *const D2D1_COLOR_F),
    BeginDraw: unsafe extern "system" fn(*mut ID2D1RenderTarget),
    EndDraw: unsafe extern "system" fn(*mut ID2D1RenderTarget, *mut u64, *mut u64) -> i32,
    _unused_50_50: [*const c_void; 1],
    SetDpi: unsafe extern "system" fn(*mut ID2D1RenderTarget, f32, f32),
    GetDpi: unsafe extern "system" fn(*mut ID2D1RenderTarget, *mut f32, *mut f32),
    _unused_53_53: [*const c_void; 1],
    // Structs are returned through a pointer after `this`, which is returned too.
    GetPixelSize:
        unsafe extern "system" fn(*mut ID2D1RenderTarget, *mut D2D_SIZE_U) -> *mut D2D_SIZE_U,
    _unused_55_56: [*const c_void; 2],
}

impl ID2D1RenderTarget {
    pub(super) unsafe fn GetDpi(&self) -> (f32, f32) {
        let (mut x, mut y) = (0.0, 0.0);
        unsafe { ((*self.lpVtbl).GetDpi)(self as *const Self as *mut Self, &mut x, &mut y) };
        (x, y)
    }
    pub(super) unsafe fn GetPixelSize(&self) -> D2D_SIZE_U {
        let mut size = D2D_SIZE_U::default();
        unsafe { ((*self.lpVtbl).GetPixelSize)(self as *const Self as *mut Self, &mut size) };
        size
    }
    pub(super) unsafe fn CreateSolidColorBrush(
        &self,
        color: *const D2D1_COLOR_F,
        properties: *const c_void,
    ) -> Result<ComPtr<ID2D1SolidColorBrush>, i32> {
        unsafe {
            ComPtr::create(|out| {
                ((*self.lpVtbl).CreateSolidColorBrush)(
                    self as *const Self as *mut Self,
                    color,
                    properties,
                    out,
                )
            })
        }
    }
    pub(super) unsafe fn DrawRectangle(
        &self,
        rect: *const D2D_RECT_F,
        brush: *mut ID2D1Brush,
        width: f32,
        style: *mut ID2D1StrokeStyle,
    ) {
        unsafe {
            ((*self.lpVtbl).DrawRectangle)(
                self as *const Self as *mut Self,
                rect,
                brush,
                width,
                style,
            )
        }
    }
    pub(super) unsafe fn FillRectangle(&self, rect: *const D2D_RECT_F, brush: *mut ID2D1Brush) {
        unsafe { ((*self.lpVtbl).FillRectangle)(self as *const Self as *mut Self, rect, brush) }
    }
    pub(super) unsafe fn DrawGeometry(
        &self,
        geometry: *mut ID2D1PathGeometry,
        brush: *mut ID2D1Brush,
        width: f32,
        style: *mut ID2D1StrokeStyle,
    ) {
        unsafe {
            ((*self.lpVtbl).DrawGeometry)(
                self as *const Self as *mut Self,
                geometry,
                brush,
                width,
                style,
            )
        }
    }
    pub(super) unsafe fn FillGeometry(
        &self,
        geometry: *mut ID2D1PathGeometry,
        brush: *mut ID2D1Brush,
        opacity_brush: *mut ID2D1Brush,
    ) {
        unsafe {
            ((*self.lpVtbl).FillGeometry)(
                self as *const Self as *mut Self,
                geometry,
                brush,
                opacity_brush,
            )
        }
    }
    pub(super) unsafe fn DrawTextLayout(
        &self,
        origin: D2D_POINT_2F,
        layout: *mut IDWriteTextLayout,
        brush: *mut ID2D1Brush,
        options: u32,
    ) {
        unsafe {
            ((*self.lpVtbl).DrawTextLayout)(
                self as *const Self as *mut Self,
                origin,
                layout,
                brush,
                options,
            )
        }
    }
    pub(super) unsafe fn CreateBitmap(
        &self,
        size: D2D_SIZE_U,
        pixels: *const c_void,
        pitch: u32,
        properties: *const D2D1_BITMAP_PROPERTIES,
    ) -> Result<ComPtr<ID2D1Bitmap>, i32> {
        unsafe {
            ComPtr::create(|out| {
                ((*self.lpVtbl).CreateBitmap)(
                    self as *const Self as *mut Self,
                    size,
                    pixels,
                    pitch,
                    properties,
                    out,
                )
            })
        }
    }
    pub(super) unsafe fn CreateBitmapBrush(
        &self,
        bitmap: *mut ID2D1Bitmap,
    ) -> Result<ComPtr<ID2D1Brush>, i32> {
        unsafe {
            ComPtr::create(|out| {
                ((*self.lpVtbl).CreateBitmapBrush)(
                    self as *const Self as *mut Self,
                    bitmap,
                    std::ptr::null(),
                    std::ptr::null(),
                    out,
                )
            })
        }
    }
    pub(super) unsafe fn CreateGradientStopCollection(
        &self,
        stops: &[D2D1_GRADIENT_STOP],
        extend: u32,
    ) -> Result<ComPtr<ID2D1GradientStopCollection>, i32> {
        unsafe {
            ComPtr::create(|out| {
                ((*self.lpVtbl).CreateGradientStopCollection)(
                    self as *const Self as *mut Self,
                    stops.as_ptr(),
                    stops.len() as u32,
                    D2D1_GAMMA_2_2,
                    extend,
                    out,
                )
            })
        }
    }
    pub(super) unsafe fn CreateLinearGradientBrush(
        &self,
        gradient: *const D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES,
        brush: *const D2D1_BRUSH_PROPERTIES,
        stops: *mut ID2D1GradientStopCollection,
    ) -> Result<ComPtr<ID2D1Brush>, i32> {
        unsafe {
            ComPtr::create(|out| {
                ((*self.lpVtbl).CreateLinearGradientBrush)(
                    self as *const Self as *mut Self,
                    gradient,
                    brush,
                    stops,
                    out,
                )
            })
        }
    }
    pub(super) unsafe fn CreateRadialGradientBrush(
        &self,
        gradient: *const D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES,
        brush: *const D2D1_BRUSH_PROPERTIES,
        stops: *mut ID2D1GradientStopCollection,
    ) -> Result<ComPtr<ID2D1Brush>, i32> {
        unsafe {
            ComPtr::create(|out| {
                ((*self.lpVtbl).CreateRadialGradientBrush)(
                    self as *const Self as *mut Self,
                    gradient,
                    brush,
                    stops,
                    out,
                )
            })
        }
    }
    /// A bitmap render target with the size, DPI and pixel format of this one.
    pub(super) unsafe fn CreateCompatibleRenderTarget(
        &self,
    ) -> Result<ComPtr<ID2D1BitmapRenderTarget>, i32> {
        unsafe {
            ComPtr::create(|out| {
                ((*self.lpVtbl).CreateCompatibleRenderTarget)(
                    self as *const Self as *mut Self,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    out,
                )
            })
        }
    }
    pub(super) unsafe fn DrawBitmap(
        &self,
        bitmap: *mut ID2D1Bitmap,
        rect: *const D2D_RECT_F,
        opacity: f32,
        interpolation: u32,
    ) {
        unsafe {
            ((*self.lpVtbl).DrawBitmap)(
                self as *const Self as *mut Self,
                bitmap,
                rect,
                opacity,
                interpolation,
                std::ptr::null(),
            )
        }
    }
    pub(super) unsafe fn SetTransform(&self, transform: *const D2D_MATRIX_3X2_F) {
        unsafe { ((*self.lpVtbl).SetTransform)(self as *const Self as *mut Self, transform) }
    }
    pub(super) unsafe fn PushLayer(
        &self,
        parameters: *const D2D1_LAYER_PARAMETERS,
        layer: *mut c_void,
    ) {
        unsafe { ((*self.lpVtbl).PushLayer)(self as *const Self as *mut Self, parameters, layer) }
    }
    pub(super) unsafe fn PopLayer(&self) {
        unsafe { ((*self.lpVtbl).PopLayer)(self as *const Self as *mut Self) }
    }
    pub(super) unsafe fn PushAxisAlignedClip(&self, rect: *const D2D_RECT_F, mode: u32) {
        unsafe {
            ((*self.lpVtbl).PushAxisAlignedClip)(self as *const Self as *mut Self, rect, mode)
        }
    }
    pub(super) unsafe fn PopAxisAlignedClip(&self) {
        unsafe { ((*self.lpVtbl).PopAxisAlignedClip)(self as *const Self as *mut Self) }
    }
    pub(super) unsafe fn Clear(&self, color: *const D2D1_COLOR_F) {
        unsafe { ((*self.lpVtbl).Clear)(self as *const Self as *mut Self, color) }
    }
    pub(super) unsafe fn BeginDraw(&self) {
        unsafe { ((*self.lpVtbl).BeginDraw)(self as *const Self as *mut Self) }
    }
    pub(super) unsafe fn EndDraw(&self, tag1: *mut u64, tag2: *mut u64) -> Result<(), i32> {
        unsafe {
            check(((*self.lpVtbl).EndDraw)(
                self as *const Self as *mut Self,
                tag1,
                tag2,
            ))
        }
    }
    pub(super) unsafe fn SetDpi(&self, x: f32, y: f32) {
        unsafe { ((*self.lpVtbl).SetDpi)(self as *const Self as *mut Self, x, y) }
    }
}

#[repr(C)]
pub(super) struct ID2D1HwndRenderTarget {
    lpVtbl: *const ID2D1HwndRenderTargetVtbl,
}

#[repr(C)]
struct ID2D1HwndRenderTargetVtbl {
    base: ID2D1RenderTargetVtbl,
    _unused_57_57: [*const c_void; 1],
    Resize: unsafe extern "system" fn(*mut ID2D1HwndRenderTarget, *const D2D_SIZE_U) -> i32,
}

impl ID2D1HwndRenderTarget {
    pub(super) unsafe fn Resize(&self, size: *const D2D_SIZE_U) -> Result<(), i32> {
        unsafe {
            check(((*self.lpVtbl).Resize)(
                self as *const Self as *mut Self,
                size,
            ))
        }
    }
}

#[repr(C)]
pub(super) struct ID2D1DCRenderTarget {
    lpVtbl: *const ID2D1DCRenderTargetVtbl,
}

#[repr(C)]
struct ID2D1DCRenderTargetVtbl {
    base: ID2D1RenderTargetVtbl,
    BindDC: unsafe extern "system" fn(*mut ID2D1DCRenderTarget, HDC, *const RECT) -> i32,
}

impl ID2D1DCRenderTarget {
    pub(super) unsafe fn BindDC(&self, dc: HDC, rect: *const RECT) -> Result<(), i32> {
        unsafe {
            check(((*self.lpVtbl).BindDC)(
                self as *const Self as *mut Self,
                dc,
                rect,
            ))
        }
    }
}

// Derived render targets begin with the ID2D1RenderTarget vtable.
impl Deref for ID2D1HwndRenderTarget {
    type Target = ID2D1RenderTarget;
    fn deref(&self) -> &ID2D1RenderTarget {
        unsafe { &*(self as *const Self).cast() }
    }
}
impl Deref for ID2D1DCRenderTarget {
    type Target = ID2D1RenderTarget;
    fn deref(&self) -> &ID2D1RenderTarget {
        unsafe { &*(self as *const Self).cast() }
    }
}

#[repr(C)]
pub(super) struct ID2D1BitmapRenderTarget {
    lpVtbl: *const ID2D1BitmapRenderTargetVtbl,
}

#[repr(C)]
struct ID2D1BitmapRenderTargetVtbl {
    base: ID2D1RenderTargetVtbl,
    GetBitmap:
        unsafe extern "system" fn(*mut ID2D1BitmapRenderTarget, *mut *mut ID2D1Bitmap) -> i32,
}

impl ID2D1BitmapRenderTarget {
    pub(super) unsafe fn GetBitmap(&self) -> Result<ComPtr<ID2D1Bitmap>, i32> {
        unsafe {
            ComPtr::create(|out| ((*self.lpVtbl).GetBitmap)(self as *const Self as *mut Self, out))
        }
    }
}

impl Deref for ID2D1BitmapRenderTarget {
    type Target = ID2D1RenderTarget;
    fn deref(&self) -> &ID2D1RenderTarget {
        unsafe { &*(self as *const Self).cast() }
    }
}

/// Any brush, solid, gradient or bitmap.
#[repr(C)]
pub(super) struct ID2D1Brush {
    _vtable: *const IUnknownVtbl,
}

// Solid color brushes begin with the ID2D1Brush vtable.
impl Deref for ID2D1SolidColorBrush {
    type Target = ID2D1Brush;
    fn deref(&self) -> &ID2D1Brush {
        unsafe { &*(self as *const Self).cast() }
    }
}

#[repr(C)]
pub(super) struct ID2D1Bitmap {
    lpVtbl: *const ID2D1BitmapVtbl,
}

#[repr(C)]
struct ID2D1BitmapVtbl {
    base: IUnknownVtbl,
    _unused_3_8: [*const c_void; 6],
    CopyFromRenderTarget: unsafe extern "system" fn(
        *mut ID2D1Bitmap,
        *const c_void,
        *mut ID2D1RenderTarget,
        *const c_void,
    ) -> i32,
}

impl ID2D1Bitmap {
    /// Copy all of a render target with the same size into this bitmap.
    pub(super) unsafe fn CopyFromRenderTarget(
        &self,
        target: *mut ID2D1RenderTarget,
    ) -> Result<(), i32> {
        unsafe {
            check(((*self.lpVtbl).CopyFromRenderTarget)(
                self as *const Self as *mut Self,
                std::ptr::null(),
                target,
                std::ptr::null(),
            ))
        }
    }
}

/// A Direct2D 1.1 effect.
#[repr(C)]
pub(super) struct ID2D1Effect {
    lpVtbl: *const ID2D1EffectVtbl,
}

#[repr(C)]
struct ID2D1EffectVtbl {
    base: IUnknownVtbl,
    _unused_3_8: [*const c_void; 6],
    SetValue: unsafe extern "system" fn(*mut ID2D1Effect, u32, u32, *const u8, u32) -> i32,
    _unused_10_13: [*const c_void; 4],
    SetInput: unsafe extern "system" fn(*mut ID2D1Effect, u32, *mut c_void, BOOL),
    _unused_15_17: [*const c_void; 3],
    GetOutput: unsafe extern "system" fn(*mut ID2D1Effect, *mut *mut c_void),
}

impl ID2D1Effect {
    /// Set a property to a value whose type is checked by its size.
    pub(super) unsafe fn SetValue<T: Copy>(&self, index: u32, value: T) -> Result<(), i32> {
        unsafe {
            check(((*self.lpVtbl).SetValue)(
                self as *const Self as *mut Self,
                index,
                0,
                (&raw const value).cast(),
                size_of::<T>() as u32,
            ))
        }
    }
    pub(super) unsafe fn SetInput(&self, index: u32, image: *mut c_void) {
        unsafe { ((*self.lpVtbl).SetInput)(self as *const Self as *mut Self, index, image, 1) }
    }
    /// The output image, which holds a reference.
    pub(super) unsafe fn GetOutput(&self) -> ComPtr<ID2D1Image> {
        unsafe {
            ComPtr::create(|out: *mut *mut ID2D1Image| {
                ((*self.lpVtbl).GetOutput)(self as *const Self as *mut Self, out.cast());
                0
            })
        }
        .expect("Direct2D effect output")
    }
}

/// A Direct2D 1.1 image, like a bitmap or an effect output.
#[repr(C)]
pub(super) struct ID2D1Image {
    _vtable: *const IUnknownVtbl,
}

#[repr(C)]
pub(super) struct ID2D1GradientStopCollection {
    _vtable: *const IUnknownVtbl,
}

/// Direct2D 1.1 interface, queried from a render target on Windows 8 and later.
#[repr(C)]
pub(super) struct ID2D1DeviceContext {
    lpVtbl: *const ID2D1DeviceContextVtbl,
}

#[repr(C)]
struct ID2D1DeviceContextVtbl {
    base: ID2D1RenderTargetVtbl,
    _unused_57_62: [*const c_void; 6],
    CreateEffect: unsafe extern "system" fn(
        *mut ID2D1DeviceContext,
        *const GUID,
        *mut *mut ID2D1Effect,
    ) -> i32,
    _unused_64_77: [*const c_void; 14],
    SetPrimitiveBlend: unsafe extern "system" fn(*mut ID2D1DeviceContext, u32),
    _unused_79_82: [*const c_void; 4],
    DrawImage: unsafe extern "system" fn(
        *mut ID2D1DeviceContext,
        *mut c_void,
        *const D2D_POINT_2F,
        *const D2D_RECT_F,
        u32,
        u32,
    ),
    _unused_84_84: [*const c_void; 1],
    DrawBitmap: unsafe extern "system" fn(
        *mut ID2D1DeviceContext,
        *mut ID2D1Bitmap,
        *const D2D_RECT_F,
        f32,
        u32,
        *const D2D_RECT_F,
        *const c_void,
    ),
}

impl ID2D1DeviceContext {
    pub(super) unsafe fn CreateEffect(&self, clsid: &GUID) -> Result<ComPtr<ID2D1Effect>, i32> {
        unsafe {
            ComPtr::create(|out| {
                ((*self.lpVtbl).CreateEffect)(self as *const Self as *mut Self, clsid, out)
            })
        }
    }
    /// Draw an image at the origin of the world transform with a composite mode.
    pub(super) unsafe fn DrawImage(&self, image: *mut c_void, composite: u32) {
        unsafe {
            ((*self.lpVtbl).DrawImage)(
                self as *const Self as *mut Self,
                image,
                std::ptr::null(),
                std::ptr::null(),
                1,
                composite,
            )
        }
    }
    pub(super) unsafe fn SetPrimitiveBlend(&self, blend: u32) {
        unsafe { ((*self.lpVtbl).SetPrimitiveBlend)(self as *const Self as *mut Self, blend) }
    }
    /// Draw a bitmap with a Direct2D 1.1 interpolation mode, like high quality cubic.
    pub(super) unsafe fn DrawBitmap(
        &self,
        bitmap: *mut ID2D1Bitmap,
        rect: *const D2D_RECT_F,
        opacity: f32,
        interpolation: u32,
    ) {
        unsafe {
            ((*self.lpVtbl).DrawBitmap)(
                self as *const Self as *mut Self,
                bitmap,
                rect,
                opacity,
                interpolation,
                std::ptr::null(),
                std::ptr::null(),
            )
        }
    }
}

#[repr(C)]
pub(super) struct ID2D1PathGeometry {
    lpVtbl: *const ID2D1PathGeometryVtbl,
}

#[repr(C)]
struct ID2D1PathGeometryVtbl {
    base: IUnknownVtbl,
    _unused_3_15: [*const c_void; 13],
    Widen: unsafe extern "system" fn(
        *mut ID2D1PathGeometry,
        f32,
        *mut ID2D1StrokeStyle,
        *const D2D_MATRIX_3X2_F,
        f32,
        *mut ID2D1GeometrySink,
    ) -> i32,
    Open: unsafe extern "system" fn(*mut ID2D1PathGeometry, *mut *mut ID2D1GeometrySink) -> i32,
}

impl ID2D1PathGeometry {
    /// Write the outline of the stroke of this geometry, transformed, into a sink.
    pub(super) unsafe fn Widen(
        &self,
        width: f32,
        style: *mut ID2D1StrokeStyle,
        transform: &D2D_MATRIX_3X2_F,
        sink: *mut ID2D1GeometrySink,
    ) -> Result<(), i32> {
        unsafe {
            check(((*self.lpVtbl).Widen)(
                self as *const Self as *mut Self,
                width,
                style,
                transform,
                0.25,
                sink,
            ))
        }
    }
    pub(super) unsafe fn Open(&self) -> Result<ComPtr<ID2D1GeometrySink>, i32> {
        unsafe {
            ComPtr::create(|out| ((*self.lpVtbl).Open)(self as *const Self as *mut Self, out))
        }
    }
}

#[repr(C)]
pub(super) struct ID2D1GeometrySink {
    lpVtbl: *const ID2D1GeometrySinkVtbl,
}

#[repr(C)]
struct ID2D1GeometrySinkVtbl {
    base: IUnknownVtbl,
    SetFillMode: unsafe extern "system" fn(*mut ID2D1GeometrySink, u32),
    _unused_4_4: [*const c_void; 1],
    BeginFigure: unsafe extern "system" fn(*mut ID2D1GeometrySink, D2D_POINT_2F, u32),
    _unused_6_7: [*const c_void; 2],
    EndFigure: unsafe extern "system" fn(*mut ID2D1GeometrySink, u32),
    Close: unsafe extern "system" fn(*mut ID2D1GeometrySink) -> i32,
    AddLine: unsafe extern "system" fn(*mut ID2D1GeometrySink, D2D_POINT_2F),
    AddBezier: unsafe extern "system" fn(*mut ID2D1GeometrySink, *const D2D1_BEZIER_SEGMENT),
    AddQuadraticBezier:
        unsafe extern "system" fn(*mut ID2D1GeometrySink, *const D2D1_QUADRATIC_BEZIER_SEGMENT),
}

impl ID2D1GeometrySink {
    pub(super) unsafe fn SetFillMode(&self, mode: u32) {
        unsafe { ((*self.lpVtbl).SetFillMode)(self as *const Self as *mut Self, mode) }
    }
    pub(super) unsafe fn BeginFigure(&self, point: D2D_POINT_2F, begin: u32) {
        unsafe { ((*self.lpVtbl).BeginFigure)(self as *const Self as *mut Self, point, begin) }
    }
    pub(super) unsafe fn EndFigure(&self, end: u32) {
        unsafe { ((*self.lpVtbl).EndFigure)(self as *const Self as *mut Self, end) }
    }
    pub(super) unsafe fn Close(&self) -> Result<(), i32> {
        unsafe { check(((*self.lpVtbl).Close)(self as *const Self as *mut Self)) }
    }
    pub(super) unsafe fn AddLine(&self, point: D2D_POINT_2F) {
        unsafe { ((*self.lpVtbl).AddLine)(self as *const Self as *mut Self, point) }
    }
    pub(super) unsafe fn AddBezier(&self, bezier: *const D2D1_BEZIER_SEGMENT) {
        unsafe { ((*self.lpVtbl).AddBezier)(self as *const Self as *mut Self, bezier) }
    }
    pub(super) unsafe fn AddQuadraticBezier(&self, bezier: *const D2D1_QUADRATIC_BEZIER_SEGMENT) {
        unsafe { ((*self.lpVtbl).AddQuadraticBezier)(self as *const Self as *mut Self, bezier) }
    }
}

#[repr(C)]
pub(super) struct IDWriteFactory {
    lpVtbl: *const IDWriteFactoryVtbl,
}

#[repr(C)]
struct IDWriteFactoryVtbl {
    base: IUnknownVtbl,
    _unused_3_14: [*const c_void; 12],
    CreateTextFormat: unsafe extern "system" fn(
        *mut IDWriteFactory,
        *const u16,
        *mut c_void,
        u32,
        u32,
        u32,
        f32,
        *const u16,
        *mut *mut IDWriteTextFormat,
    ) -> i32,
    _unused_16_17: [*const c_void; 2],
    CreateTextLayout: unsafe extern "system" fn(
        *mut IDWriteFactory,
        *const u16,
        u32,
        *mut IDWriteTextFormat,
        f32,
        f32,
        *mut *mut IDWriteTextLayout,
    ) -> i32,
}

impl IDWriteFactory {
    pub(super) unsafe fn CreateTextFormat(
        &self,
        family: *const u16,
        collection: *mut c_void,
        weight: u32,
        style: u32,
        stretch: u32,
        size: f32,
        locale: *const u16,
    ) -> Result<ComPtr<IDWriteTextFormat>, i32> {
        unsafe {
            ComPtr::create(|out| {
                ((*self.lpVtbl).CreateTextFormat)(
                    self as *const Self as *mut Self,
                    family,
                    collection,
                    weight,
                    style,
                    stretch,
                    size,
                    locale,
                    out,
                )
            })
        }
    }
    pub(super) unsafe fn CreateTextLayout(
        &self,
        text: *const u16,
        length: u32,
        format: *mut IDWriteTextFormat,
        width: f32,
        height: f32,
    ) -> Result<ComPtr<IDWriteTextLayout>, i32> {
        unsafe {
            ComPtr::create(|out| {
                ((*self.lpVtbl).CreateTextLayout)(
                    self as *const Self as *mut Self,
                    text,
                    length,
                    format,
                    width,
                    height,
                    out,
                )
            })
        }
    }
}

#[repr(C)]
pub(super) struct IDWriteTextFormat {
    lpVtbl: *const IDWriteTextFormatVtbl,
}

#[repr(C)]
struct IDWriteTextFormatVtbl {
    base: IUnknownVtbl,
    SetTextAlignment: unsafe extern "system" fn(*mut IDWriteTextFormat, u32) -> i32,
    SetParagraphAlignment: unsafe extern "system" fn(*mut IDWriteTextFormat, u32) -> i32,
}

impl IDWriteTextFormat {
    pub(super) unsafe fn SetTextAlignment(&self, alignment: u32) -> Result<(), i32> {
        unsafe {
            check(((*self.lpVtbl).SetTextAlignment)(
                self as *const Self as *mut Self,
                alignment,
            ))
        }
    }
    pub(super) unsafe fn SetParagraphAlignment(&self, alignment: u32) -> Result<(), i32> {
        unsafe {
            check(((*self.lpVtbl).SetParagraphAlignment)(
                self as *const Self as *mut Self,
                alignment,
            ))
        }
    }
}

#[repr(C)]
pub(super) struct IDWriteTextLayout {
    lpVtbl: *const IDWriteTextLayoutVtbl,
}

#[repr(C)]
struct IDWriteTextLayoutVtbl {
    base: IUnknownVtbl,
    _unused_3_58: [*const c_void; 56],
    GetLineMetrics: unsafe extern "system" fn(
        *mut IDWriteTextLayout,
        *mut DWRITE_LINE_METRICS,
        u32,
        *mut u32,
    ) -> i32,
    GetMetrics: unsafe extern "system" fn(*mut IDWriteTextLayout, *mut DWRITE_TEXT_METRICS) -> i32,
}

impl IDWriteTextLayout {
    pub(super) unsafe fn GetLineMetrics(
        &self,
        metrics: *mut DWRITE_LINE_METRICS,
        capacity: u32,
        count: *mut u32,
    ) -> Result<(), i32> {
        unsafe {
            check(((*self.lpVtbl).GetLineMetrics)(
                self as *const Self as *mut Self,
                metrics,
                capacity,
                count,
            ))
        }
    }
    pub(super) unsafe fn GetMetrics(&self, metrics: *mut DWRITE_TEXT_METRICS) -> Result<(), i32> {
        unsafe {
            check(((*self.lpVtbl).GetMetrics)(
                self as *const Self as *mut Self,
                metrics,
            ))
        }
    }
}

#[repr(C)]
pub(super) struct ID2D1SolidColorBrush {
    lpVtbl: *const ID2D1SolidColorBrushVtbl,
}

#[repr(C)]
struct ID2D1SolidColorBrushVtbl {
    base: IUnknownVtbl,
    _unused_3_7: [*const c_void; 5],
    SetColor: unsafe extern "system" fn(*mut ID2D1SolidColorBrush, *const D2D1_COLOR_F),
}

impl ID2D1SolidColorBrush {
    pub(super) unsafe fn SetColor(&self, color: *const D2D1_COLOR_F) {
        unsafe { ((*self.lpVtbl).SetColor)(self as *const Self as *mut Self, color) }
    }
}

#[repr(C)]
pub(super) struct ID2D1StrokeStyle {
    _vtable: *const IUnknownVtbl,
}

// Factory creation functions return one owned COM reference.
const IID_ID2D1_FACTORY: GUID = GUID {
    data1: 0x06152247,
    data2: 0x6f50,
    data3: 0x465a,
    data4: [0x92, 0x45, 0x11, 0x8b, 0xfd, 0x3b, 0x60, 0x07],
};
pub(super) const IID_ID2D1_DEVICE_CONTEXT: GUID = GUID {
    data1: 0xe8f7fe7a,
    data2: 0x191c,
    data3: 0x466d,
    data4: [0xad, 0x95, 0x97, 0x56, 0x78, 0xbd, 0xa9, 0x98],
};
const IID_IDWRITE_FACTORY: GUID = GUID {
    data1: 0xb859ee5a,
    data2: 0xd838,
    data3: 0x4b5b,
    data4: [0xa2, 0xe8, 0x1a, 0xdc, 0x7d, 0x93, 0xdb, 0x48],
};
#[link(name = "d2d1")]
unsafe extern "system" {
    fn D2D1CreateFactory(
        factory_type: u32,
        iid: *const GUID,
        options: *const c_void,
        factory: *mut *mut ID2D1Factory,
    ) -> i32;
}
#[link(name = "dwrite")]
unsafe extern "system" {
    fn DWriteCreateFactory(
        factory_type: u32,
        iid: *const GUID,
        factory: *mut *mut IDWriteFactory,
    ) -> i32;
}
impl ID2D1Factory {
    pub(super) unsafe fn new() -> Result<ComPtr<Self>, i32> {
        unsafe {
            ComPtr::create(|out| D2D1CreateFactory(0, &IID_ID2D1_FACTORY, std::ptr::null(), out))
        }
    }
}
impl IDWriteFactory {
    pub(super) unsafe fn new() -> Result<ComPtr<Self>, i32> {
        unsafe { ComPtr::create(|out| DWriteCreateFactory(0, &IID_IDWRITE_FACTORY, out)) }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D2D_POINT_2F {
    pub(super) x: f32,
    pub(super) y: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D2D_MATRIX_3X2_F {
    pub(super) m11: f32,
    pub(super) m12: f32,
    pub(super) m21: f32,
    pub(super) m22: f32,
    pub(super) dx: f32,
    pub(super) dy: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D2D_SIZE_U {
    pub(super) width: u32,
    pub(super) height: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D2D_RECT_F {
    pub(super) left: f32,
    pub(super) top: f32,
    pub(super) right: f32,
    pub(super) bottom: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D2D1_COLOR_F {
    pub(super) r: f32,
    pub(super) g: f32,
    pub(super) b: f32,
    pub(super) a: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D2D1_PIXEL_FORMAT {
    pub(super) format: u32,
    pub(super) alphaMode: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D2D1_RENDER_TARGET_PROPERTIES {
    pub(super) target_type: u32,
    pub(super) pixelFormat: D2D1_PIXEL_FORMAT,
    pub(super) dpiX: f32,
    pub(super) dpiY: f32,
    pub(super) usage: u32,
    pub(super) minLevel: u32,
}

pub(super) const D2D1_PRESENT_OPTIONS_IMMEDIATELY: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D2D1_HWND_RENDER_TARGET_PROPERTIES {
    pub(super) hwnd: HWND,
    pub(super) pixelSize: D2D_SIZE_U,
    pub(super) presentOptions: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D2D1_STROKE_STYLE_PROPERTIES {
    pub(super) startCap: u32,
    pub(super) endCap: u32,
    pub(super) dashCap: u32,
    pub(super) lineJoin: u32,
    pub(super) miterLimit: f32,
    pub(super) dashStyle: u32,
    pub(super) dashOffset: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D2D1_BEZIER_SEGMENT {
    pub(super) point1: D2D_POINT_2F,
    pub(super) point2: D2D_POINT_2F,
    pub(super) point3: D2D_POINT_2F,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D2D1_QUADRATIC_BEZIER_SEGMENT {
    pub(super) point1: D2D_POINT_2F,
    pub(super) point2: D2D_POINT_2F,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D2D1_GRADIENT_STOP {
    pub(super) position: f32,
    pub(super) color: D2D1_COLOR_F,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D2D1_BRUSH_PROPERTIES {
    pub(super) opacity: f32,
    pub(super) transform: D2D_MATRIX_3X2_F,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
    pub(super) startPoint: D2D_POINT_2F,
    pub(super) endPoint: D2D_POINT_2F,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
    pub(super) center: D2D_POINT_2F,
    pub(super) gradientOriginOffset: D2D_POINT_2F,
    pub(super) radiusX: f32,
    pub(super) radiusY: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D2D1_BITMAP_PROPERTIES {
    pub(super) pixelFormat: D2D1_PIXEL_FORMAT,
    pub(super) dpiX: f32,
    pub(super) dpiY: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D2D1_LAYER_PARAMETERS {
    pub(super) contentBounds: D2D_RECT_F,
    pub(super) geometricMask: *mut ID2D1PathGeometry,
    pub(super) maskAntialiasMode: u32,
    pub(super) maskTransform: D2D_MATRIX_3X2_F,
    pub(super) opacity: f32,
    pub(super) opacityBrush: *mut ID2D1Brush,
    pub(super) layerOptions: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct DWRITE_TEXT_METRICS {
    pub(super) left: f32,
    pub(super) top: f32,
    pub(super) width: f32,
    pub(super) widthIncludingTrailingWhitespace: f32,
    pub(super) height: f32,
    pub(super) layoutWidth: f32,
    pub(super) layoutHeight: f32,
    pub(super) maxBidiReorderingDepth: u32,
    pub(super) lineCount: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct DWRITE_LINE_METRICS {
    pub(super) length: u32,
    pub(super) trailingWhitespaceLength: u32,
    pub(super) newlineLength: u32,
    pub(super) height: f32,
    pub(super) baseline: f32,
    pub(super) isTrimmed: BOOL,
}

impl D2D_MATRIX_3X2_F {
    pub(super) const fn identity() -> Self {
        Self {
            m11: 1.0,
            m12: 0.0,
            m21: 0.0,
            m22: 1.0,
            dx: 0.0,
            dy: 0.0,
        }
    }
}

pub(super) const D2D1_ALPHA_MODE_UNKNOWN: u32 = 0;
pub(super) const D2D1_ALPHA_MODE_PREMULTIPLIED: u32 = 1;
pub(super) const D2D1_RENDER_TARGET_TYPE_SOFTWARE: u32 = 1;
pub(super) const DXGI_FORMAT_UNKNOWN: u32 = 0;
pub(super) const DXGI_FORMAT_B8G8R8A8_UNORM: u32 = 87;
pub(super) const D2D1_ANTIALIAS_MODE_PER_PRIMITIVE: u32 = 0;
pub(super) const D2D1_ANTIALIAS_MODE_ALIASED: u32 = 1;
pub(super) const D2D1_FILL_MODE_ALTERNATE: u32 = 0;
pub(super) const D2D1_FILL_MODE_WINDING: u32 = 1;
pub(super) const D2D1_GAMMA_2_2: u32 = 0;
pub(super) const D2D1_EXTEND_MODE_CLAMP: u32 = 0;
pub(super) const D2D1_EXTEND_MODE_WRAP: u32 = 1;
pub(super) const D2D1_EXTEND_MODE_MIRROR: u32 = 2;
pub(super) const D2D1_DASH_STYLE_CUSTOM: u32 = 5;
pub(super) const D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR: u32 = 0;
pub(super) const D2D1_BITMAP_INTERPOLATION_MODE_LINEAR: u32 = 1;
pub(super) const D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR: u32 = 0;
pub(super) const D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC: u32 = 5;
pub(super) const D2D1_PRIMITIVE_BLEND_ADD: u32 = 3;
pub(super) const CLSID_D2D1_BLEND: GUID = GUID {
    data1: 0x81c5_b77b,
    data2: 0x13f8,
    data3: 0x4cdd,
    data4: [0xad, 0x20, 0xc8, 0x90, 0x54, 0x7a, 0xc6, 0x5d],
};
pub(super) const CLSID_D2D1_OPACITY: GUID = GUID {
    data1: 0x811d_79a4,
    data2: 0xde28,
    data3: 0x4454,
    data4: [0x80, 0x94, 0xc6, 0x46, 0x85, 0xf8, 0xbd, 0x4c],
};
pub(super) const D2D1_COMPOSITE_MODE_SOURCE_COPY: u32 = 10;
pub(super) const D2D1_FIGURE_BEGIN_FILLED: u32 = 0;
pub(super) const D2D1_FIGURE_END_OPEN: u32 = 0;
pub(super) const D2D1_FIGURE_END_CLOSED: u32 = 1;
pub(super) const D2D1_CAP_STYLE_FLAT: u32 = 0;
pub(super) const D2D1_CAP_STYLE_SQUARE: u32 = 1;
pub(super) const D2D1_CAP_STYLE_ROUND: u32 = 2;
pub(super) const D2D1_LINE_JOIN_BEVEL: u32 = 1;
pub(super) const D2D1_LINE_JOIN_ROUND: u32 = 2;
pub(super) const D2D1_LINE_JOIN_MITER_OR_BEVEL: u32 = 3;
pub(super) const D2D1_PRIMITIVE_BLEND_SOURCE_OVER: u32 = 0;
pub(super) const D2D1_PRIMITIVE_BLEND_COPY: u32 = 1;
pub(super) const D2D1_DRAW_TEXT_OPTIONS_NONE: u32 = 0;
pub(super) const DWRITE_FONT_WEIGHT_NORMAL: u32 = 400;
pub(super) const DWRITE_FONT_WEIGHT_BOLD: u32 = 700;
pub(super) const DWRITE_FONT_STYLE_NORMAL: u32 = 0;
pub(super) const DWRITE_FONT_STRETCH_NORMAL: u32 = 5;
pub(super) const DWRITE_TEXT_ALIGNMENT_LEADING: u32 = 0;
pub(super) const DWRITE_PARAGRAPH_ALIGNMENT_NEAR: u32 = 0;

// ABI guards: slot numbers include IUnknown and all inherited methods.
const _: () = {
    let slot = size_of::<*const c_void>();
    assert!(std::mem::offset_of!(ID2D1FactoryVtbl, CreatePathGeometry) == 10 * slot);
    assert!(std::mem::offset_of!(ID2D1FactoryVtbl, CreateStrokeStyle) == 11 * slot);
    assert!(std::mem::offset_of!(ID2D1FactoryVtbl, CreateHwndRenderTarget) == 14 * slot);
    assert!(std::mem::offset_of!(ID2D1FactoryVtbl, CreateDCRenderTarget) == 16 * slot);
    assert!(std::mem::offset_of!(ID2D1RenderTargetVtbl, CreateBitmap) == 4 * slot);
    assert!(std::mem::offset_of!(ID2D1RenderTargetVtbl, CreateBitmapBrush) == 7 * slot);
    assert!(std::mem::offset_of!(ID2D1RenderTargetVtbl, CreateSolidColorBrush) == 8 * slot);
    assert!(std::mem::offset_of!(ID2D1RenderTargetVtbl, CreateGradientStopCollection) == 9 * slot);
    assert!(std::mem::offset_of!(ID2D1RenderTargetVtbl, CreateLinearGradientBrush) == 10 * slot);
    assert!(std::mem::offset_of!(ID2D1RenderTargetVtbl, CreateRadialGradientBrush) == 11 * slot);
    assert!(std::mem::offset_of!(ID2D1RenderTargetVtbl, CreateCompatibleRenderTarget) == 12 * slot);
    assert!(std::mem::offset_of!(ID2D1RenderTargetVtbl, DrawBitmap) == 26 * slot);
    assert!(std::mem::offset_of!(ID2D1BitmapRenderTargetVtbl, GetBitmap) == 57 * slot);
    assert!(std::mem::offset_of!(ID2D1DeviceContextVtbl, DrawBitmap) == 85 * slot);
    assert!(std::mem::offset_of!(ID2D1DeviceContextVtbl, CreateEffect) == 63 * slot);
    assert!(std::mem::offset_of!(ID2D1DeviceContextVtbl, DrawImage) == 83 * slot);
    assert!(std::mem::offset_of!(ID2D1RenderTargetVtbl, GetDpi) == 52 * slot);
    assert!(std::mem::offset_of!(ID2D1RenderTargetVtbl, GetPixelSize) == 54 * slot);
    assert!(std::mem::offset_of!(ID2D1BitmapVtbl, CopyFromRenderTarget) == 9 * slot);
    assert!(std::mem::offset_of!(ID2D1EffectVtbl, SetValue) == 9 * slot);
    assert!(std::mem::offset_of!(ID2D1EffectVtbl, SetInput) == 14 * slot);
    assert!(std::mem::offset_of!(ID2D1EffectVtbl, GetOutput) == 18 * slot);
    assert!(std::mem::offset_of!(ID2D1PathGeometryVtbl, Widen) == 16 * slot);
    assert!(std::mem::offset_of!(ID2D1RenderTargetVtbl, DrawRectangle) == 16 * slot);
    assert!(std::mem::offset_of!(ID2D1RenderTargetVtbl, FillRectangle) == 17 * slot);
    assert!(std::mem::offset_of!(ID2D1RenderTargetVtbl, DrawGeometry) == 22 * slot);
    assert!(std::mem::offset_of!(ID2D1RenderTargetVtbl, FillGeometry) == 23 * slot);
    assert!(std::mem::offset_of!(ID2D1RenderTargetVtbl, DrawTextLayout) == 28 * slot);
    assert!(std::mem::offset_of!(ID2D1RenderTargetVtbl, SetTransform) == 30 * slot);
    assert!(std::mem::offset_of!(ID2D1RenderTargetVtbl, PushLayer) == 40 * slot);
    assert!(std::mem::offset_of!(ID2D1RenderTargetVtbl, PopLayer) == 41 * slot);
    assert!(std::mem::offset_of!(ID2D1RenderTargetVtbl, PushAxisAlignedClip) == 45 * slot);
    assert!(std::mem::offset_of!(ID2D1RenderTargetVtbl, PopAxisAlignedClip) == 46 * slot);
    assert!(std::mem::offset_of!(ID2D1RenderTargetVtbl, Clear) == 47 * slot);
    assert!(std::mem::offset_of!(ID2D1RenderTargetVtbl, BeginDraw) == 48 * slot);
    assert!(std::mem::offset_of!(ID2D1RenderTargetVtbl, EndDraw) == 49 * slot);
    assert!(std::mem::offset_of!(ID2D1RenderTargetVtbl, SetDpi) == 51 * slot);
    assert!(size_of::<ID2D1RenderTargetVtbl>() == 57 * slot);
    assert!(std::mem::offset_of!(ID2D1HwndRenderTargetVtbl, Resize) == 58 * slot);
    assert!(std::mem::offset_of!(ID2D1DCRenderTargetVtbl, BindDC) == 57 * slot);
    assert!(std::mem::offset_of!(ID2D1DeviceContextVtbl, SetPrimitiveBlend) == 78 * slot);
    assert!(std::mem::offset_of!(ID2D1SolidColorBrushVtbl, SetColor) == 8 * slot);
    assert!(std::mem::offset_of!(ID2D1PathGeometryVtbl, Open) == 17 * slot);
    assert!(std::mem::offset_of!(ID2D1GeometrySinkVtbl, SetFillMode) == 3 * slot);
    assert!(std::mem::offset_of!(ID2D1GeometrySinkVtbl, BeginFigure) == 5 * slot);
    assert!(std::mem::offset_of!(ID2D1GeometrySinkVtbl, EndFigure) == 8 * slot);
    assert!(std::mem::offset_of!(ID2D1GeometrySinkVtbl, Close) == 9 * slot);
    assert!(std::mem::offset_of!(ID2D1GeometrySinkVtbl, AddLine) == 10 * slot);
    assert!(std::mem::offset_of!(ID2D1GeometrySinkVtbl, AddBezier) == 11 * slot);
    assert!(std::mem::offset_of!(ID2D1GeometrySinkVtbl, AddQuadraticBezier) == 12 * slot);
    assert!(std::mem::offset_of!(IDWriteFactoryVtbl, CreateTextFormat) == 15 * slot);
    assert!(std::mem::offset_of!(IDWriteFactoryVtbl, CreateTextLayout) == 18 * slot);
    assert!(std::mem::offset_of!(IDWriteTextFormatVtbl, SetTextAlignment) == 3 * slot);
    assert!(std::mem::offset_of!(IDWriteTextFormatVtbl, SetParagraphAlignment) == 4 * slot);
    assert!(std::mem::offset_of!(IDWriteTextLayoutVtbl, GetLineMetrics) == 59 * slot);
    assert!(std::mem::offset_of!(IDWriteTextLayoutVtbl, GetMetrics) == 60 * slot);
    assert!(size_of::<D2D_POINT_2F>() == 8);
    assert!(size_of::<D2D_MATRIX_3X2_F>() == 24);
    assert!(size_of::<D2D_SIZE_U>() == 8);
    assert!(size_of::<D2D_RECT_F>() == 16);
    assert!(size_of::<D2D1_COLOR_F>() == 16);
    assert!(size_of::<D2D1_PIXEL_FORMAT>() == 8);
    assert!(size_of::<D2D1_RENDER_TARGET_PROPERTIES>() == 28);
    assert!(size_of::<D2D1_HWND_RENDER_TARGET_PROPERTIES>() == if slot == 8 { 24 } else { 16 });
    assert!(size_of::<D2D1_STROKE_STYLE_PROPERTIES>() == 28);
    assert!(size_of::<D2D1_BEZIER_SEGMENT>() == 24);
    assert!(size_of::<D2D1_QUADRATIC_BEZIER_SEGMENT>() == 16);
    assert!(size_of::<D2D1_LAYER_PARAMETERS>() == if slot == 8 { 72 } else { 60 });
    assert!(size_of::<D2D1_GRADIENT_STOP>() == 20);
    assert!(size_of::<D2D1_BRUSH_PROPERTIES>() == 28);
    assert!(size_of::<D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES>() == 24);
    assert!(size_of::<D2D1_BITMAP_PROPERTIES>() == 16);
    assert!(size_of::<DWRITE_TEXT_METRICS>() == 36);
    assert!(size_of::<DWRITE_LINE_METRICS>() == 24);
};

#[cfg(test)]
mod tests {
    use std::ptr::{null, null_mut};

    use super::*;

    #[test]
    fn native_geometry_sink_and_stroke_style() {
        unsafe {
            let factory = ID2D1Factory::new().expect("Direct2D factory");
            let geometry = factory.CreatePathGeometry().expect("path");
            let sink = geometry.Open().expect("sink");
            sink.SetFillMode(D2D1_FILL_MODE_WINDING);
            sink.BeginFigure(D2D_POINT_2F::default(), D2D1_FIGURE_BEGIN_FILLED);
            sink.AddLine(D2D_POINT_2F { x: 10.0, y: 0.0 });
            sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                point1: D2D_POINT_2F { x: 15.0, y: 0.0 },
                point2: D2D_POINT_2F { x: 15.0, y: 10.0 },
                point3: D2D_POINT_2F { x: 10.0, y: 10.0 },
            });
            sink.AddQuadraticBezier(&D2D1_QUADRATIC_BEZIER_SEGMENT {
                point1: D2D_POINT_2F { x: 0.0, y: 10.0 },
                point2: D2D_POINT_2F::default(),
            });
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            sink.Close().expect("finish path");
            let style = factory
                .CreateStrokeStyle(
                    &D2D1_STROKE_STYLE_PROPERTIES {
                        miterLimit: 10.0,
                        ..Default::default()
                    },
                    null(),
                    0,
                )
                .expect("stroke style");
            let cloned = geometry.clone();
            drop(geometry);
            drop(cloned);
            drop(style);
        }
    }

    #[test]
    fn native_text_layout_metrics() {
        unsafe {
            let factory = IDWriteFactory::new().expect("DirectWrite factory");
            let family = wide_string("Segoe UI");
            let locale = wide_string("en-us");
            let format = factory
                .CreateTextFormat(
                    family.as_ptr(),
                    null_mut(),
                    DWRITE_FONT_WEIGHT_NORMAL,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    16.0,
                    locale.as_ptr(),
                )
                .expect("text format");
            format
                .SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING)
                .expect("alignment");
            format
                .SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR)
                .expect("paragraph");
            let text: Vec<u16> = "Reversi".encode_utf16().collect();
            let layout = factory
                .CreateTextLayout(
                    text.as_ptr(),
                    text.len() as u32,
                    format.as_ptr(),
                    1000.0,
                    1000.0,
                )
                .expect("text layout");
            drop(format);
            let mut metrics = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut metrics).expect("metrics");
            assert!(metrics.width > 0.0 && metrics.height > 0.0);
            assert_eq!(metrics.lineCount, 1);
            let mut line = DWRITE_LINE_METRICS::default();
            let mut count = 0;
            layout
                .GetLineMetrics(&mut line, 1, &mut count)
                .expect("line metrics");
            assert_eq!(count, 1);
            assert!(line.baseline > 0.0 && line.baseline < line.height);
        }
    }
}

// MARK: Window
// The Win32 API the canvas draws window client areas with.
mod window {
    use super::{BOOL, HWND, RECT};

    #[cfg(feature = "window")]
    pub(crate) const TRUE: BOOL = 1;
    pub(crate) const FALSE: BOOL = 0;

    #[link(name = "user32")]
    unsafe extern "system" {
        pub(crate) fn GetClientRect(hwnd: HWND, rect: *mut RECT) -> BOOL;
        pub(crate) fn InvalidateRect(hwnd: HWND, rect: *const RECT, erase: BOOL) -> BOOL;
        pub(crate) fn GetDpiForWindow(hwnd: HWND) -> u32;
        pub(crate) fn ValidateRect(hwnd: HWND, rect: *const RECT) -> BOOL;
    }
}
pub(super) use window::*;
