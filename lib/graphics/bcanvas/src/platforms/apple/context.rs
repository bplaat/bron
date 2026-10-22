/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr::{null, null_mut};
use std::rc::Rc;

use super::headers::*;
use crate::{CanvasState, Color, FontWeight, LineCap, LineJoin, TextAlign, TextBaseline};

const MAX_TEXT_STYLES: usize = 64;

pub(crate) struct PlatformCanvasContext {
    context: CGContextRef,
    initial_transform: CGAffineTransform,
    fill_color: Option<(Color, u32)>,
    stroke_style: Option<StrokeStyle>,
    text_styles: TextStyleCache,
}

type StrokeStyle = (Color, u32, u32, LineCap, LineJoin);

/// Text attributes shared by all frames of one canvas.
#[derive(Default, Clone)]
pub(super) struct TextStyleCache(Rc<RefCell<HashMap<TextStyleKey, Rc<TextStyle>>>>);

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct TextStyleKey {
    family: Rc<str>,
    size: u32,
    weight: FontWeight,
}

/// Core Text attributes, the text color comes from the context fill color.
struct TextStyle {
    attributes: CFObject,
    ascent: f32,
    descent: f32,
}

impl TextStyleCache {
    fn get(&self, s: &CanvasState) -> Rc<TextStyle> {
        let key = TextStyleKey {
            family: s.font_family.clone(),
            size: s.font_size.to_bits(),
            weight: s.font_weight,
        };
        let mut styles = self.0.borrow_mut();
        if !styles.contains_key(&key) && styles.len() >= MAX_TEXT_STYLES {
            styles.clear();
        }
        styles
            .entry(key)
            .or_insert_with(|| Rc::new(create_text_style(s)))
            .clone()
    }
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
    pub(super) unsafe fn new(context: CGContextRef, text_styles: TextStyleCache) -> Self {
        unsafe {
            CGContextSaveGState(context);
            CGContextBeginPath(context);
        }
        Self {
            context,
            initial_transform: unsafe { CGContextGetCTM(context) },
            fill_color: None,
            stroke_style: None,
            text_styles,
        }
    }

    pub(crate) fn save(&mut self) {
        unsafe { CGContextSaveGState(self.context) }
    }
    pub(crate) fn restore(&mut self) {
        self.invalidate_styles();
        unsafe { CGContextRestoreGState(self.context) }
    }
    pub(crate) fn clear_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.preserving_path(|context| unsafe { CGContextClearRect(context, rect(x, y, w, h)) });
    }
    pub(crate) fn fill_rect(&mut self, s: &CanvasState, x: f32, y: f32, w: f32, h: f32) {
        self.fill_style(s);
        self.preserving_path(|context| unsafe { CGContextFillRect(context, rect(x, y, w, h)) });
    }
    pub(crate) fn stroke_rect(&mut self, s: &CanvasState, x: f32, y: f32, w: f32, h: f32) {
        self.stroke_style(s);
        self.preserving_path(|context| unsafe { CGContextStrokeRect(context, rect(x, y, w, h)) });
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
                ccw.into(),
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
        self.preserving_path(|context| unsafe { CGContextFillPath(context) });
    }
    pub(crate) fn stroke(&mut self, s: &CanvasState) {
        self.stroke_style(s);
        self.preserving_path(|context| unsafe { CGContextStrokePath(context) });
    }
    pub(crate) fn clip(&mut self) {
        self.preserving_path(|context| unsafe { CGContextClip(context) });
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
        let transform = CGAffineTransform {
            a: a.into(),
            b: b.into(),
            c: c.into(),
            d: d.into(),
            tx: e.into(),
            ty: f.into(),
        };
        self.set_ctm(unsafe { CGAffineTransformConcat(transform, self.initial_transform) });
    }
    pub(crate) fn reset_transform(&mut self) {
        self.set_ctm(self.initial_transform);
    }
    pub(crate) fn fill_text(&mut self, s: &CanvasState, text: &str, x: f32, y: f32) {
        self.fill_style(s);
        let style = self.text_styles.get(s);
        let line = text_line(text, &style);
        let x = match s.text_align {
            TextAlign::Center => x - line_width(&line) / 2.0,
            TextAlign::Right | TextAlign::End => x - line_width(&line),
            _ => x,
        };
        // Core Text draws from the alphabetic baseline, so offset by the font metrics.
        let y = match s.text_baseline {
            TextBaseline::Middle => y + (style.ascent - style.descent) / 2.0,
            TextBaseline::Bottom | TextBaseline::Ideographic => y - style.descent,
            TextBaseline::Alphabetic => y,
            _ => y + style.ascent,
        };
        unsafe {
            // The context is flipped, so flip the glyphs back upright.
            CGContextSetTextMatrix(
                self.context,
                CGAffineTransform {
                    a: 1.0,
                    b: 0.0,
                    c: 0.0,
                    d: -1.0,
                    tx: 0.0,
                    ty: 0.0,
                },
            );
            CGContextSetTextPosition(self.context, x.into(), y.into());
            CTLineDraw(line.as_ptr(), self.context);
        }
    }
    pub(crate) fn measure_text(&mut self, s: &CanvasState, text: &str) -> f32 {
        line_width(&text_line(text, &self.text_styles.get(s)))
    }
    const fn invalidate_styles(&mut self) {
        self.fill_color = None;
        self.stroke_style = None;
    }
    /// Core Graphics consumes the current path when painting, HTML canvas keeps it.
    fn preserving_path(&mut self, paint: impl FnOnce(CGContextRef)) {
        unsafe {
            if CGContextIsPathEmpty(self.context) {
                paint(self.context);
                return;
            }
            let path = CGContextCopyPath(self.context);
            paint(self.context);
            CGContextBeginPath(self.context);
            CGContextAddPath(self.context, path);
            CGPathRelease(path);
        }
    }
    /// Replace the CTM with public API only, CGContextSetCTM is private.
    fn set_ctm(&mut self, transform: CGAffineTransform) {
        unsafe {
            let current = CGAffineTransformInvert(CGContextGetCTM(self.context));
            CGContextConcatCTM(self.context, CGAffineTransformConcat(transform, current));
        }
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
    fn stroke_style(&mut self, s: &CanvasState) {
        let style = (
            s.stroke,
            s.alpha.to_bits(),
            s.line_width.to_bits(),
            s.line_cap,
            s.line_join,
        );
        if self.stroke_style == Some(style) {
            return;
        }
        self.stroke_style = Some(style);
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
    CGRect {
        origin: CGPoint {
            x: x.into(),
            y: y.into(),
        },
        size: CGSize {
            width: w.into(),
            height: h.into(),
        },
    }
}

fn text_line(text: &str, style: &TextStyle) -> CFObject {
    let string = CFObject::string(text);
    unsafe {
        let attributed = CFObject::from_create(CFAttributedStringCreate(
            null(),
            string.as_ptr(),
            style.attributes.as_ptr(),
        ))
        .expect("Could not create attributed string");
        CFObject::from_create(CTLineCreateWithAttributedString(attributed.as_ptr()))
            .expect("Could not create text line")
    }
}

fn line_width(line: &CFObject) -> f32 {
    unsafe { CTLineGetTypographicBounds(line.as_ptr(), null_mut(), null_mut(), null_mut()) as f32 }
}

fn create_text_style(s: &CanvasState) -> TextStyle {
    let size = f64::from(s.font_size);
    let mut font = named_font(&s.font_family, size).unwrap_or_else(|| unsafe {
        CFObject::from_create(CTFontCreateUIFontForLanguage(
            K_CT_FONT_UI_FONT_SYSTEM,
            size,
            null(),
        ))
        .expect("Could not create system font")
    });
    if s.font_weight == FontWeight::Bold {
        // Fonts without a bold face keep their regular face.
        if let Some(bold) = unsafe {
            CFObject::from_create(CTFontCreateCopyWithSymbolicTraits(
                font.as_ptr(),
                0.0,
                null(),
                K_CT_FONT_BOLD_TRAIT,
                K_CT_FONT_BOLD_TRAIT,
            ))
        } {
            font = bold;
        }
    }
    unsafe {
        let keys = [
            kCTFontAttributeName,
            kCTForegroundColorFromContextAttributeName,
        ];
        let values = [font.as_ptr(), kCFBooleanTrue];
        let attributes = CFObject::from_create(CFDictionaryCreate(
            null(),
            keys.as_ptr(),
            values.as_ptr(),
            keys.len() as isize,
            &raw const kCFTypeDictionaryKeyCallBacks,
            &raw const kCFTypeDictionaryValueCallBacks,
        ))
        .expect("Could not create text attributes");
        TextStyle {
            attributes,
            ascent: CTFontGetAscent(font.as_ptr()) as f32,
            descent: CTFontGetDescent(font.as_ptr()) as f32,
        }
    }
}

/// Core Text substitutes a fallback font for unknown names, so only accept a font that
/// matches the requested family, full or PostScript name.
fn named_font(family: &str, size: f64) -> Option<CFObject> {
    let name = CFObject::string(family);
    unsafe {
        let font = CFObject::from_create(CTFontCreateWithName(name.as_ptr(), size, null()))?;
        [
            CTFontCopyFamilyName,
            CTFontCopyFullName,
            CTFontCopyPostScriptName,
        ]
        .into_iter()
        .filter_map(|copy| CFObject::from_create(copy(font.as_ptr())))
        .any(|candidate| {
            CFStringCompare(
                name.as_ptr(),
                candidate.as_ptr(),
                K_CF_COMPARE_CASE_INSENSITIVE,
            ) == 0
        })
        .then_some(font)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bold_text_preserves_the_requested_family() {
        let state = CanvasState {
            font_family: Rc::from("Menlo"),
            font_size: 18.0,
            ..CanvasState::default()
        };
        let normal = create_text_style(&state);
        let bold = create_text_style(&CanvasState {
            font_weight: FontWeight::Bold,
            ..state
        });
        unsafe {
            let normal_font =
                CFDictionaryGetValue(normal.attributes.as_ptr(), kCTFontAttributeName);
            let bold_font = CFDictionaryGetValue(bold.attributes.as_ptr(), kCTFontAttributeName);
            let normal_family =
                CFObject::from_create(CTFontCopyFamilyName(normal_font)).expect("family name");
            let bold_family =
                CFObject::from_create(CTFontCopyFamilyName(bold_font)).expect("family name");
            assert_eq!(
                CFStringCompare(normal_family.as_ptr(), bold_family.as_ptr(), 0),
                0
            );
            assert_ne!(CTFontGetSymbolicTraits(bold_font) & K_CT_FONT_BOLD_TRAIT, 0);
            assert_eq!(CTFontGetSize(bold_font), 18.0);
        }
    }

    #[test]
    fn unknown_font_family_falls_back_to_the_system_font() {
        assert!(named_font("sans-serif", 12.0).is_none());
        assert!(named_font("menlo", 12.0).is_some());
    }

    #[test]
    fn text_is_drawn_upright_above_the_alphabetic_baseline() {
        let mut canvas = crate::OffscreenCanvas::new(40, 40);
        canvas.draw(|context| {
            context.set_font("Menlo", 20.0);
            context.set_fill_style(Color::rgb(255, 0, 0));
            context.fill_text("T", 10.0, 30.0);
        });
        let pixels = canvas.pixels();
        let ink = |y: usize| (0..40).any(|x| pixels[(y * 40 + x) * 4 + 3] > 128);
        // The crossbar of an upright T sits near the top of its glyph box.
        let top = (0..40).find(|&y| ink(y)).expect("text was drawn");
        assert!((10..20).contains(&top));
        assert!(!(31..40).any(ink));
        let red = (0..40 * 40)
            .find(|i| pixels[i * 4 + 3] == 255)
            .expect("opaque ink");
        assert_eq!(&pixels[red * 4..][..4], [255, 0, 0, 255]);
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
            {
                let mut canvas = PlatformCanvasContext::new(context, TextStyleCache::default());
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

    #[test]
    fn rect_painting_preserves_the_current_path() {
        let mut canvas = crate::OffscreenCanvas::new(80, 80);
        canvas.draw(|context| {
            context.begin_path();
            context.rect(10.0, 10.0, 20.0, 20.0);
            context.fill_rect(60.0, 60.0, 10.0, 10.0);
            context.stroke_rect(40.0, 40.0, 10.0, 10.0);
            context.clear_rect(0.0, 0.0, 5.0, 5.0);
            context.fill();
        });
        let pixels = canvas.pixels();
        let pixel = |x: usize, y: usize| &pixels[(y * 80 + x) * 4..][..4];
        assert_eq!(pixel(20, 20), [0, 0, 0, 255]);
        assert_eq!(pixel(65, 65), [0, 0, 0, 255]);
        assert_eq!(pixel(35, 35), [0, 0, 0, 0]);
    }

    #[test]
    fn set_transform_is_relative_to_the_initial_transform() {
        let mut canvas = crate::OffscreenCanvas::new(4, 2);
        canvas.draw(|context| {
            context.translate(3.0, 0.0);
            context.scale(2.0, 2.0);
            context.set_transform(1.0, 0.0, 0.0, 1.0, 1.0, 1.0);
            context.fill_rect(0.0, 0.0, 1.0, 1.0);
            context.reset_transform();
            context.fill_rect(0.0, 0.0, 1.0, 1.0);
        });
        let alpha: Vec<u8> = canvas.pixels().iter().skip(3).step_by(4).copied().collect();
        assert_eq!(alpha, [255, 0, 0, 0, 0, 255, 0, 0]);
    }
}
