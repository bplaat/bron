/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use super::values::{
    length, length_list, normalized_diagonal, number, parse_css_transform, positive_number,
    unit_number,
};
use super::xml::Element;
use crate::{
    BlendMode, Color, ColorSpace, FillRule, LineCap, LineJoin, Size, StrokeStyle, VectorDecodeError,
};

#[derive(Clone)]
pub(super) struct Style {
    pub(super) color: Color,
    pub(super) color_interpolation: ColorSpace,
    pub(super) fill: PaintValue,
    pub(super) stroke: PaintValue,
    pub(super) context_fill: PaintValue,
    pub(super) context_stroke: PaintValue,
    pub(super) context_paint_available: bool,
    pub(super) fill_opacity: f64,
    pub(super) stroke_opacity: f64,
    pub(super) opacity: f64,
    pub(super) blend_mode: BlendMode,
    pub(super) isolated: bool,
    pub(super) z_index: i32,
    pub(super) fill_rule: FillRule,
    pub(super) visible: bool,
    pub(super) displayed: bool,
    pub(super) overflow_visible: bool,
    pub(super) path_length: Option<f64>,
    pub(super) transform: Option<crate::Transform>,
    pub(super) transform_origin: Option<(TransformOrigin, TransformOrigin)>,
    pub(super) transform_fill_box: bool,
    pub(super) paint_order: [PaintLayer; 3],
    pub(super) marker_start: Option<String>,
    pub(super) marker_mid: Option<String>,
    pub(super) marker_end: Option<String>,
    pub(super) stroke_style: StrokeStyle,
    pub(super) clip_path: Option<String>,
    pub(super) mask: Option<String>,
    pub(super) stop_color: Color,
    pub(super) stop_opacity: f64,
    pub(super) geometry: GeometryStyle,
}

#[derive(Clone, Copy, Default)]
pub(super) struct GeometryStyle {
    pub(super) x: Option<GeometryValue>,
    pub(super) y: Option<GeometryValue>,
    pub(super) x1: Option<GeometryValue>,
    pub(super) y1: Option<GeometryValue>,
    pub(super) x2: Option<GeometryValue>,
    pub(super) y2: Option<GeometryValue>,
    pub(super) cx: Option<GeometryValue>,
    pub(super) cy: Option<GeometryValue>,
    pub(super) r: Option<GeometryValue>,
    pub(super) rx: Option<GeometryValue>,
    pub(super) ry: Option<GeometryValue>,
    pub(super) width: Option<GeometryValue>,
    pub(super) height: Option<GeometryValue>,
}

#[derive(Clone, Copy)]
pub(super) enum GeometryValue {
    Auto,
    Length(f64),
}

#[derive(Clone, Copy)]
pub(super) enum TransformOrigin {
    Length(f64),
    Percentage(f64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PaintLayer {
    Fill,
    Stroke,
    Markers,
}

impl TransformOrigin {
    pub(super) fn resolve(self, start: f64, length: f64) -> f64 {
        match self {
            Self::Length(value) => start + value,
            Self::Percentage(value) => start + value * length,
        }
    }
}

impl GeometryValue {
    pub(super) const fn length_or(self, default: f64) -> f64 {
        match self {
            Self::Auto => default,
            Self::Length(value) => value,
        }
    }
}

#[derive(Clone)]
pub(super) enum PaintValue {
    None,
    Color(Color),
    ContextFill,
    ContextStroke,
    Reference {
        reference: String,
        fallback: Option<Box<PaintValue>>,
    },
}

impl Default for Style {
    fn default() -> Self {
        let black = Color {
            red: 0.0,
            green: 0.0,
            blue: 0.0,
            alpha: 1.0,
            color_space: ColorSpace::Srgb,
        };
        Self {
            color: black,
            color_interpolation: ColorSpace::Srgb,
            fill: PaintValue::Color(black),
            stroke: PaintValue::None,
            context_fill: PaintValue::None,
            context_stroke: PaintValue::None,
            context_paint_available: false,
            fill_opacity: 1.0,
            stroke_opacity: 1.0,
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            isolated: false,
            z_index: 0,
            fill_rule: FillRule::NonZero,
            visible: true,
            displayed: true,
            overflow_visible: false,
            path_length: None,
            transform: None,
            transform_origin: None,
            transform_fill_box: false,
            paint_order: [PaintLayer::Fill, PaintLayer::Stroke, PaintLayer::Markers],
            marker_start: None,
            marker_mid: None,
            marker_end: None,
            stroke_style: StrokeStyle {
                width: 1.0,
                line_cap: LineCap::Butt,
                line_join: LineJoin::Miter,
                miter_limit: 4.0,
                dash_array: Vec::new(),
                dash_offset: 0.0,
            },
            clip_path: None,
            mask: None,
            stop_color: black,
            stop_opacity: 1.0,
            geometry: GeometryStyle::default(),
        }
    }
}

struct CssRule {
    selectors: Vec<Selector>,
    declarations: Vec<(String, String, bool)>,
    order: usize,
}

struct Selector {
    parent: Option<Box<Selector>>,
    element: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
    specificity: u32,
}

impl Selector {
    fn parse(source: &str) -> Option<Self> {
        let source = source.trim();
        let (parent, source) = if let Some((parent, child)) = source.rsplit_once('>') {
            (Some(Box::new(Self::parse(parent)?)), child.trim())
        } else {
            (None, source)
        };
        if source.is_empty() || source.bytes().any(|byte| byte.is_ascii_whitespace()) {
            return None;
        }
        let mut element = None;
        let mut id = None;
        let mut classes = Vec::new();
        let mut rest = source;
        if rest.starts_with('*') {
            rest = &rest[1..];
        } else if !rest.starts_with('.') && !rest.starts_with('#') {
            let end = rest.find(['.', '#']).unwrap_or(rest.len());
            element = Some(rest[..end].to_ascii_lowercase());
            rest = &rest[end..];
        }
        while !rest.is_empty() {
            let marker = rest.as_bytes()[0];
            if !matches!(marker, b'.' | b'#') {
                return None;
            }
            rest = &rest[1..];
            let end = rest.find(['.', '#']).unwrap_or(rest.len());
            let value = &rest[..end];
            if value.is_empty()
                || !value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            {
                return None;
            }
            if marker == b'#' {
                if id.replace(value.to_owned()).is_some() {
                    return None;
                }
            } else {
                classes.push(value.to_owned());
            }
            rest = &rest[end..];
        }
        let specificity = parent.as_ref().map_or(0, |parent| parent.specificity)
            + u32::from(id.is_some()) * 65_536
            + classes.len() as u32 * 256
            + u32::from(element.is_some());
        Some(Self {
            parent,
            element,
            id,
            classes,
            specificity,
        })
    }

    fn matches(&self, element: &Element<'_>, elements: &[Element<'_>]) -> bool {
        self.parent.as_ref().is_none_or(|parent| {
            element
                .parent
                .is_some_and(|index| parent.matches(&elements[index], elements))
        }) && self
            .element
            .as_deref()
            .is_none_or(|name| name == element.name)
            && self
                .id
                .as_deref()
                .is_none_or(|id| element.attr("id") == Some(id))
            && self.classes.iter().all(|class| {
                element
                    .attr("class")
                    .is_some_and(|value| value.split_ascii_whitespace().any(|value| value == class))
            })
    }
}

fn css_declarations(source: &str) -> Result<Vec<(String, String, bool)>, VectorDecodeError> {
    let declarations = source
        .split(';')
        .filter_map(|declaration| {
            let (name, value) = declaration.split_once(':')?;
            let name = name.trim().to_ascii_lowercase();
            let mut value = value.trim();
            let important = value
                .strip_suffix("!important")
                .map(|plain| {
                    value = plain.trim_end();
                    true
                })
                .unwrap_or(false);
            (!name.is_empty() && !value.is_empty()).then(|| (name, value.to_owned(), important))
        })
        .collect::<Vec<_>>();
    if declarations.len() > super::MAX_CSS_DECLARATIONS {
        return Err(VectorDecodeError::ResourceLimit);
    }
    Ok(declarations)
}

pub(super) struct Stylesheet {
    rules: Vec<CssRule>,
    uses_z_index: bool,
}

impl Stylesheet {
    pub(super) fn parse(elements: &[Element<'_>]) -> Result<Self, VectorDecodeError> {
        let mut rules = Vec::new();
        let mut uses_z_index = elements.iter().any(|element| {
            element.attr("z-index").is_some()
                || element.attr("style").is_some_and(|style| {
                    style
                        .as_bytes()
                        .windows(b"z-index".len())
                        .any(|name| name.eq_ignore_ascii_case(b"z-index"))
                        && element
                            .inline_style
                            .get_or_init(|| css_declarations(style))
                            .as_ref()
                            .is_ok_and(|declarations| {
                                declarations.iter().any(|(name, _, _)| name == "z-index")
                            })
                })
        });
        let mut text_bytes = 0usize;
        let mut selector_count = 0usize;
        let mut declaration_count = 0usize;
        for element in elements
            .iter()
            .filter(|element| element.is_svg && element.name == "style")
        {
            text_bytes = text_bytes
                .checked_add(element.text.len())
                .ok_or(VectorDecodeError::ResourceLimit)?;
            if text_bytes > super::MAX_STYLESHEET_BYTES {
                return Err(VectorDecodeError::ResourceLimit);
            }
            let mut source = element.text.replace("<![CDATA[", "").replace("]]>", "");
            while let Some(start) = source.find("/*") {
                let Some(end) = source[start + 2..].find("*/") else {
                    break;
                };
                source.replace_range(start..start + 2 + end + 2, "");
            }
            for block in source.split('}') {
                let Some((selectors, declarations)) = block.split_once('{') else {
                    continue;
                };
                if selectors.trim_start().starts_with('@') {
                    continue;
                }
                let selectors = selectors
                    .split(',')
                    .filter_map(Selector::parse)
                    .collect::<Vec<_>>();
                let declarations = css_declarations(declarations)?;
                uses_z_index |= declarations.iter().any(|(name, _, _)| name == "z-index");
                selector_count = selector_count
                    .checked_add(selectors.len())
                    .ok_or(VectorDecodeError::ResourceLimit)?;
                declaration_count = declaration_count
                    .checked_add(declarations.len())
                    .ok_or(VectorDecodeError::ResourceLimit)?;
                if selector_count > super::MAX_CSS_SELECTORS
                    || declaration_count > super::MAX_CSS_DECLARATIONS
                {
                    return Err(VectorDecodeError::ResourceLimit);
                }
                if !selectors.is_empty() && !declarations.is_empty() {
                    let order = rules.len();
                    rules.push(CssRule {
                        selectors,
                        declarations,
                        order,
                    });
                }
                if rules.len() > super::MAX_CSS_RULES {
                    return Err(VectorDecodeError::ResourceLimit);
                }
            }
        }
        Ok(Self {
            rules,
            uses_z_index,
        })
    }

    pub(super) const fn uses_z_index(&self) -> bool {
        self.uses_z_index
    }

    pub(super) fn compute(
        &self,
        elements: &[Element<'_>],
        element: &Element<'_>,
        inherited: &Style,
        viewport: Size,
    ) -> Result<Style, VectorDecodeError> {
        let mut style = inherited.clone();
        style.opacity = 1.0;
        style.blend_mode = BlendMode::Normal;
        style.isolated = false;
        style.z_index = 0;
        style.clip_path = None;
        style.mask = None;
        style.displayed = true;
        style.overflow_visible = false;
        style.path_length = None;
        style.transform = None;
        style.transform_origin = None;
        style.transform_fill_box = false;
        style.geometry = GeometryStyle::default();
        let mut declarations = Vec::<(&str, &str, bool, (bool, u8, u32, usize))>::with_capacity(
            element.attributes.len(),
        );
        for (order, &(prefix, name, value)) in element.attributes.iter().enumerate() {
            if prefix.is_empty() {
                declarations.push((name, value, false, (false, 0, 0, order)));
            }
        }
        for rule in &self.rules {
            if let Some(specificity) = rule
                .selectors
                .iter()
                .filter(|selector| selector.matches(element, elements))
                .map(|selector| selector.specificity)
                .max()
            {
                for (name, value, important) in &rule.declarations {
                    declarations.push((
                        name,
                        value,
                        true,
                        (*important, 1, specificity, rule.order),
                    ));
                }
            }
        }
        let inline_declarations = element
            .attr("style")
            .map(|source| {
                element
                    .inline_style
                    .get_or_init(|| css_declarations(source))
                    .as_ref()
                    .map_err(Clone::clone)
            })
            .transpose()?;
        if let Some(inline_declarations) = inline_declarations {
            for (order, (name, value, important)) in inline_declarations.iter().enumerate() {
                declarations.push((name, value, true, (*important, 2, u32::MAX, order)));
            }
        }
        declarations.sort_by_key(|declaration| declaration.3);
        for &(name, value, fallback, _) in &declarations {
            if name == "color" && value != "inherit" {
                match parse_color(value, style.color) {
                    Ok(color) => style.color = color,
                    Err(error) if !fallback => return Err(error),
                    Err(_) => {}
                }
            }
        }
        for (name, value, fallback, _) in declarations {
            let result = (|| -> Result<(), VectorDecodeError> {
                match name {
                    "color-interpolation" => {
                        style.color_interpolation = match value.to_ascii_lowercase().as_str() {
                            "srgb" | "auto" => ColorSpace::Srgb,
                            "linearrgb" => ColorSpace::LinearSrgb,
                            _ => return Err(VectorDecodeError::InvalidData),
                        }
                    }
                    "font-size" | "fill" | "stroke" | "fill-opacity" | "stroke-opacity"
                    | "fill-rule" | "clip-rule" | "visibility" | "stroke-width"
                    | "stroke-linecap" | "stroke-linejoin" | "stroke-miterlimit"
                    | "stroke-dasharray" | "stroke-dashoffset" | "marker" | "marker-start"
                    | "marker-mid" | "marker-end"
                        if value == "inherit" => {}
                    "fill" => {
                        style.fill = resolve_context_paint(
                            parse_paint(value, style.color)?,
                            &style.context_fill,
                            &style.context_stroke,
                            style.context_paint_available,
                        )?
                    }
                    "stroke" => {
                        style.stroke = resolve_context_paint(
                            parse_paint(value, style.color)?,
                            &style.context_fill,
                            &style.context_stroke,
                            style.context_paint_available,
                        )?
                    }
                    "fill-opacity" => style.fill_opacity = unit_number(value)?,
                    "stroke-opacity" => style.stroke_opacity = unit_number(value)?,
                    "opacity" => style.opacity = unit_number(value)?,
                    "mix-blend-mode" => {
                        style.blend_mode = match value.to_ascii_lowercase().as_str() {
                            "normal" => BlendMode::Normal,
                            "multiply" => BlendMode::Multiply,
                            "screen" => BlendMode::Screen,
                            "overlay" => BlendMode::Overlay,
                            "darken" => BlendMode::Darken,
                            "lighten" => BlendMode::Lighten,
                            "color-dodge" => BlendMode::ColorDodge,
                            "color-burn" => BlendMode::ColorBurn,
                            "hard-light" => BlendMode::HardLight,
                            "soft-light" => BlendMode::SoftLight,
                            "difference" => BlendMode::Difference,
                            "exclusion" => BlendMode::Exclusion,
                            "hue" => BlendMode::Hue,
                            "saturation" => BlendMode::Saturation,
                            "color" => BlendMode::Color,
                            "luminosity" => BlendMode::Luminosity,
                            _ => return Err(VectorDecodeError::InvalidData),
                        }
                    }
                    "isolation" => {
                        style.isolated = match value.to_ascii_lowercase().as_str() {
                            "auto" => false,
                            "isolate" => true,
                            _ => return Err(VectorDecodeError::InvalidData),
                        }
                    }
                    "z-index" => {
                        style.z_index = if value.eq_ignore_ascii_case("auto") {
                            0
                        } else {
                            value
                                .parse::<i32>()
                                .map_err(|_| VectorDecodeError::InvalidData)?
                        }
                    }
                    "fill-rule" | "clip-rule" => {
                        style.fill_rule = match value {
                            "nonzero" => FillRule::NonZero,
                            "evenodd" => FillRule::EvenOdd,
                            _ => return Err(VectorDecodeError::InvalidData),
                        }
                    }
                    "visibility" => {
                        style.visible = match value {
                            "visible" => true,
                            "hidden" | "collapse" => false,
                            _ => return Err(VectorDecodeError::InvalidData),
                        }
                    }
                    "display" => {
                        style.displayed = match value {
                            "none" => false,
                            "inline" | "block" => true,
                            _ => {
                                return Err(VectorDecodeError::UnsupportedFeature("display value"));
                            }
                        }
                    }
                    "overflow" => {
                        style.overflow_visible = match value.to_ascii_lowercase().as_str() {
                            "visible" => true,
                            "hidden" | "scroll" | "auto" => false,
                            _ => return Err(VectorDecodeError::InvalidData),
                        }
                    }
                    "pathLength" | "path-length" => {
                        let value =
                            length(value, normalized_diagonal(viewport.width, viewport.height))?;
                        if value >= 0.0 {
                            style.path_length = Some(value);
                        }
                    }
                    "transform" => style.transform = Some(parse_css_transform(value)?),
                    "transform-origin" => {
                        let values = value.split_ascii_whitespace().collect::<Vec<_>>();
                        if values.len() != 2 {
                            return Err(VectorDecodeError::InvalidData);
                        }
                        style.transform_origin = Some((
                            transform_origin(values[0], viewport.width)?,
                            transform_origin(values[1], viewport.height)?,
                        ));
                    }
                    "transform-box" => {
                        style.transform_fill_box = match value {
                            "fill-box" | "stroke-box" | "content-box" | "border-box" => true,
                            "view-box" => false,
                            _ => return Err(VectorDecodeError::InvalidData),
                        }
                    }
                    "paint-order" => {
                        if value.eq_ignore_ascii_case("normal") {
                            style.paint_order =
                                [PaintLayer::Fill, PaintLayer::Stroke, PaintLayer::Markers];
                        } else {
                            let mut order = Vec::new();
                            for value in value.split_ascii_whitespace() {
                                let layer = match value.to_ascii_lowercase().as_str() {
                                    "fill" => PaintLayer::Fill,
                                    "stroke" => PaintLayer::Stroke,
                                    "markers" => PaintLayer::Markers,
                                    _ => return Err(VectorDecodeError::InvalidData),
                                };
                                if order.contains(&layer) {
                                    return Err(VectorDecodeError::InvalidData);
                                }
                                order.push(layer);
                            }
                            if order.is_empty() {
                                return Err(VectorDecodeError::InvalidData);
                            }
                            for layer in [PaintLayer::Fill, PaintLayer::Stroke, PaintLayer::Markers]
                            {
                                if !order.contains(&layer) {
                                    order.push(layer);
                                }
                            }
                            style.paint_order = order
                                .try_into()
                                .expect("paint order contains every layer exactly once");
                        }
                    }
                    "marker" => {
                        let reference = paint_server_reference(value)?;
                        style.marker_start.clone_from(&reference);
                        style.marker_mid.clone_from(&reference);
                        style.marker_end = reference;
                    }
                    "marker-start" => style.marker_start = paint_server_reference(value)?,
                    "marker-mid" => style.marker_mid = paint_server_reference(value)?,
                    "marker-end" => style.marker_end = paint_server_reference(value)?,
                    "stroke-width" => {
                        let axis = normalized_diagonal(viewport.width, viewport.height);
                        let width = length(value, axis)?;
                        if width < 0.0 {
                            return Err(VectorDecodeError::InvalidData);
                        }
                        style.stroke_style.width = width;
                    }
                    "stroke-linecap" => {
                        style.stroke_style.line_cap = match value {
                            "butt" => LineCap::Butt,
                            "round" => LineCap::Round,
                            "square" => LineCap::Square,
                            _ => return Err(VectorDecodeError::InvalidData),
                        }
                    }
                    "stroke-linejoin" => {
                        style.stroke_style.line_join = match value {
                            "miter" | "miter-clip" => LineJoin::Miter,
                            "round" => LineJoin::Round,
                            "bevel" | "arcs" => LineJoin::Bevel,
                            _ => return Err(VectorDecodeError::InvalidData),
                        }
                    }
                    "stroke-miterlimit" => style.stroke_style.miter_limit = positive_number(value)?,
                    "stroke-dasharray" => {
                        let mut values = if value == "none" {
                            Vec::new()
                        } else {
                            length_list(
                                value,
                                normalized_diagonal(viewport.width, viewport.height),
                            )?
                        };
                        if values.iter().any(|value| *value < 0.0) {
                            return Err(VectorDecodeError::InvalidData);
                        }
                        if values.len() % 2 == 1 {
                            values.extend(values.clone());
                        }
                        style.stroke_style.dash_array = values;
                    }
                    "stroke-dashoffset" => {
                        style.stroke_style.dash_offset =
                            length(value, normalized_diagonal(viewport.width, viewport.height))?
                    }
                    "clip-path" => {
                        style.clip_path = if value == "none" {
                            None
                        } else {
                            Some(
                                value
                                    .strip_prefix("url(")
                                    .and_then(|value| value.strip_suffix(')'))
                                    .ok_or(VectorDecodeError::InvalidData)?
                                    .trim()
                                    .to_owned(),
                            )
                        }
                    }
                    "mask" => {
                        style.mask = if value == "none" {
                            None
                        } else {
                            Some(
                                value
                                    .strip_prefix("url(")
                                    .and_then(|value| value.strip_suffix(')'))
                                    .ok_or(VectorDecodeError::InvalidData)?
                                    .trim()
                                    .to_owned(),
                            )
                        }
                    }
                    "stop-color" => style.stop_color = parse_color(value, style.color)?,
                    "stop-opacity" => style.stop_opacity = unit_number(value)?,
                    "x" => style.geometry.x = Some(geometry_value(value, viewport.width, false)?),
                    "y" => style.geometry.y = Some(geometry_value(value, viewport.height, false)?),
                    "x1" => style.geometry.x1 = Some(geometry_value(value, viewport.width, false)?),
                    "y1" => {
                        style.geometry.y1 = Some(geometry_value(value, viewport.height, false)?)
                    }
                    "x2" => style.geometry.x2 = Some(geometry_value(value, viewport.width, false)?),
                    "y2" => {
                        style.geometry.y2 = Some(geometry_value(value, viewport.height, false)?)
                    }
                    "cx" => style.geometry.cx = Some(geometry_value(value, viewport.width, false)?),
                    "cy" => {
                        style.geometry.cy = Some(geometry_value(value, viewport.height, false)?)
                    }
                    "r" => {
                        style.geometry.r = Some(geometry_value(
                            value,
                            normalized_diagonal(viewport.width, viewport.height),
                            false,
                        )?)
                    }
                    "rx" => style.geometry.rx = Some(geometry_value(value, viewport.width, true)?),
                    "ry" => style.geometry.ry = Some(geometry_value(value, viewport.height, true)?),
                    "width" => {
                        style.geometry.width = Some(geometry_value(value, viewport.width, true)?)
                    }
                    "height" => {
                        style.geometry.height = Some(geometry_value(value, viewport.height, true)?)
                    }
                    _ => {}
                }
                Ok(())
            })();
            if let Err(error) = result {
                if matches!(error, VectorDecodeError::UnsupportedFeature(_)) && fallback {
                    continue;
                }
                if !fallback {
                    return Err(error);
                }
            }
        }
        Ok(style)
    }
}

pub(super) fn skips_unsupported_subtree(name: &str) -> bool {
    matches!(
        name,
        "filter"
            | "pattern"
            | "text"
            | "tspan"
            | "textPath"
            | "image"
            | "style"
            | "script"
            | "foreignObject"
            | "animate"
            | "animateTransform"
            | "set"
    )
}

pub(super) fn local_reference(value: &str) -> Result<&str, VectorDecodeError> {
    value
        .strip_prefix('#')
        .filter(|id| !id.is_empty())
        .ok_or(VectorDecodeError::UnsupportedFeature("external URL"))
}

fn resolve_context_paint(
    value: PaintValue,
    fill: &PaintValue,
    stroke: &PaintValue,
    available: bool,
) -> Result<PaintValue, VectorDecodeError> {
    match value {
        PaintValue::ContextFill if available => Ok(fill.clone()),
        PaintValue::ContextStroke if available => Ok(stroke.clone()),
        PaintValue::ContextFill | PaintValue::ContextStroke => {
            Err(VectorDecodeError::UnsupportedFeature("context paint"))
        }
        value => Ok(value),
    }
}

fn geometry_value(
    value: &str,
    percentage: f64,
    allows_auto: bool,
) -> Result<GeometryValue, VectorDecodeError> {
    if value == "auto" && allows_auto {
        Ok(GeometryValue::Auto)
    } else {
        Ok(GeometryValue::Length(length(value, percentage)?))
    }
}

fn transform_origin(value: &str, percentage: f64) -> Result<TransformOrigin, VectorDecodeError> {
    if let Some(value) = value.strip_suffix('%') {
        Ok(TransformOrigin::Percentage(number(value)? / 100.0))
    } else {
        Ok(TransformOrigin::Length(length(value, percentage)?))
    }
}

fn parse_paint(value: &str, current: Color) -> Result<PaintValue, VectorDecodeError> {
    if value.eq_ignore_ascii_case("none") {
        Ok(PaintValue::None)
    } else if value.eq_ignore_ascii_case("currentcolor") {
        Ok(PaintValue::Color(current))
    } else if value.eq_ignore_ascii_case("context-fill") {
        Ok(PaintValue::ContextFill)
    } else if value.eq_ignore_ascii_case("context-stroke") {
        Ok(PaintValue::ContextStroke)
    } else if let Some(rest) = value.strip_prefix("url(") {
        let close = rest.find(')').ok_or(VectorDecodeError::InvalidData)?;
        let reference = rest[..close].trim().to_owned();
        let fallback = rest[close + 1..].trim();
        let fallback = if fallback.is_empty() {
            None
        } else if fallback.eq_ignore_ascii_case("none") {
            Some(Box::new(PaintValue::None))
        } else if fallback.eq_ignore_ascii_case("currentcolor") {
            Some(Box::new(PaintValue::Color(current)))
        } else {
            Some(Box::new(PaintValue::Color(parse_color(fallback, current)?)))
        };
        Ok(PaintValue::Reference {
            reference,
            fallback,
        })
    } else {
        Ok(PaintValue::Color(parse_color(value, current)?))
    }
}

fn paint_server_reference(value: &str) -> Result<Option<String>, VectorDecodeError> {
    if value.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    value
        .strip_prefix("url(")
        .and_then(|value| value.strip_suffix(')'))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| Some(value.to_owned()))
        .ok_or(VectorDecodeError::InvalidData)
}

pub(super) fn parse_color(value: &str, current: Color) -> Result<Color, VectorDecodeError> {
    if value.eq_ignore_ascii_case("currentcolor") {
        return Ok(current);
    }
    let rgba = if value.eq_ignore_ascii_case("transparent") {
        [0, 0, 0, 0]
    } else if let Some(hex) = value.strip_prefix('#') {
        parse_hex(hex)?
    } else if value
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("rgb("))
        || value
            .get(..5)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("rgba("))
    {
        parse_rgb(value)?
    } else {
        named_color(value).ok_or(VectorDecodeError::InvalidData)?
    };
    Ok(Color {
        red: f64::from(rgba[0]) / 255.0,
        green: f64::from(rgba[1]) / 255.0,
        blue: f64::from(rgba[2]) / 255.0,
        alpha: f64::from(rgba[3]) / 255.0,
        color_space: ColorSpace::Srgb,
    })
}

fn parse_hex(value: &str) -> Result<[u8; 4], VectorDecodeError> {
    let digit = |byte: u8| {
        char::from(byte)
            .to_digit(16)
            .map(|v| v as u8)
            .ok_or(VectorDecodeError::InvalidData)
    };
    match value.as_bytes() {
        [r, g, b] => Ok([digit(*r)? * 17, digit(*g)? * 17, digit(*b)? * 17, 255]),
        [r, g, b, a] => Ok([
            digit(*r)? * 17,
            digit(*g)? * 17,
            digit(*b)? * 17,
            digit(*a)? * 17,
        ]),
        [r0, r1, g0, g1, b0, b1] => Ok([
            digit(*r0)? * 16 + digit(*r1)?,
            digit(*g0)? * 16 + digit(*g1)?,
            digit(*b0)? * 16 + digit(*b1)?,
            255,
        ]),
        [r0, r1, g0, g1, b0, b1, a0, a1] => Ok([
            digit(*r0)? * 16 + digit(*r1)?,
            digit(*g0)? * 16 + digit(*g1)?,
            digit(*b0)? * 16 + digit(*b1)?,
            digit(*a0)? * 16 + digit(*a1)?,
        ]),
        _ => Err(VectorDecodeError::InvalidData),
    }
}

fn parse_rgb(value: &str) -> Result<[u8; 4], VectorDecodeError> {
    let rgba = value
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("rgba("));
    let inside = value
        .split_once('(')
        .and_then(|(_, rest)| rest.strip_suffix(')'))
        .ok_or(VectorDecodeError::InvalidData)?;
    let component = |part: &str| -> Result<u8, VectorDecodeError> {
        if let Some(value) = part.strip_suffix('%') {
            Ok((number(value)?.clamp(0.0, 100.0) * 2.55).round() as u8)
        } else {
            Ok(number(part)?.clamp(0.0, 255.0).round() as u8)
        }
    };
    let alpha = |part: &str| -> Result<u8, VectorDecodeError> {
        if let Some(value) = part.strip_suffix('%') {
            Ok((number(value)?.clamp(0.0, 100.0) * 2.55).round() as u8)
        } else {
            Ok((number(part)?.clamp(0.0, 1.0) * 255.0).round() as u8)
        }
    };
    if inside.contains(',') {
        if inside.contains('/') {
            return Err(VectorDecodeError::InvalidData);
        }
        let parts = inside.split(',').map(str::trim).collect::<Vec<_>>();
        let expected = if rgba { 4 } else { 3 };
        if parts.len() != expected || parts.iter().any(|part| part.is_empty()) {
            return Err(VectorDecodeError::InvalidData);
        }
        Ok([
            component(parts[0])?,
            component(parts[1])?,
            component(parts[2])?,
            if rgba { alpha(parts[3])? } else { 255 },
        ])
    } else {
        let mut sides = inside.split('/');
        let colors = sides
            .next()
            .ok_or(VectorDecodeError::InvalidData)?
            .split_whitespace()
            .collect::<Vec<_>>();
        if colors.len() != 3 {
            return Err(VectorDecodeError::InvalidData);
        }
        let alpha = if let Some(value) = sides.next() {
            let values = value.split_whitespace().collect::<Vec<_>>();
            if values.len() != 1 || sides.next().is_some() {
                return Err(VectorDecodeError::InvalidData);
            }
            alpha(values[0])?
        } else {
            255
        };
        Ok([
            component(colors[0])?,
            component(colors[1])?,
            component(colors[2])?,
            alpha,
        ])
    }
}

fn named_color(value: &str) -> Option<[u8; 4]> {
    let rgb = match value.to_ascii_lowercase().as_str() {
        "black" => 0x000000,
        "white" => 0xffffff,
        "red" => 0xff0000,
        "green" => 0x008000,
        "blue" => 0x0000ff,
        "yellow" => 0xffff00,
        "gray" | "grey" => 0x808080,
        "silver" => 0xc0c0c0,
        "maroon" => 0x800000,
        "purple" => 0x800080,
        "fuchsia" | "magenta" => 0xff00ff,
        "lime" => 0x00ff00,
        "olive" => 0x808000,
        "navy" => 0x000080,
        "teal" => 0x008080,
        "aqua" | "cyan" => 0x00ffff,
        "orange" => 0xffa500,
        "aliceblue" => 0xf0f8ff,
        "antiquewhite" => 0xfaebd7,
        "aquamarine" => 0x7fffd4,
        "azure" => 0xf0ffff,
        "beige" => 0xf5f5dc,
        "bisque" => 0xffe4c4,
        "blanchedalmond" => 0xffebcd,
        "blueviolet" => 0x8a2be2,
        "brown" => 0xa52a2a,
        "burlywood" => 0xdeb887,
        "cadetblue" => 0x5f9ea0,
        "chartreuse" => 0x7fff00,
        "chocolate" => 0xd2691e,
        "coral" => 0xff7f50,
        "cornflowerblue" => 0x6495ed,
        "cornsilk" => 0xfff8dc,
        "crimson" => 0xdc143c,
        "darkblue" => 0x00008b,
        "darkcyan" => 0x008b8b,
        "darkgoldenrod" => 0xb8860b,
        "darkgray" | "darkgrey" => 0xa9a9a9,
        "darkgreen" => 0x006400,
        "darkkhaki" => 0xbdb76b,
        "darkmagenta" => 0x8b008b,
        "darkolivegreen" => 0x556b2f,
        "darkorange" => 0xff8c00,
        "darkorchid" => 0x9932cc,
        "darkred" => 0x8b0000,
        "darksalmon" => 0xe9967a,
        "darkseagreen" => 0x8fbc8f,
        "darkslateblue" => 0x483d8b,
        "darkslategray" | "darkslategrey" => 0x2f4f4f,
        "darkturquoise" => 0x00ced1,
        "darkviolet" => 0x9400d3,
        "deeppink" => 0xff1493,
        "deepskyblue" => 0x00bfff,
        "dimgray" | "dimgrey" => 0x696969,
        "dodgerblue" => 0x1e90ff,
        "firebrick" => 0xb22222,
        "floralwhite" => 0xfffaf0,
        "forestgreen" => 0x228b22,
        "gainsboro" => 0xdcdcdc,
        "ghostwhite" => 0xf8f8ff,
        "gold" => 0xffd700,
        "goldenrod" => 0xdaa520,
        "greenyellow" => 0xadff2f,
        "honeydew" => 0xf0fff0,
        "hotpink" => 0xff69b4,
        "indianred" => 0xcd5c5c,
        "indigo" => 0x4b0082,
        "ivory" => 0xfffff0,
        "khaki" => 0xf0e68c,
        "lavender" => 0xe6e6fa,
        "lavenderblush" => 0xfff0f5,
        "lawngreen" => 0x7cfc00,
        "lemonchiffon" => 0xfffacd,
        "lightblue" => 0xadd8e6,
        "lightcoral" => 0xf08080,
        "lightcyan" => 0xe0ffff,
        "lightgoldenrodyellow" => 0xfafad2,
        "lightgray" | "lightgrey" => 0xd3d3d3,
        "lightgreen" => 0x90ee90,
        "lightpink" => 0xffb6c1,
        "lightsalmon" => 0xffa07a,
        "lightseagreen" => 0x20b2aa,
        "lightskyblue" => 0x87cefa,
        "lightslategray" | "lightslategrey" => 0x778899,
        "lightsteelblue" => 0xb0c4de,
        "lightyellow" => 0xffffe0,
        "limegreen" => 0x32cd32,
        "linen" => 0xfaf0e6,
        "mediumaquamarine" => 0x66cdaa,
        "mediumblue" => 0x0000cd,
        "mediumorchid" => 0xba55d3,
        "mediumpurple" => 0x9370db,
        "mediumseagreen" => 0x3cb371,
        "mediumslateblue" => 0x7b68ee,
        "mediumspringgreen" => 0x00fa9a,
        "mediumturquoise" => 0x48d1cc,
        "mediumvioletred" => 0xc71585,
        "midnightblue" => 0x191970,
        "mintcream" => 0xf5fffa,
        "mistyrose" => 0xffe4e1,
        "moccasin" => 0xffe4b5,
        "navajowhite" => 0xffdead,
        "oldlace" => 0xfdf5e6,
        "olivedrab" => 0x6b8e23,
        "orangered" => 0xff4500,
        "orchid" => 0xda70d6,
        "palegoldenrod" => 0xeee8aa,
        "palegreen" => 0x98fb98,
        "paleturquoise" => 0xafeeee,
        "palevioletred" => 0xdb7093,
        "papayawhip" => 0xffefd5,
        "peachpuff" => 0xffdab9,
        "peru" => 0xcd853f,
        "pink" => 0xffc0cb,
        "plum" => 0xdda0dd,
        "powderblue" => 0xb0e0e6,
        "rebeccapurple" => 0x663399,
        "rosybrown" => 0xbc8f8f,
        "royalblue" => 0x4169e1,
        "saddlebrown" => 0x8b4513,
        "salmon" => 0xfa8072,
        "sandybrown" => 0xf4a460,
        "seagreen" => 0x2e8b57,
        "seashell" => 0xfff5ee,
        "sienna" => 0xa0522d,
        "skyblue" => 0x87ceeb,
        "slateblue" => 0x6a5acd,
        "slategray" | "slategrey" => 0x708090,
        "snow" => 0xfffafa,
        "springgreen" => 0x00ff7f,
        "steelblue" => 0x4682b4,
        "tan" => 0xd2b48c,
        "thistle" => 0xd8bfd8,
        "tomato" => 0xff6347,
        "turquoise" => 0x40e0d0,
        "violet" => 0xee82ee,
        "wheat" => 0xf5deb3,
        "whitesmoke" => 0xf5f5f5,
        "yellowgreen" => 0x9acd32,
        _ => return None,
    };
    Some([(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 255])
}
