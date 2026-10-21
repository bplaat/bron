/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::c_void;

use bwindow::ffi::CGRect;

#[cfg(target_os = "macos")]
pub(super) const NS_BOLD_FONT_MASK: usize = 1 << 1;
#[cfg(target_os = "ios")]
pub(super) const UI_FONT_DESCRIPTOR_TRAIT_BOLD: u32 = 1 << 1;

pub(super) type CGContextRef = *mut c_void;
#[cfg(target_os = "macos")]
pub(super) type CVDisplayLinkRef = *mut c_void;
#[cfg(target_os = "macos")]
pub(super) type CVDisplayLinkOutputCallback = unsafe extern "C" fn(
    CVDisplayLinkRef,
    *const c_void,
    *const c_void,
    u64,
    *mut u64,
    *mut c_void,
) -> i32;

#[cfg(target_os = "macos")]
#[link(name = "CoreVideo", kind = "framework")]
unsafe extern "C" {
    pub(super) fn CVDisplayLinkCreateWithActiveCGDisplays(
        display_link: *mut CVDisplayLinkRef,
    ) -> i32;
    pub(super) fn CVDisplayLinkSetOutputCallback(
        display_link: CVDisplayLinkRef,
        callback: CVDisplayLinkOutputCallback,
        user_info: *mut c_void,
    ) -> i32;
    pub(super) fn CVDisplayLinkSetCurrentCGDisplay(
        display_link: CVDisplayLinkRef,
        display_id: u32,
    ) -> i32;
    pub(super) fn CVDisplayLinkStart(display_link: CVDisplayLinkRef) -> i32;
    pub(super) fn CVDisplayLinkStop(display_link: CVDisplayLinkRef) -> i32;
    pub(super) fn CVDisplayLinkRelease(display_link: CVDisplayLinkRef);
}

#[cfg(target_os = "ios")]
#[link(name = "UIKit", kind = "framework")]
unsafe extern "C" {
    pub(super) fn UIGraphicsGetCurrentContext() -> CGContextRef;
    pub(super) fn UIGraphicsPushContext(context: CGContextRef);
    pub(super) fn UIGraphicsPopContext();
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    pub(super) fn CGContextSaveGState(context: CGContextRef);
    pub(super) fn CGContextRestoreGState(context: CGContextRef);
    pub(super) fn CGContextFlush(context: CGContextRef);
    pub(super) fn CGContextClearRect(context: CGContextRef, rect: CGRect);
    pub(super) fn CGContextSetRGBFillColor(context: CGContextRef, r: f64, g: f64, b: f64, a: f64);
    pub(super) fn CGContextSetRGBStrokeColor(context: CGContextRef, r: f64, g: f64, b: f64, a: f64);
    pub(super) fn CGContextSetLineWidth(context: CGContextRef, width: f64);
    pub(super) fn CGContextSetLineCap(context: CGContextRef, cap: i32);
    pub(super) fn CGContextSetLineJoin(context: CGContextRef, join: i32);
    pub(super) fn CGContextFillRect(context: CGContextRef, rect: CGRect);
    pub(super) fn CGContextStrokeRect(context: CGContextRef, rect: CGRect);
    pub(super) fn CGContextBeginPath(context: CGContextRef);
    pub(super) fn CGContextClosePath(context: CGContextRef);
    pub(super) fn CGContextMoveToPoint(context: CGContextRef, x: f64, y: f64);
    pub(super) fn CGContextAddLineToPoint(context: CGContextRef, x: f64, y: f64);
    pub(super) fn CGContextAddRect(context: CGContextRef, rect: CGRect);
    pub(super) fn CGContextAddEllipseInRect(context: CGContextRef, rect: CGRect);
    pub(super) fn CGPathCreateWithRoundedRect(
        rect: CGRect,
        corner_width: f64,
        corner_height: f64,
        transform: *const CGAffineTransform,
    ) -> *mut c_void;
    pub(super) fn CGContextAddArc(
        context: CGContextRef,
        x: f64,
        y: f64,
        radius: f64,
        start: f64,
        end: f64,
        clockwise: i32,
    );
    pub(super) fn CGContextAddQuadCurveToPoint(
        context: CGContextRef,
        cpx: f64,
        cpy: f64,
        x: f64,
        y: f64,
    );
    pub(super) fn CGContextAddCurveToPoint(
        context: CGContextRef,
        cp1x: f64,
        cp1y: f64,
        cp2x: f64,
        cp2y: f64,
        x: f64,
        y: f64,
    );
    pub(super) fn CGContextFillPath(context: CGContextRef);
    pub(super) fn CGContextStrokePath(context: CGContextRef);
    pub(super) fn CGContextIsPathEmpty(context: CGContextRef) -> bool;
    pub(super) fn CGContextCopyPath(context: CGContextRef) -> *mut c_void;
    pub(super) fn CGContextAddPath(context: CGContextRef, path: *mut c_void);
    pub(super) fn CGPathRelease(path: *mut c_void);
    pub(super) fn CGContextClip(context: CGContextRef);
    pub(super) fn CGContextTranslateCTM(context: CGContextRef, x: f64, y: f64);
    pub(super) fn CGContextRotateCTM(context: CGContextRef, angle: f64);
    pub(super) fn CGContextScaleCTM(context: CGContextRef, x: f64, y: f64);
    pub(super) fn CGContextConcatCTM(context: CGContextRef, transform: CGAffineTransform);
    pub(super) fn CGContextGetCTM(context: CGContextRef) -> CGAffineTransform;
    pub(super) fn CGAffineTransformInvert(transform: CGAffineTransform) -> CGAffineTransform;
    pub(super) fn CGAffineTransformConcat(
        t1: CGAffineTransform,
        t2: CGAffineTransform,
    ) -> CGAffineTransform;
}

#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct CGAffineTransform {
    pub(super) a: f64,
    pub(super) b: f64,
    pub(super) c: f64,
    pub(super) d: f64,
    pub(super) tx: f64,
    pub(super) ty: f64,
}

unsafe extern "C" {
    pub(super) fn CGColorSpaceCreateDeviceRGB() -> *mut c_void;
    pub(super) fn CGColorSpaceRelease(space: *mut c_void);
    pub(super) fn CGBitmapContextCreate(
        data: *mut c_void,
        width: usize,
        height: usize,
        bits_per_component: usize,
        bytes_per_row: usize,
        space: *mut c_void,
        bitmap_info: u32,
    ) -> CGContextRef;
    pub(super) fn CGContextRelease(context: CGContextRef);
}
