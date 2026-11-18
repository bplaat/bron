/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// Minimal GDI+ flat API.
#![allow(non_camel_case_types, non_snake_case, non_upper_case_globals)]
#![allow(clippy::too_many_arguments, clippy::upper_case_acronyms)]
#![allow(unused)]

use std::ffi::c_void;

use crate::platforms::windows::headers::*;

pub(super) type GpStatus = i32;
pub(super) const GP_OK: GpStatus = 0;

macro_rules! opaque {
    ($($name:ident),* $(,)?) => {
        $(
            #[repr(C)]
            pub(super) struct $name {
                _private: [u8; 0],
            }
        )*
    };
}
opaque!(
    GpImage,
    GpGraphics,
    GpMatrix,
    GpPath,
    GpBrush,
    GpPen,
    GpRegion,
    GpFontFamily,
    GpFontCollection,
    GpFont,
    GpStringFormat,
    GpImageAttributes,
);

#[repr(C)]
pub(super) struct GdiplusStartupInput {
    pub(super) version: u32,
    pub(super) debug_event_callback: *const c_void,
    pub(super) suppress_background_thread: BOOL,
    pub(super) suppress_external_codecs: BOOL,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub(super) struct RectF {
    pub(super) x: f32,
    pub(super) y: f32,
    pub(super) width: f32,
    pub(super) height: f32,
}

#[repr(C)]
pub(super) struct ColorMatrix {
    pub(super) m: [[f32; 5]; 5],
}

pub(super) const PixelFormat32bppPARGB: i32 = 0x000e_200b;
pub(super) const UnitWorld: i32 = 0;
pub(super) const UnitPixel: i32 = 2;
pub(super) const FillModeAlternate: i32 = 0;
pub(super) const FillModeWinding: i32 = 1;
pub(super) const SmoothingModeNone: i32 = 3;
pub(super) const SmoothingModeAntiAlias: i32 = 4;
pub(super) const PixelOffsetModeHalf: i32 = 4;
pub(super) const CompositingModeSourceOver: i32 = 0;
pub(super) const CompositingModeSourceCopy: i32 = 1;
pub(super) const TextRenderingHintAntiAlias: i32 = 4;
pub(super) const InterpolationModeNearestNeighbor: i32 = 5;
pub(super) const InterpolationModeHighQualityBicubic: i32 = 7;
pub(super) const CombineModeReplace: i32 = 0;
pub(super) const CombineModeIntersect: i32 = 1;
pub(super) const WrapModeTileFlipXY: i32 = 3;
pub(super) const WrapModeClamp: i32 = 4;
pub(super) const LineCapFlat: i32 = 0;
pub(super) const LineCapSquare: i32 = 1;
pub(super) const LineCapRound: i32 = 2;
pub(super) const DashCapFlat: i32 = 0;
pub(super) const DashCapRound: i32 = 2;
pub(super) const LineJoinBevel: i32 = 1;
pub(super) const LineJoinRound: i32 = 2;
pub(super) const LineJoinMiterClipped: i32 = 3;
pub(super) const FontStyleRegular: i32 = 0;
pub(super) const FontStyleBold: i32 = 1;
pub(super) const StringFormatFlagsMeasureTrailingSpaces: i32 = 0x0800;
pub(super) const StringFormatFlagsNoWrap: i32 = 0x1000;
pub(super) const StringFormatFlagsNoClip: i32 = 0x4000;
pub(super) const ColorAdjustTypeDefault: i32 = 0;
pub(super) const ColorMatrixFlagsDefault: i32 = 0;
pub(super) const FlushIntentionSync: i32 = 1;

// GDI fills of the canvas bitmaps.
pub(super) const PATCOPY: u32 = 0x00f0_0021;

#[link(name = "gdi32")]
unsafe extern "system" {
    pub(super) fn CreateDIBPatternBrushPt(packed: *const c_void, usage: u32) -> HANDLE;
    pub(super) fn PatBlt(dc: HDC, x: i32, y: i32, width: i32, height: i32, rop: u32) -> BOOL;
}

#[link(name = "gdiplus")]
unsafe extern "system" {
    pub(super) fn GdiplusStartup(
        token: *mut usize,
        input: *const GdiplusStartupInput,
        output: *mut c_void,
    ) -> GpStatus;

    pub(super) fn GdipCreateBitmapFromScan0(
        width: i32,
        height: i32,
        stride: i32,
        format: i32,
        scan0: *mut u8,
        bitmap: *mut *mut GpImage,
    ) -> GpStatus;
    pub(super) fn GdipDisposeImage(image: *mut GpImage) -> GpStatus;
    pub(super) fn GdipGetImageGraphicsContext(
        image: *mut GpImage,
        graphics: *mut *mut GpGraphics,
    ) -> GpStatus;

    pub(super) fn GdipDeleteGraphics(graphics: *mut GpGraphics) -> GpStatus;
    pub(super) fn GdipFlush(graphics: *mut GpGraphics, intention: i32) -> GpStatus;
    pub(super) fn GdipSetPageUnit(graphics: *mut GpGraphics, unit: i32) -> GpStatus;
    pub(super) fn GdipSetSmoothingMode(graphics: *mut GpGraphics, mode: i32) -> GpStatus;
    pub(super) fn GdipSetPixelOffsetMode(graphics: *mut GpGraphics, mode: i32) -> GpStatus;
    pub(super) fn GdipSetCompositingMode(graphics: *mut GpGraphics, mode: i32) -> GpStatus;
    pub(super) fn GdipSetTextRenderingHint(graphics: *mut GpGraphics, hint: i32) -> GpStatus;
    pub(super) fn GdipSetInterpolationMode(graphics: *mut GpGraphics, mode: i32) -> GpStatus;
    pub(super) fn GdipSetWorldTransform(
        graphics: *mut GpGraphics,
        matrix: *mut GpMatrix,
    ) -> GpStatus;
    pub(super) fn GdipResetWorldTransform(graphics: *mut GpGraphics) -> GpStatus;

    pub(super) fn GdipCreateMatrix2(
        m11: f32,
        m12: f32,
        m21: f32,
        m22: f32,
        dx: f32,
        dy: f32,
        matrix: *mut *mut GpMatrix,
    ) -> GpStatus;
    pub(super) fn GdipSetMatrixElements(
        matrix: *mut GpMatrix,
        m11: f32,
        m12: f32,
        m21: f32,
        m22: f32,
        dx: f32,
        dy: f32,
    ) -> GpStatus;
    pub(super) fn GdipDeleteMatrix(matrix: *mut GpMatrix) -> GpStatus;

    pub(super) fn GdipCreatePath(fill_mode: i32, path: *mut *mut GpPath) -> GpStatus;
    pub(super) fn GdipDeletePath(path: *mut GpPath) -> GpStatus;
    pub(super) fn GdipClonePath(path: *mut GpPath, clone: *mut *mut GpPath) -> GpStatus;
    pub(super) fn GdipGetPathWorldBounds(
        path: *mut GpPath,
        bounds: *mut RectF,
        matrix: *mut GpMatrix,
        pen: *mut GpPen,
    ) -> GpStatus;
    pub(super) fn GdipStartPathFigure(path: *mut GpPath) -> GpStatus;
    pub(super) fn GdipClosePathFigure(path: *mut GpPath) -> GpStatus;
    pub(super) fn GdipAddPathLine(
        path: *mut GpPath,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
    ) -> GpStatus;
    pub(super) fn GdipAddPathBezier(
        path: *mut GpPath,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        x3: f32,
        y3: f32,
        x4: f32,
        y4: f32,
    ) -> GpStatus;
    pub(super) fn GdipAddPathRectangle(
        path: *mut GpPath,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> GpStatus;
    pub(super) fn GdipWidenPath(
        path: *mut GpPath,
        pen: *mut GpPen,
        matrix: *mut GpMatrix,
        flatness: f32,
    ) -> GpStatus;
    pub(super) fn GdipFillPath(
        graphics: *mut GpGraphics,
        brush: *mut GpBrush,
        path: *mut GpPath,
    ) -> GpStatus;
    pub(super) fn GdipDrawPath(
        graphics: *mut GpGraphics,
        pen: *mut GpPen,
        path: *mut GpPath,
    ) -> GpStatus;
    pub(super) fn GdipFillRectangle(
        graphics: *mut GpGraphics,
        brush: *mut GpBrush,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> GpStatus;
    pub(super) fn GdipFillEllipse(
        graphics: *mut GpGraphics,
        brush: *mut GpBrush,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> GpStatus;

    pub(super) fn GdipCreateSolidFill(argb: u32, brush: *mut *mut GpBrush) -> GpStatus;
    pub(super) fn GdipCreateTexture(
        image: *mut GpImage,
        wrap_mode: i32,
        texture: *mut *mut GpBrush,
    ) -> GpStatus;
    pub(super) fn GdipSetTextureTransform(brush: *mut GpBrush, matrix: *mut GpMatrix) -> GpStatus;
    pub(super) fn GdipDeleteBrush(brush: *mut GpBrush) -> GpStatus;

    pub(super) fn GdipCreatePen2(
        brush: *mut GpBrush,
        width: f32,
        unit: i32,
        pen: *mut *mut GpPen,
    ) -> GpStatus;
    pub(super) fn GdipDeletePen(pen: *mut GpPen) -> GpStatus;
    pub(super) fn GdipSetPenLineCap197819(
        pen: *mut GpPen,
        start_cap: i32,
        end_cap: i32,
        dash_cap: i32,
    ) -> GpStatus;
    pub(super) fn GdipSetPenLineJoin(pen: *mut GpPen, join: i32) -> GpStatus;
    pub(super) fn GdipSetPenMiterLimit(pen: *mut GpPen, limit: f32) -> GpStatus;
    pub(super) fn GdipSetPenDashArray(pen: *mut GpPen, dashes: *const f32, count: i32) -> GpStatus;
    pub(super) fn GdipSetPenDashOffset(pen: *mut GpPen, offset: f32) -> GpStatus;

    pub(super) fn GdipCreateRegion(region: *mut *mut GpRegion) -> GpStatus;
    pub(super) fn GdipDeleteRegion(region: *mut GpRegion) -> GpStatus;
    pub(super) fn GdipGetClip(graphics: *mut GpGraphics, region: *mut GpRegion) -> GpStatus;
    pub(super) fn GdipGetClipBounds(graphics: *mut GpGraphics, bounds: *mut RectF) -> GpStatus;
    pub(super) fn GdipSetClipRegion(
        graphics: *mut GpGraphics,
        region: *mut GpRegion,
        mode: i32,
    ) -> GpStatus;
    pub(super) fn GdipSetClipPath(
        graphics: *mut GpGraphics,
        path: *mut GpPath,
        mode: i32,
    ) -> GpStatus;

    pub(super) fn GdipCreateImageAttributes(attributes: *mut *mut GpImageAttributes) -> GpStatus;
    pub(super) fn GdipDisposeImageAttributes(attributes: *mut GpImageAttributes) -> GpStatus;
    pub(super) fn GdipSetImageAttributesColorMatrix(
        attributes: *mut GpImageAttributes,
        adjust_type: i32,
        enable: BOOL,
        matrix: *const ColorMatrix,
        gray_matrix: *const ColorMatrix,
        flags: i32,
    ) -> GpStatus;
    pub(super) fn GdipSetImageAttributesWrapMode(
        attributes: *mut GpImageAttributes,
        wrap: i32,
        argb: u32,
        clamp: BOOL,
    ) -> GpStatus;
    pub(super) fn GdipDrawImageRectRect(
        graphics: *mut GpGraphics,
        image: *mut GpImage,
        dst_x: f32,
        dst_y: f32,
        dst_width: f32,
        dst_height: f32,
        src_x: f32,
        src_y: f32,
        src_width: f32,
        src_height: f32,
        src_unit: i32,
        attributes: *mut GpImageAttributes,
        callback: *const c_void,
        callback_data: *const c_void,
    ) -> GpStatus;

    pub(super) fn GdipCreateFontFamilyFromName(
        name: *const u16,
        collection: *mut GpFontCollection,
        family: *mut *mut GpFontFamily,
    ) -> GpStatus;
    pub(super) fn GdipGetGenericFontFamilySansSerif(family: *mut *mut GpFontFamily) -> GpStatus;
    pub(super) fn GdipDeleteFontFamily(family: *mut GpFontFamily) -> GpStatus;
    pub(super) fn GdipGetEmHeight(
        family: *mut GpFontFamily,
        style: i32,
        height: *mut u16,
    ) -> GpStatus;
    pub(super) fn GdipGetCellAscent(
        family: *mut GpFontFamily,
        style: i32,
        ascent: *mut u16,
    ) -> GpStatus;
    pub(super) fn GdipGetLineSpacing(
        family: *mut GpFontFamily,
        style: i32,
        spacing: *mut u16,
    ) -> GpStatus;
    pub(super) fn GdipCreateFont(
        family: *mut GpFontFamily,
        size: f32,
        style: i32,
        unit: i32,
        font: *mut *mut GpFont,
    ) -> GpStatus;
    pub(super) fn GdipDeleteFont(font: *mut GpFont) -> GpStatus;

    pub(super) fn GdipStringFormatGetGenericTypographic(
        format: *mut *mut GpStringFormat,
    ) -> GpStatus;
    pub(super) fn GdipCloneStringFormat(
        format: *mut GpStringFormat,
        clone: *mut *mut GpStringFormat,
    ) -> GpStatus;
    pub(super) fn GdipSetStringFormatFlags(format: *mut GpStringFormat, flags: i32) -> GpStatus;
    pub(super) fn GdipDeleteStringFormat(format: *mut GpStringFormat) -> GpStatus;
    pub(super) fn GdipDrawString(
        graphics: *mut GpGraphics,
        text: *const u16,
        length: i32,
        font: *mut GpFont,
        layout: *const RectF,
        format: *mut GpStringFormat,
        brush: *mut GpBrush,
    ) -> GpStatus;
    pub(super) fn GdipMeasureString(
        graphics: *mut GpGraphics,
        text: *const u16,
        length: i32,
        font: *mut GpFont,
        layout: *const RectF,
        format: *mut GpStringFormat,
        bounds: *mut RectF,
        codepoints_fitted: *mut i32,
        lines_filled: *mut i32,
    ) -> GpStatus;
}

/// Start GDI+ once for the process, it stays running until the process exits.
pub(super) fn startup() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        let mut token = 0;
        let input = GdiplusStartupInput {
            version: 1,
            debug_event_callback: std::ptr::null(),
            suppress_background_thread: FALSE,
            suppress_external_codecs: TRUE,
        };
        let status = unsafe { GdiplusStartup(&mut token, &input, std::ptr::null_mut()) };
        assert_eq!(status, GP_OK, "GDI+ startup");
    });
}

/// An owned GDI+ object, deleted with the function of its type.
pub(super) struct Gp<T: GpDelete>(pub(super) *mut T);

pub(super) trait GpDelete {
    unsafe fn delete(object: *mut Self);
}

impl<T: GpDelete> Gp<T> {
    /// Create an object through its out parameter, `None` when GDI+ fails.
    pub(super) unsafe fn create(create: impl FnOnce(*mut *mut T) -> GpStatus) -> Option<Self> {
        let mut object = std::ptr::null_mut();
        let status = create(&mut object);
        if object.is_null() {
            return None;
        }
        let object = Self(object);
        (status == GP_OK).then_some(object)
    }

    pub(super) const fn get(&self) -> *mut T {
        self.0
    }
}

impl<T: GpDelete> Drop for Gp<T> {
    fn drop(&mut self) {
        unsafe { T::delete(self.0) };
    }
}

macro_rules! deletes {
    ($($type:ident => $delete:ident),* $(,)?) => {
        $(
            impl GpDelete for $type {
                unsafe fn delete(object: *mut Self) {
                    unsafe { $delete(object) };
                }
            }
        )*
    };
}
deletes!(
    GpImage => GdipDisposeImage,
    GpGraphics => GdipDeleteGraphics,
    GpMatrix => GdipDeleteMatrix,
    GpPath => GdipDeletePath,
    GpBrush => GdipDeleteBrush,
    GpPen => GdipDeletePen,
    GpRegion => GdipDeleteRegion,
    GpFontFamily => GdipDeleteFontFamily,
    GpFont => GdipDeleteFont,
    GpStringFormat => GdipDeleteStringFormat,
    GpImageAttributes => GdipDisposeImageAttributes,
);
