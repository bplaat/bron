/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::collections::HashMap;
use std::ptr::null;
use std::rc::Rc;

use bwindow::ffi::*;
use objc2::rc::Retained;
use objc2::runtime::AnyObject as Object;
use objc2::{class, msg_send};

use super::headers::*;
use crate::{CanvasState, Color, FontWeight, LineCap, LineJoin, TextAlign, TextBaseline};

pub(crate) struct PlatformCanvasContext {
    context: CGContextRef,
    initial_transform: CGAffineTransform,
    fill_color: Option<(Color, u32)>,
    text_attributes: HashMap<TextStyleKey, Retained<Object>>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct TextStyleKey {
    family: Rc<str>,
    size: u32,
    weight: FontWeight,
    color: Color,
    alpha: u32,
}

impl Drop for PlatformCanvasContext {
    fn drop(&mut self) {
        unsafe {
            CGContextBeginPath(self.context);
            CGContextRestoreGState(self.context);
        }
    }
}

impl PlatformCanvasContext {
    pub(super) unsafe fn new(context: CGContextRef) -> Self {
        unsafe {
            CGContextSaveGState(context);
            CGContextBeginPath(context);
        }
        Self {
            context,
            initial_transform: unsafe { CGContextGetCTM(context) },
            fill_color: None,
            text_attributes: HashMap::new(),
        }
    }

    pub(crate) fn save(&mut self) {
        unsafe { CGContextSaveGState(self.context) }
    }
    pub(crate) fn restore(&mut self) {
        self.fill_color = None;
        unsafe { CGContextRestoreGState(self.context) }
    }
    pub(crate) fn clear_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        unsafe { CGContextClearRect(self.context, rect(x, y, w, h)) }
    }
    pub(crate) fn fill_rect(&mut self, s: &CanvasState, x: f32, y: f32, w: f32, h: f32) {
        self.fill_style(s);
        unsafe { CGContextFillRect(self.context, rect(x, y, w, h)) }
    }
    pub(crate) fn stroke_rect(&mut self, s: &CanvasState, x: f32, y: f32, w: f32, h: f32) {
        self.stroke_style(s);
        unsafe { CGContextStrokeRect(self.context, rect(x, y, w, h)) }
    }
    pub(crate) fn begin_path(&mut self) {
        unsafe { CGContextBeginPath(self.context) }
    }
    pub(crate) fn close_path(&mut self) {
        unsafe { CGContextClosePath(self.context) }
    }
    pub(crate) fn move_to(&mut self, x: f32, y: f32) {
        unsafe { CGContextMoveToPoint(self.context, x.into(), y.into()) }
    }
    pub(crate) fn line_to(&mut self, x: f32, y: f32) {
        unsafe { CGContextAddLineToPoint(self.context, x.into(), y.into()) }
    }
    pub(crate) fn rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        unsafe { CGContextAddRect(self.context, rect(x, y, w, h)) }
    }
    pub(crate) fn round_rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32) {
        let radius = r.max(0.0).min(w.abs() * 0.5).min(h.abs() * 0.5);
        unsafe {
            let path =
                CGPathCreateWithRoundedRect(rect(x, y, w, h), radius.into(), radius.into(), null());
            CGContextAddPath(self.context, path);
            CGPathRelease(path);
        }
    }
    pub(crate) fn ellipse(&mut self, x: f32, y: f32, rx: f32, ry: f32) {
        unsafe { CGContextAddEllipseInRect(self.context, rect(x - rx, y - ry, rx * 2.0, ry * 2.0)) }
    }
    pub(crate) fn arc(&mut self, x: f32, y: f32, r: f32, start: f32, end: f32, ccw: bool) {
        unsafe {
            CGContextAddArc(
                self.context,
                x.into(),
                y.into(),
                r.into(),
                start.into(),
                end.into(),
                ccw,
            )
        }
    }
    pub(crate) fn quadratic_curve_to(&mut self, cpx: f32, cpy: f32, x: f32, y: f32) {
        unsafe {
            CGContextAddQuadCurveToPoint(self.context, cpx.into(), cpy.into(), x.into(), y.into())
        }
    }
    pub(crate) fn bezier_curve_to(&mut self, a: f32, b: f32, c: f32, d: f32, x: f32, y: f32) {
        unsafe {
            CGContextAddCurveToPoint(
                self.context,
                a.into(),
                b.into(),
                c.into(),
                d.into(),
                x.into(),
                y.into(),
            )
        }
    }
    pub(crate) fn fill(&mut self, s: &CanvasState) {
        self.fill_style(s);
        unsafe {
            let path = CGContextCopyPath(self.context);
            CGContextFillPath(self.context);
            if !path.is_null() {
                CGContextAddPath(self.context, path);
                CGPathRelease(path);
            }
        }
    }
    pub(crate) fn stroke(&mut self, s: &CanvasState) {
        self.stroke_style(s);
        unsafe {
            let path = CGContextCopyPath(self.context);
            CGContextStrokePath(self.context);
            if !path.is_null() {
                CGContextAddPath(self.context, path);
                CGPathRelease(path);
            }
        }
    }
    pub(crate) fn clip(&mut self) {
        unsafe {
            let path = CGContextCopyPath(self.context);
            CGContextClip(self.context);
            if !path.is_null() {
                CGContextAddPath(self.context, path);
                CGPathRelease(path);
            }
        }
    }
    pub(crate) fn translate(&mut self, x: f32, y: f32) {
        unsafe { CGContextTranslateCTM(self.context, x.into(), y.into()) }
    }
    pub(crate) fn rotate(&mut self, a: f32) {
        unsafe { CGContextRotateCTM(self.context, a.into()) }
    }
    pub(crate) fn scale(&mut self, x: f32, y: f32) {
        unsafe { CGContextScaleCTM(self.context, x.into(), y.into()) }
    }
    pub(crate) fn set_transform(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) {
        unsafe {
            CGContextSetCTM(self.context, self.initial_transform);
            CGContextConcatCTM(
                self.context,
                CGAffineTransform {
                    a: a.into(),
                    b: b.into(),
                    c: c.into(),
                    d: d.into(),
                    tx: e.into(),
                    ty: f.into(),
                },
            )
        }
    }
    pub(crate) fn reset_transform(&mut self) {
        unsafe { CGContextSetCTM(self.context, self.initial_transform) }
    }
    pub(crate) fn fill_text(&mut self, s: &CanvasState, text: &str, x: f32, y: f32) {
        // AppKit text drawing can change native graphics state.
        self.fill_color = None;
        unsafe {
            let attrs = self.text_attributes(s);
            let string = NSString::new(text);
            let measured: CGSize = msg_send![&*string, sizeWithAttributes:attrs];
            let (x, y) =
                aligned_text_position(s, x, y, measured.width as f32, measured.height as f32);
            let _: () = msg_send![&*string, drawAtPoint:CGPoint::new(x.into(),y.into()), withAttributes:attrs];
        }
    }
    pub(crate) fn measure_text(&mut self, s: &CanvasState, text: &str) -> f32 {
        unsafe {
            let attrs = self.text_attributes(s);
            let string = NSString::new(text);
            let size: CGSize = msg_send![&*string,sizeWithAttributes:attrs];
            size.width as f32
        }
    }
    fn text_attributes(&mut self, s: &CanvasState) -> *mut Object {
        let key = TextStyleKey {
            family: s.font_family.clone(),
            size: s.font_size.to_bits(),
            weight: s.font_weight,
            color: s.fill,
            alpha: s.alpha.to_bits(),
        };
        self.text_attributes
            .entry(key)
            .or_insert_with(|| unsafe { create_text_attributes(s) })
            .as_ptr()
    }
    fn fill_style(&mut self, s: &CanvasState) {
        let fill = (s.fill, s.alpha.to_bits());
        if self.fill_color == Some(fill) {
            return;
        }
        self.fill_color = Some(fill);
        let (r, g, b, a) = s.fill.components(s.alpha);
        unsafe { CGContextSetRGBFillColor(self.context, r, g, b, a) }
    }
    fn stroke_style(&self, s: &CanvasState) {
        let (r, g, b, a) = s.stroke.components(s.alpha);
        unsafe {
            CGContextSetRGBStrokeColor(self.context, r, g, b, a);
            CGContextSetLineWidth(self.context, s.line_width.into());
            CGContextSetLineCap(
                self.context,
                match s.line_cap {
                    LineCap::Butt => 0,
                    LineCap::Round => 1,
                    LineCap::Square => 2,
                },
            );
            CGContextSetLineJoin(
                self.context,
                match s.line_join {
                    LineJoin::Miter => 0,
                    LineJoin::Round => 1,
                    LineJoin::Bevel => 2,
                },
            );
        }
    }
}

fn rect(x: f32, y: f32, w: f32, h: f32) -> CGRect {
    CGRect::new(
        CGPoint::new(x.into(), y.into()),
        CGSize::new(w.into(), h.into()),
    )
}

unsafe fn create_text_attributes(s: &CanvasState) -> Retained<Object> {
    #[cfg(target_os = "macos")]
    let (font_class, color_class) = (class!(NSFont), class!(NSColor));
    #[cfg(target_os = "ios")]
    let (font_class, color_class) = (class!(UIFont), class!(UIColor));
    let family = NSString::new(&s.font_family);
    let mut font: *mut Object =
        unsafe { msg_send![font_class,fontWithName:family,size:s.font_size as f64] };
    if font.is_null() {
        font = unsafe { msg_send![font_class,systemFontOfSize:s.font_size as f64] };
    }
    if s.font_weight == FontWeight::Bold {
        font = unsafe { bold_font(font) };
    }
    let (r, g, b, a) = s.fill.components(s.alpha);
    let color: *mut Object =
        unsafe { msg_send![color_class,colorWithRed:r,green:g,blue:b,alpha:a] };
    let attrs: Retained<Object> = unsafe { msg_send![class!(NSMutableDictionary), new] };
    let _: () = unsafe { msg_send![&*attrs, setObject:font, forKey:ns_string!("NSFont")] };
    let _: () = unsafe { msg_send![&*attrs, setObject:color, forKey:ns_string!("NSColor")] };
    attrs
}

#[cfg(target_os = "macos")]
unsafe fn bold_font(font: *mut Object) -> *mut Object {
    let manager: *mut Object = unsafe { msg_send![class!(NSFontManager), sharedFontManager] };
    unsafe { msg_send![manager, convertFont:font, toHaveTrait:NS_BOLD_FONT_MASK] }
}

#[cfg(target_os = "ios")]
unsafe fn bold_font(font: *mut Object) -> *mut Object {
    unsafe {
        let descriptor: *mut Object = msg_send![font, fontDescriptor];
        let descriptor: *mut Object =
            msg_send![descriptor, fontDescriptorWithSymbolicTraits:UI_FONT_DESCRIPTOR_TRAIT_BOLD];
        if descriptor.is_null() {
            return font;
        }
        msg_send![class!(UIFont), fontWithDescriptor:descriptor, size:0.0f64]
    }
}

fn aligned_text_position(s: &CanvasState, mut x: f32, mut y: f32, w: f32, h: f32) -> (f32, f32) {
    match s.text_align {
        TextAlign::Center => x -= w / 2.0,
        TextAlign::Right | TextAlign::End => x -= w,
        _ => {}
    }
    match s.text_baseline {
        TextBaseline::Middle => y -= h / 2.0,
        TextBaseline::Bottom | TextBaseline::Ideographic => y -= h,
        TextBaseline::Alphabetic => y -= h * 0.8,
        _ => {}
    }
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn bold_text_preserves_the_requested_family() {
        objc2::rc::autoreleasepool(|_| unsafe {
            let state = CanvasState {
                font_family: Rc::from("Menlo"),
                font_size: 18.0,
                ..CanvasState::default()
            };
            let normal = create_text_attributes(&state);
            let bold = create_text_attributes(&CanvasState {
                font_weight: FontWeight::Bold,
                ..state
            });
            let key = NSString::new("NSFont");
            let normal_font: *mut Object = msg_send![&*normal, objectForKey:&*key];
            let bold_font: *mut Object = msg_send![&*bold, objectForKey:&*key];
            let normal_family: *mut Object = msg_send![normal_font, familyName];
            let bold_family: *mut Object = msg_send![bold_font, familyName];
            let same_family: objc2::runtime::Bool =
                msg_send![normal_family, isEqualToString:bold_family];
            assert!(same_family.as_bool());
            let manager: *mut Object = msg_send![class!(NSFontManager), sharedFontManager];
            let traits: usize = msg_send![manager, traitsOfFont:bold_font];
            assert_ne!(traits & NS_BOLD_FONT_MASK, 0);
            let size: f64 = msg_send![bold_font, pointSize];
            assert_eq!(size, 18.0);
        });
    }

    #[test]
    fn cached_fill_preserves_restored_colors_and_alpha() {
        let mut pixels = [0_u8; 16];
        unsafe {
            let space = CGColorSpaceCreateDeviceRGB();
            // Big-endian RGBA, premultiplied alpha in the last component.
            let context = CGBitmapContextCreate(
                pixels.as_mut_ptr().cast(),
                4,
                1,
                8,
                16,
                space,
                (4 << 12) | 1,
            );
            CGColorSpaceRelease(space);
            assert!(!context.is_null());
            CGContextSaveGState(context);
            {
                let mut canvas = PlatformCanvasContext {
                    context,
                    initial_transform: CGContextGetCTM(context),
                    fill_color: None,
                    text_attributes: HashMap::new(),
                };
                let mut state = CanvasState {
                    fill: Color::rgb(255, 0, 0),
                    ..CanvasState::default()
                };
                canvas.fill_rect(&state, 0.0, 0.0, 1.0, 1.0);
                canvas.save();
                state.fill = Color::rgb(0, 0, 255);
                canvas.fill_rect(&state, 1.0, 0.0, 1.0, 1.0);
                canvas.restore();
                canvas.fill_rect(&state, 2.0, 0.0, 1.0, 1.0);
                state.alpha = 0.5;
                canvas.fill_rect(&state, 3.0, 0.0, 1.0, 1.0);
            }
            CGContextRelease(context);
        }
        assert_eq!(
            &pixels[..12],
            &[255, 0, 0, 255, 0, 0, 255, 255, 0, 0, 255, 255]
        );
        assert_eq!(&pixels[12..], &[0, 0, 128, 128]);
    }
}
