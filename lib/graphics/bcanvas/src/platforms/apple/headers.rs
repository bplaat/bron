/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::c_void;
use std::ptr::{NonNull, null};

pub(super) type CGContextRef = *mut c_void;
pub(super) type CFTypeRef = *const c_void;
pub(super) type CFStringRef = *const c_void;
pub(super) type CFDictionaryRef = *const c_void;
pub(super) type CTFontRef = *const c_void;
#[cfg(all(feature = "window", target_os = "macos"))]
pub(super) type CVDisplayLinkRef = *mut c_void;
#[cfg(all(feature = "window", target_os = "macos"))]
pub(super) type CVDisplayLinkOutputCallback = unsafe extern "C" fn(
    CVDisplayLinkRef,
    *const c_void,
    *const c_void,
    u64,
    *mut u64,
    *mut c_void,
) -> i32;

pub(super) const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
pub(super) const K_CF_COMPARE_CASE_INSENSITIVE: usize = 1;
pub(super) const K_CT_FONT_UI_FONT_SYSTEM: u32 = 2;
pub(super) const K_CT_FONT_BOLD_TRAIT: u32 = 1 << 1;

#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct CGPoint {
    pub(super) x: f64,
    pub(super) y: f64,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct CGSize {
    pub(super) width: f64,
    pub(super) height: f64,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct CGRect {
    pub(super) origin: CGPoint,
    pub(super) size: CGSize,
}

// Window views pass the geometry through Objective-C messages.
#[cfg(feature = "window")]
mod encode {
    use objc2::{Encode, Encoding};

    use super::{CGPoint, CGRect, CGSize};

    unsafe impl Encode for CGPoint {
        const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
    }
    unsafe impl Encode for CGSize {
        const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
    }
    unsafe impl Encode for CGRect {
        const ENCODING: Encoding =
            Encoding::Struct("CGRect", &[CGPoint::ENCODING, CGSize::ENCODING]);
    }
}

#[cfg(all(feature = "window", target_os = "macos"))]
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

#[cfg(all(feature = "window", target_os = "ios"))]
#[link(name = "UIKit", kind = "framework")]
unsafe extern "C" {
    pub(super) fn UIGraphicsGetCurrentContext() -> CGContextRef;
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
    pub(super) fn CGContextSetTextMatrix(context: CGContextRef, transform: CGAffineTransform);
    pub(super) fn CGContextSetTextPosition(context: CGContextRef, x: f64, y: f64);
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub(super) static kCFBooleanTrue: CFTypeRef;
    pub(super) static kCFTypeDictionaryKeyCallBacks: c_void;
    pub(super) static kCFTypeDictionaryValueCallBacks: c_void;
    pub(super) fn CFRelease(object: CFTypeRef);
    pub(super) fn CFStringCreateWithBytes(
        allocator: *const c_void,
        bytes: *const u8,
        length: isize,
        encoding: u32,
        external_representation: bool,
    ) -> CFStringRef;
    pub(super) fn CFStringCompare(a: CFStringRef, b: CFStringRef, options: usize) -> isize;
    pub(super) fn CFDictionaryCreate(
        allocator: *const c_void,
        keys: *const CFTypeRef,
        values: *const CFTypeRef,
        count: isize,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> CFDictionaryRef;
    #[cfg(test)]
    pub(super) fn CFDictionaryGetValue(dictionary: CFDictionaryRef, key: CFTypeRef) -> CFTypeRef;
    pub(super) fn CFAttributedStringCreate(
        allocator: *const c_void,
        string: CFStringRef,
        attributes: CFDictionaryRef,
    ) -> CFTypeRef;
}

#[link(name = "CoreText", kind = "framework")]
unsafe extern "C" {
    pub(super) static kCTFontAttributeName: CFStringRef;
    pub(super) static kCTForegroundColorFromContextAttributeName: CFStringRef;
    pub(super) fn CTFontCreateWithName(
        name: CFStringRef,
        size: f64,
        matrix: *const CGAffineTransform,
    ) -> CTFontRef;
    pub(super) fn CTFontCreateUIFontForLanguage(
        font_type: u32,
        size: f64,
        language: CFStringRef,
    ) -> CTFontRef;
    pub(super) fn CTFontCreateCopyWithSymbolicTraits(
        font: CTFontRef,
        size: f64,
        matrix: *const CGAffineTransform,
        value: u32,
        mask: u32,
    ) -> CTFontRef;
    pub(super) fn CTFontCopyFamilyName(font: CTFontRef) -> CFStringRef;
    pub(super) fn CTFontCopyFullName(font: CTFontRef) -> CFStringRef;
    pub(super) fn CTFontCopyPostScriptName(font: CTFontRef) -> CFStringRef;
    pub(super) fn CTFontGetAscent(font: CTFontRef) -> f64;
    pub(super) fn CTFontGetDescent(font: CTFontRef) -> f64;
    #[cfg(test)]
    pub(super) fn CTFontGetSize(font: CTFontRef) -> f64;
    #[cfg(test)]
    pub(super) fn CTFontGetSymbolicTraits(font: CTFontRef) -> u32;
    pub(super) fn CTLineCreateWithAttributedString(string: CFTypeRef) -> CFTypeRef;
    pub(super) fn CTLineGetTypographicBounds(
        line: CFTypeRef,
        ascent: *mut f64,
        descent: *mut f64,
        leading: *mut f64,
    ) -> f64;
    pub(super) fn CTLineDraw(line: CFTypeRef, context: CGContextRef);
}

/// An owned Core Foundation object, released on drop.
pub(super) struct CFObject(NonNull<c_void>);

impl CFObject {
    /// Take ownership of a +1 reference from a Create or Copy function.
    pub(super) unsafe fn from_create(object: CFTypeRef) -> Option<Self> {
        NonNull::new(object.cast_mut()).map(Self)
    }

    pub(super) const fn as_ptr(&self) -> CFTypeRef {
        self.0.as_ptr().cast_const()
    }

    pub(super) fn string(text: &str) -> Self {
        unsafe {
            Self::from_create(CFStringCreateWithBytes(
                null(),
                text.as_ptr(),
                text.len() as isize,
                K_CF_STRING_ENCODING_UTF8,
                false,
            ))
        }
        .expect("Could not create string")
    }
}

impl Drop for CFObject {
    fn drop(&mut self) {
        unsafe { CFRelease(self.as_ptr()) };
    }
}
