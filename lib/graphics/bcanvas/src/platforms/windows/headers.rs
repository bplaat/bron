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
    #[cfg(feature = "window")]
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
    _unused_3_7: [*const c_void; 5],
    CreateSolidColorBrush: unsafe extern "system" fn(
        *mut ID2D1RenderTarget,
        *const D2D1_COLOR_F,
        *const c_void,
        *mut *mut ID2D1SolidColorBrush,
    ) -> i32,
    _unused_9_15: [*const c_void; 7],
    DrawRectangle: unsafe extern "system" fn(
        *mut ID2D1RenderTarget,
        *const D2D_RECT_F,
        *mut ID2D1SolidColorBrush,
        f32,
        *mut ID2D1StrokeStyle,
    ),
    FillRectangle: unsafe extern "system" fn(
        *mut ID2D1RenderTarget,
        *const D2D_RECT_F,
        *mut ID2D1SolidColorBrush,
    ),
    _unused_18_21: [*const c_void; 4],
    DrawGeometry: unsafe extern "system" fn(
        *mut ID2D1RenderTarget,
        *mut ID2D1PathGeometry,
        *mut ID2D1SolidColorBrush,
        f32,
        *mut ID2D1StrokeStyle,
    ),
    FillGeometry: unsafe extern "system" fn(
        *mut ID2D1RenderTarget,
        *mut ID2D1PathGeometry,
        *mut ID2D1SolidColorBrush,
        *mut ID2D1SolidColorBrush,
    ),
    _unused_24_27: [*const c_void; 4],
    DrawTextLayout: unsafe extern "system" fn(
        *mut ID2D1RenderTarget,
        D2D_POINT_2F,
        *mut IDWriteTextLayout,
        *mut ID2D1SolidColorBrush,
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
    _unused_52_56: [*const c_void; 5],
}

impl ID2D1RenderTarget {
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
        brush: *mut ID2D1SolidColorBrush,
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
    pub(super) unsafe fn FillRectangle(
        &self,
        rect: *const D2D_RECT_F,
        brush: *mut ID2D1SolidColorBrush,
    ) {
        unsafe { ((*self.lpVtbl).FillRectangle)(self as *const Self as *mut Self, rect, brush) }
    }
    pub(super) unsafe fn DrawGeometry(
        &self,
        geometry: *mut ID2D1PathGeometry,
        brush: *mut ID2D1SolidColorBrush,
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
        brush: *mut ID2D1SolidColorBrush,
        opacity_brush: *mut ID2D1SolidColorBrush,
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
        brush: *mut ID2D1SolidColorBrush,
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
    #[cfg(feature = "window")]
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
    #[cfg(feature = "window")]
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

/// Direct2D 1.1 interface, queried from a render target on Windows 8 and later.
#[repr(C)]
pub(super) struct ID2D1DeviceContext {
    lpVtbl: *const ID2D1DeviceContextVtbl,
}

#[repr(C)]
struct ID2D1DeviceContextVtbl {
    base: ID2D1RenderTargetVtbl,
    _unused_57_77: [*const c_void; 21],
    SetPrimitiveBlend: unsafe extern "system" fn(*mut ID2D1DeviceContext, u32),
}

impl ID2D1DeviceContext {
    pub(super) unsafe fn SetPrimitiveBlend(&self, blend: u32) {
        unsafe { ((*self.lpVtbl).SetPrimitiveBlend)(self as *const Self as *mut Self, blend) }
    }
}

#[repr(C)]
pub(super) struct ID2D1PathGeometry {
    lpVtbl: *const ID2D1PathGeometryVtbl,
}

#[repr(C)]
struct ID2D1PathGeometryVtbl {
    base: IUnknownVtbl,
    _unused_3_16: [*const c_void; 14],
    Open: unsafe extern "system" fn(*mut ID2D1PathGeometry, *mut *mut ID2D1GeometrySink) -> i32,
}

impl ID2D1PathGeometry {
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
pub(super) struct D2D1_LAYER_PARAMETERS {
    pub(super) contentBounds: D2D_RECT_F,
    pub(super) geometricMask: *mut ID2D1PathGeometry,
    pub(super) maskAntialiasMode: u32,
    pub(super) maskTransform: D2D_MATRIX_3X2_F,
    pub(super) opacity: f32,
    pub(super) opacityBrush: *mut ID2D1SolidColorBrush,
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

#[cfg(feature = "window")]
pub(super) const D2D1_ALPHA_MODE_UNKNOWN: u32 = 0;
pub(super) const D2D1_ALPHA_MODE_PREMULTIPLIED: u32 = 1;
pub(super) const D2D1_RENDER_TARGET_TYPE_SOFTWARE: u32 = 1;
#[cfg(feature = "window")]
pub(super) const DXGI_FORMAT_UNKNOWN: u32 = 0;
pub(super) const DXGI_FORMAT_B8G8R8A8_UNORM: u32 = 87;
pub(super) const D2D1_ANTIALIAS_MODE_PER_PRIMITIVE: u32 = 0;
pub(super) const D2D1_ANTIALIAS_MODE_ALIASED: u32 = 1;
pub(super) const D2D1_FILL_MODE_WINDING: u32 = 1;
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
    assert!(std::mem::offset_of!(ID2D1RenderTargetVtbl, CreateSolidColorBrush) == 8 * slot);
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
// The Win32 API the canvas child window uses.
#[cfg(feature = "window")]
mod window {
    use std::ffi::c_void;

    use super::{BOOL, HANDLE, HDC, HWND, RECT};

    pub(crate) type HCURSOR = HANDLE;
    pub(crate) type HDROP = HANDLE;
    pub(crate) type HMODULE = *const c_void;
    pub(crate) type WPARAM = usize;
    pub(crate) type LPARAM = isize;
    pub(crate) type LRESULT = isize;

    pub(crate) const TRUE: BOOL = 1;
    pub(crate) const FALSE: BOOL = 0;

    #[repr(C)]
    pub(crate) struct POINT {
        pub(crate) x: i32,
        pub(crate) y: i32,
    }

    #[repr(C)]
    #[derive(Default)]
    pub(crate) struct WNDCLASSEXW {
        pub(crate) cbSize: u32,
        pub(crate) style: u32,
        pub(crate) lpfnWndProc:
            Option<unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT>,
        pub(crate) cbClsExtra: i32,
        pub(crate) cbWndExtra: i32,
        pub(crate) hInstance: HMODULE,
        pub(crate) hIcon: HANDLE,
        pub(crate) hCursor: HCURSOR,
        pub(crate) hbrBackground: usize,
        pub(crate) lpszMenuName: *const u16,
        pub(crate) lpszClassName: *const u16,
        pub(crate) hIconSm: HANDLE,
    }

    #[repr(C)]
    pub(crate) struct CREATESTRUCTW {
        pub(crate) lpCreateParams: *mut c_void,
        pub(crate) hInstance: HMODULE,
        pub(crate) hMenu: HANDLE,
        pub(crate) hwndParent: HWND,
        pub(crate) cy: i32,
        pub(crate) cx: i32,
        pub(crate) y: i32,
        pub(crate) x: i32,
        pub(crate) style: i32,
        pub(crate) lpszName: *const u16,
        pub(crate) lpszClass: *const u16,
        pub(crate) dwExStyle: u32,
    }

    #[repr(C)]
    #[derive(Default)]
    pub(crate) struct PAINTSTRUCT {
        pub(crate) hdc: HDC,
        pub(crate) fErase: BOOL,
        pub(crate) rcPaint: RECT,
        pub(crate) fRestore: BOOL,
        pub(crate) fIncUpdate: BOOL,
        pub(crate) rgbReserved: [u8; 32],
    }

    #[repr(C)]
    #[derive(Default)]
    pub(crate) struct TRACKMOUSEEVENT {
        pub(crate) cbSize: u32,
        pub(crate) dwFlags: u32,
        pub(crate) hwndTrack: HWND,
        pub(crate) dwHoverTime: u32,
    }

    const _: () = {
        let wide = size_of::<HWND>() == 8;
        assert!(size_of::<TRACKMOUSEEVENT>() == if wide { 24 } else { 16 });
        assert!(size_of::<PAINTSTRUCT>() == if wide { 72 } else { 64 });
    };

    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub(crate) fn GetModuleHandleW(name: *const u16) -> HMODULE;
    }

    #[link(name = "shell32")]
    unsafe extern "system" {
        pub(crate) fn DragAcceptFiles(hwnd: HWND, accept: BOOL);
        pub(crate) fn DragQueryFileW(
            drop: HDROP,
            file: u32,
            buffer: *mut u16,
            buffer_size: u32,
        ) -> u32;
        pub(crate) fn DragFinish(drop: HDROP);
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        pub(crate) fn GetClassInfoExW(
            instance: HMODULE,
            class: *const u16,
            class_info: *mut WNDCLASSEXW,
        ) -> BOOL;
        pub(crate) fn RegisterClassExW(class_info: *const WNDCLASSEXW) -> u16;
        pub(crate) fn CreateWindowExW(
            ex_style: u32,
            class: *const u16,
            name: *const u16,
            style: u32,
            x: i32,
            y: i32,
            width: i32,
            height: i32,
            parent: HWND,
            menu: HANDLE,
            instance: HMODULE,
            param: LPARAM,
        ) -> HWND;
        pub(crate) fn DestroyWindow(hwnd: HWND) -> BOOL;
        pub(crate) fn DefWindowProcW(
            hwnd: HWND,
            message: u32,
            wparam: WPARAM,
            lparam: LPARAM,
        ) -> LRESULT;
        pub(crate) fn GetClientRect(hwnd: HWND, rect: *mut RECT) -> BOOL;
        pub(crate) fn SetWindowPos(
            hwnd: HWND,
            insert_after: HWND,
            x: i32,
            y: i32,
            cx: i32,
            cy: i32,
            flags: u32,
        ) -> BOOL;
        pub(crate) fn BeginPaint(hwnd: HWND, paint: *mut PAINTSTRUCT) -> HDC;
        pub(crate) fn EndPaint(hwnd: HWND, paint: *const PAINTSTRUCT) -> BOOL;
        pub(crate) fn InvalidateRect(hwnd: HWND, rect: *const RECT, erase: BOOL) -> BOOL;
        pub(crate) fn GetDpiForWindow(hwnd: HWND) -> u32;
        #[cfg(target_pointer_width = "64")]
        #[link_name = "GetWindowLongPtrW"]
        pub(crate) fn GetWindowLong(hwnd: HWND, index: i32) -> isize;
        #[cfg(target_pointer_width = "64")]
        #[link_name = "SetWindowLongPtrW"]
        pub(crate) fn SetWindowLong(hwnd: HWND, index: i32, value: isize) -> isize;
        #[cfg(target_pointer_width = "32")]
        #[link_name = "GetWindowLongW"]
        pub(crate) fn GetWindowLong(hwnd: HWND, index: i32) -> isize;
        #[cfg(target_pointer_width = "32")]
        #[link_name = "SetWindowLongW"]
        pub(crate) fn SetWindowLong(hwnd: HWND, index: i32, value: isize) -> isize;
        pub(crate) fn GetCursorPos(point: *mut POINT) -> BOOL;
        pub(crate) fn WindowFromPoint(point: POINT) -> HWND;
        pub(crate) fn SetCursor(cursor: HCURSOR) -> HCURSOR;
        pub(crate) fn LoadCursorW(instance: HMODULE, name: *const u16) -> HCURSOR;
        pub(crate) fn SetFocus(hwnd: HWND) -> HWND;
        pub(crate) fn SetCapture(hwnd: HWND) -> HWND;
        pub(crate) fn ReleaseCapture() -> BOOL;
        pub(crate) fn TrackMouseEvent(event: *mut TRACKMOUSEEVENT) -> BOOL;
    }
}
#[cfg(feature = "window")]
pub(super) use window::*;
