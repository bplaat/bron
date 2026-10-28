/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Native AppKit, Foundation, Core Foundation, and Core Graphics declarations.

#![allow(non_snake_case, non_upper_case_globals)]

use std::ffi::{c_char, c_void};

use objc2::rc::{Allocated, Retained};
use objc2::runtime::AnyObject as Object;
use objc2::{Encode, Encoding, class, msg_send};

pub(crate) const DRAW_PATH_EVEN_ODD_FILL: i32 = 1;
pub(crate) const DRAW_PATH_FILL: i32 = 0;
pub(crate) const DRAW_PATH_STROKE: i32 = 2;
pub(crate) const GRADIENT_DRAWS_AFTER_END: u32 = 2;
pub(crate) const GRADIENT_DRAWS_BEFORE_START: u32 = 1;
pub(crate) const LINE_CAP_ROUND: i32 = 1;
pub(crate) const LINE_CAP_BUTT: i32 = 0;
pub(crate) const LINE_CAP_SQUARE: i32 = 2;
pub(crate) const LINE_JOIN_MITER: i32 = 0;
pub(crate) const LINE_JOIN_ROUND: i32 = 1;
pub(crate) const LINE_JOIN_BEVEL: i32 = 2;
pub(crate) const NS_IMAGE_SCALE_PROPORTIONALLY_UP_OR_DOWN: u64 = 3;
pub(crate) const NS_IMAGE_CACHE_NEVER: u64 = 3;
pub(crate) const NS_UTF8_STRING_ENCODING: u64 = 4;
pub(crate) const QOS_CLASS_USER_INITIATED: isize = 0x19;

/// The autoresizing mask bit that keeps a view as wide as its superview.
pub const NS_VIEW_WIDTH_SIZABLE: u64 = 2;
/// The autoresizing mask bit that keeps a view as tall as its superview.
pub const NS_VIEW_HEIGHT_SIZABLE: u64 = 16;

#[link(name = "Cocoa", kind = "framework")]
unsafe extern "C" {}

#[link(name = "Foundation", kind = "framework")]
unsafe extern "C" {
    /// The class of compile-time constant strings, which `ns_string!` fills in at load time.
    pub static __CFConstantStringClassReference: Object;

    pub(crate) fn NSExtensionMain(argc: i32, argv: *const *const c_char) -> i32;
}

#[link(name = "System")]
unsafe extern "C" {
    pub(crate) fn dispatch_get_global_queue(identifier: isize, flags: usize) -> *mut c_void;
    pub(crate) fn dispatch_async_f(
        queue: *mut c_void,
        context: *mut c_void,
        work: extern "C" fn(*mut c_void),
    );
    #[link_name = "_dispatch_main_q"]
    pub(crate) static mut DISPATCH_MAIN_QUEUE: u8;
}

/// Creates an autoreleased `NSString` from a runtime string; use `ns_string!` for literals.
pub fn ns_string(value: &str) -> *mut Object {
    // SAFETY: NSString copies the valid UTF-8 bytes before the returned object is autoreleased.
    unsafe {
        let string: Allocated<Object> = msg_send![class!(NSString), alloc];
        let string: Retained<Object> = msg_send![string,
            initWithBytes: value.as_ptr().cast::<c_void>(),
            length: value.len(),
            encoding: NS_UTF8_STRING_ENCODING
        ];
        Retained::autorelease_ptr(string)
    }
}

/// Mirrors Apple's `__CFConstantString` layout for static NSString literals fixed up by dyld.
#[repr(C)]
pub struct CFConstString {
    /// The class pointer, fixed up at load time.
    pub isa: *const c_void,
    /// The Core Foundation flags: 0x07C8 is ASCII, immutable, not inline, not freed, NUL terminated.
    pub cfinfo: u32,
    /// The retain count, which is unused for constant strings.
    #[cfg(target_pointer_width = "64")]
    pub _rc: u32,
    /// The NUL terminated string bytes.
    pub data: *const u8,
    /// The string length in bytes, excluding the NUL terminator.
    pub len: usize,
}
// SAFETY: A constant string is immutable and lives for the whole program.
unsafe impl Send for CFConstString {}
// SAFETY: A constant string is immutable and lives for the whole program.
unsafe impl Sync for CFConstString {}

/// Creates a compile-time checked static ASCII `NSString` literal; call it outside closures.
#[macro_export]
macro_rules! ns_string {
    ($s:expr) => {{
        const INPUT: &str = $s;
        const BYTES: &[u8] = INPUT.as_bytes();
        const _: () = {
            let mut i = 0usize;
            while i < BYTES.len() {
                if !BYTES[i].is_ascii() || BYTES[i] == b'\0' {
                    panic!("ns_string! only supports ASCII strings without NUL bytes");
                }
                i += 1;
            }
        };
        #[unsafe(link_section = "__TEXT,__cstring,cstring_literals")]
        static DATA: [u8; BYTES.len() + 1] = {
            let mut arr = [0u8; BYTES.len() + 1];
            let mut i = 0usize;
            while i < BYTES.len() {
                arr[i] = BYTES[i];
                i += 1;
            }
            arr
        };
        #[unsafe(link_section = "__DATA,__cfstring")]
        // SAFETY: The fields mirror a constant string that dyld finalizes at load time.
        static CFSTRING: $crate::CFConstString = unsafe {
            $crate::CFConstString {
                isa: &$crate::__CFConstantStringClassReference as *const objc2::runtime::AnyObject
                    as *const ::std::ffi::c_void,
                cfinfo: 0x07C8,
                #[cfg(target_pointer_width = "64")]
                _rc: 0,
                data: DATA.as_ptr(),
                len: BYTES.len(),
            }
        };
        &CFSTRING as *const $crate::CFConstString as *mut objc2::runtime::AnyObject
    }};
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub(crate) fn CFDataCreate(
        allocator: *const c_void,
        bytes: *const u8,
        length: isize,
    ) -> *const c_void;
    pub(crate) fn CFRelease(object: *const c_void);
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    pub(crate) static kCGColorSpaceSRGB: *const c_void;
    pub(crate) static kCGColorSpaceLinearSRGB: *const c_void;

    pub(crate) fn CGColorSpaceCreateWithName(name: *const c_void) -> *const c_void;
    pub(crate) fn CGColorSpaceRelease(color_space: *const c_void);
    #[cfg(test)]
    pub(crate) fn CGBitmapContextCreate(
        data: *mut c_void,
        width: usize,
        height: usize,
        bits_per_component: usize,
        bytes_per_row: usize,
        color_space: *const c_void,
        bitmap_info: u32,
    ) -> *mut c_void;
    pub(crate) fn CGContextAddPath(context: *mut c_void, path: *const c_void);
    pub(crate) fn CGContextBeginPath(context: *mut c_void);
    pub(crate) fn CGContextClip(context: *mut c_void);
    pub(crate) fn CGContextBeginTransparencyLayer(
        context: *mut c_void,
        auxiliary_info: *const c_void,
    );
    pub(crate) fn CGContextEndTransparencyLayer(context: *mut c_void);
    pub(crate) fn CGContextConcatCTM(context: *mut c_void, transform: vector::Transform);
    pub(crate) fn CGContextDrawLinearGradient(
        context: *mut c_void,
        gradient: *const c_void,
        start: vector::Point,
        end: vector::Point,
        options: u32,
    );
    pub(crate) fn CGContextDrawPath(context: *mut c_void, mode: i32);
    pub(crate) fn CGContextDrawRadialGradient(
        context: *mut c_void,
        gradient: *const c_void,
        start_center: vector::Point,
        start_radius: f64,
        end_center: vector::Point,
        end_radius: f64,
        options: u32,
    );
    pub(crate) fn CGContextEOClip(context: *mut c_void);
    pub(crate) fn CGContextGetClipBoundingBox(context: *mut c_void) -> Rect;
    /// Fills a rectangle with the current fill color.
    pub fn CGContextFillRect(context: *mut c_void, rectangle: Rect);
    pub(crate) fn CGContextReplacePathWithStrokedPath(context: *mut c_void);
    pub(crate) fn CGContextRestoreGState(context: *mut c_void);
    #[cfg(test)]
    pub(crate) fn CGContextRelease(context: *mut c_void);
    pub(crate) fn CGContextSaveGState(context: *mut c_void);
    pub(crate) fn CGContextScaleCTM(context: *mut c_void, x: f64, y: f64);
    pub(crate) fn CGContextSetLineCap(context: *mut c_void, line_cap: i32);
    pub(crate) fn CGContextSetLineJoin(context: *mut c_void, line_join: i32);
    pub(crate) fn CGContextSetLineDash(
        context: *mut c_void,
        phase: f64,
        lengths: *const f64,
        count: usize,
    );
    pub(crate) fn CGContextSetMiterLimit(context: *mut c_void, limit: f64);
    pub(crate) fn CGContextSetAlpha(context: *mut c_void, alpha: f64);
    pub(crate) fn CGContextSetBlendMode(context: *mut c_void, mode: i32);
    pub(crate) fn CGContextSetLineWidth(context: *mut c_void, width: f64);
    /// Sets the fill color of a context in the device RGB color space.
    pub fn CGContextSetRGBFillColor(
        context: *mut c_void,
        red: f64,
        green: f64,
        blue: f64,
        alpha: f64,
    );
    pub(crate) fn CGContextSetRGBStrokeColor(
        context: *mut c_void,
        red: f64,
        green: f64,
        blue: f64,
        alpha: f64,
    );
    pub(crate) fn CGContextTranslateCTM(context: *mut c_void, x: f64, y: f64);
    pub(crate) fn CGDataProviderCreateWithCFData(data: *const c_void) -> *const c_void;
    pub(crate) fn CGDataProviderRelease(provider: *const c_void);
    pub(crate) fn CGImageCreate(
        width: usize,
        height: usize,
        bits_per_component: usize,
        bits_per_pixel: usize,
        bytes_per_row: usize,
        color_space: *const c_void,
        bitmap_info: u32,
        provider: *const c_void,
        decode: *const f64,
        should_interpolate: bool,
        rendering_intent: i32,
    ) -> *const c_void;
    pub(crate) fn CGImageRelease(image: *const c_void);
    pub(crate) fn CGPathAddCurveToPoint(
        path: *mut c_void,
        transform: *const c_void,
        control_0_x: f64,
        control_0_y: f64,
        control_1_x: f64,
        control_1_y: f64,
        x: f64,
        y: f64,
    );
    pub(crate) fn CGPathAddLineToPoint(path: *mut c_void, transform: *const c_void, x: f64, y: f64);
    pub(crate) fn CGPathAddQuadCurveToPoint(
        path: *mut c_void,
        transform: *const c_void,
        control_x: f64,
        control_y: f64,
        x: f64,
        y: f64,
    );
    pub(crate) fn CGPathCloseSubpath(path: *mut c_void);
    pub(crate) fn CGPathCreateCopy(path: *const c_void) -> *const c_void;
    pub(crate) fn CGPathCreateMutable() -> *mut c_void;
    pub(crate) fn CGPathMoveToPoint(path: *mut c_void, transform: *const c_void, x: f64, y: f64);
    pub(crate) fn CGPathRelease(path: *const c_void);
    pub(crate) fn CGGradientCreateWithColorComponents(
        color_space: *const c_void,
        components: *const f64,
        locations: *const f64,
        count: usize,
    ) -> *const c_void;
    pub(crate) fn CGGradientRelease(gradient: *const c_void);
}

/// A Core Graphics point.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Point {
    /// The horizontal coordinate.
    pub x: f64,
    /// The vertical coordinate.
    pub y: f64,
}

pub(crate) type CgPoint = Point;

// SAFETY: Point matches the Core Graphics CGPoint ABI.
unsafe impl Encode for Point {
    const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
}

/// A Core Graphics size.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Size {
    /// The width.
    pub width: f64,
    /// The height.
    pub height: f64,
}

// SAFETY: Size matches the Core Graphics CGSize ABI.
unsafe impl Encode for Size {
    const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
}

/// A Core Graphics rectangle.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Rect {
    /// The origin.
    pub origin: Point,
    /// The size.
    pub size: Size,
}

// SAFETY: Rect matches the Core Graphics CGRect ABI.
unsafe impl Encode for Rect {
    const ENCODING: Encoding = Encoding::Struct("CGRect", &[Point::ENCODING, Size::ENCODING]);
}
