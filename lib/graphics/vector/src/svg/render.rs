/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use bcanvas::{CanvasRenderingContext2d, OffscreenCanvas};

use super::geometry::{
    MarkerPlacement, PathMetrics, inverse_transform, object_bbox_length, object_bounds,
    parse_mask_type, transform_segment,
};
use super::path::rect_path;
use super::style::{
    PaintLayer, PaintValue, Style, Stylesheet, local_reference, skips_unsupported_subtree,
};
use super::values::{
    compatible_length, length, parse_transform, parse_view_box, unit_number, view_box_transform,
};
use super::xml::XmlDocument;
use super::{MAX_ITEMS, MAX_SOURCE_BYTES};
use crate::draw::{self, RenderMode, add_path, composite_operation, new_canvas, set_transform};
use crate::{
    BlendMode, FillRule, MaskType, Paint, PathSegment, Point, Rect, Size, StrokeStyle, Transform,
    VectorDecodeError, path_bounds,
};

pub(super) struct Decoder<'a> {
    document: XmlDocument<'a>,
    styles: Stylesheet,
}

impl<'a> Decoder<'a> {
    pub(super) fn new(data: &'a [u8]) -> Result<Self, VectorDecodeError> {
        if data.len() > MAX_SOURCE_BYTES {
            return Err(VectorDecodeError::ResourceLimit);
        }
        let source = std::str::from_utf8(data).map_err(|_| VectorDecodeError::InvalidData)?;
        let source = source.trim_prefix('\u{feff}');
        let document = XmlDocument::parse(source)?;
        let styles = Stylesheet::parse(&document.elements)?;
        Ok(Self { document, styles })
    }

    pub(super) fn decode(self) -> Result<OffscreenCanvas, VectorDecodeError> {
        Composer::new(&self.document, &self.styles)?.compose()
    }
}

// A path clipping the drawing of an element.
struct Clip {
    path: Vec<PathSegment>,
    transform: Transform,
    rule: FillRule,
}

// A resolved mask, whose content is drawn as the coverage of the masked content.
struct Mask {
    index: usize,
    region: Clip,
    content_transform: Transform,
    viewport: Size,
    style: Style,
    mode: RenderMode,
}

// A visible shape with its resolved paints.
struct Shape {
    path: Vec<PathSegment>,
    viewport: Size,
    bounds: Rect,
    fill: Option<Paint>,
    stroke: Option<(Paint, StrokeStyle)>,
    markers: Option<(
        Vec<MarkerPlacement>,
        Vec<MarkerPlacement>,
        Vec<MarkerPlacement>,
    )>,
}

struct Composer<'document, 'source> {
    // The parsed document and cascade stay immutable while composition draws.
    document: &'document XmlDocument<'source>,
    styles: &'document Stylesheet,
    size: Size,
    references: Vec<usize>,
    items: usize,
    // The local bounds and transform of each drawn path, for the object bounding boxes of
    // masked content, which is measured while `measuring` is nonzero.
    drawn: Vec<(Rect, Transform)>,
    measuring: usize,
    mode: RenderMode,
}

impl<'document, 'source> Composer<'document, 'source> {
    fn new(
        document: &'document XmlDocument<'source>,
        styles: &'document Stylesheet,
    ) -> Result<Self, VectorDecodeError> {
        let root = &document.elements[document.root];
        let view_box = root.attr("viewBox").map(parse_view_box).transpose()?;
        let default = view_box
            .filter(|value| value.width > 0.0 && value.height > 0.0)
            .map_or((300.0, 150.0), |value| (value.width, value.height));
        let width = compatible_length(root.attr("width"), default.0)?.unwrap_or(default.0);
        let height = compatible_length(root.attr("height"), default.1)?.unwrap_or(default.1);
        if width <= 0.0 || height <= 0.0 {
            return Err(VectorDecodeError::InvalidData);
        }
        Ok(Self {
            document,
            styles,
            size: Size { width, height },
            references: Vec::new(),
            items: 0,
            drawn: Vec::new(),
            measuring: 0,
            mode: RenderMode::Normal,
        })
    }

    fn compose(mut self) -> Result<OffscreenCanvas, VectorDecodeError> {
        let root = &self.document.elements[self.document.root];
        let view_box = root.attr("viewBox").map(parse_view_box).transpose()?;
        if view_box.is_some_and(|view| view.width == 0.0 || view.height == 0.0) {
            return Ok(new_canvas(self.size));
        }
        let transform = match view_box {
            Some(view_box) => view_box_transform(
                view_box,
                self.size.width,
                self.size.height,
                root.attr("preserveAspectRatio"),
            )?,
            None => Transform::IDENTITY,
        };
        let viewport = view_box.map_or(self.size, |view_box| Size {
            width: view_box.width,
            height: view_box.height,
        });
        let root = self.document.root;
        self.record(|this, context| {
            this.render(context, root, &Style::default(), transform, viewport, None)
        })
    }

    // Draws into a new recording of the document size.
    fn record(
        &mut self,
        draw: impl FnOnce(&mut Self, &mut CanvasRenderingContext2d<'_>) -> Result<(), VectorDecodeError>,
    ) -> Result<OffscreenCanvas, VectorDecodeError> {
        let mut canvas = new_canvas(self.size);
        let mut result = Ok(());
        canvas.draw(|context| result = draw(self, context));
        result.map(|()| canvas)
    }

    // Counts decoded items against the resource limit.
    fn claim(&mut self, items: usize) -> Result<(), VectorDecodeError> {
        self.items = self
            .items
            .checked_add(items)
            .filter(|&items| items <= MAX_ITEMS)
            .ok_or(VectorDecodeError::ResourceLimit)?;
        Ok(())
    }

    // Draws an element, skipping it when it needs an unsupported feature. Elements fail with
    // an unsupported feature only before they draw.
    fn render(
        &mut self,
        context: &mut CanvasRenderingContext2d<'_>,
        index: usize,
        inherited: &Style,
        parent_transform: Transform,
        viewport: Size,
        instance: Option<usize>,
    ) -> Result<(), VectorDecodeError> {
        let items = self.items;
        match self.render_inner(
            context,
            index,
            inherited,
            parent_transform,
            viewport,
            instance,
        ) {
            Err(VectorDecodeError::UnsupportedFeature(_)) => {
                self.items = items;
                Ok(())
            }
            result => result,
        }
    }

    fn render_inner(
        &mut self,
        context: &mut CanvasRenderingContext2d<'_>,
        index: usize,
        inherited: &Style,
        parent_transform: Transform,
        viewport: Size,
        instance: Option<usize>,
    ) -> Result<(), VectorDecodeError> {
        if self.references.len() >= 64 {
            return Err(VectorDecodeError::ResourceLimit);
        }
        let element = &self.document.elements[index];
        let is_root = index == self.document.root && instance.is_none();
        if !element.is_svg || skips_unsupported_subtree(element.name) {
            return Ok(());
        }
        if element.name == "symbol" && instance.is_none() {
            return Ok(());
        }
        if element.name == "marker" {
            return Ok(());
        }
        if matches!(
            element.name,
            "defs" | "linearGradient" | "radialGradient" | "stop" | "clipPath" | "mask"
        ) {
            return Ok(());
        }
        let style = self
            .styles
            .compute(&self.document.elements, element, inherited, viewport)?;
        if !style.displayed {
            return Ok(());
        }
        let mut local = style.transform.unwrap_or(Transform::IDENTITY);
        let mut child_viewport = viewport;
        let mut viewport_clip = if element.name == "svg" && is_root {
            let bounds = Rect {
                x: 0.0,
                y: 0.0,
                width: self.size.width,
                height: self.size.height,
            };
            Some(self.viewport_clip(bounds, Transform::IDENTITY)?)
        } else {
            None
        };
        if element.name == "svg" && !is_root || element.name == "symbol" {
            let instance = instance.map(|index| &self.document.elements[index]);
            let attr = |name: &str| {
                instance
                    .and_then(|element| element.attr(name))
                    .or_else(|| element.attr(name))
            };
            let x = attr("x")
                .map(|value| length(value, viewport.width))
                .transpose()?
                .unwrap_or(0.0);
            let y = attr("y")
                .map(|value| length(value, viewport.height))
                .transpose()?
                .unwrap_or(0.0);
            let width = attr("width")
                .map(|value| length(value, viewport.width))
                .transpose()?
                .unwrap_or(viewport.width);
            let height = attr("height")
                .map(|value| length(value, viewport.height))
                .transpose()?
                .unwrap_or(viewport.height);
            if width <= 0.0 || height <= 0.0 {
                return Ok(());
            }
            let viewport_transform = local
                .then(Transform {
                    e: x,
                    f: y,
                    ..Transform::IDENTITY
                })
                .then(parent_transform);
            if !style.overflow_visible {
                viewport_clip = Some(self.viewport_clip(
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        width,
                        height,
                    },
                    viewport_transform,
                )?);
            }
            let view = element.attr("viewBox").map(parse_view_box).transpose()?;
            if view.is_some_and(|view| view.width == 0.0 || view.height == 0.0) {
                return Ok(());
            }
            child_viewport = view.map_or(Size { width, height }, |view| Size {
                width: view.width,
                height: view.height,
            });
            let view_transform = if let Some(view) = view {
                view_box_transform(view, width, height, element.attr("preserveAspectRatio"))?
            } else {
                Transform::IDENTITY
            };
            local = local.then(view_transform).then(Transform {
                e: x,
                f: y,
                ..Transform::IDENTITY
            });
        }
        let mut transform = local.then(parent_transform);
        if element.name == "use" {
            let href = element
                .attr("href")
                .or_else(|| element.attr_prefixed("xlink", "href"))
                .ok_or(VectorDecodeError::InvalidData)?;
            let id = local_reference(href)?;
            let target = *self
                .document
                .ids
                .get(id)
                .ok_or(VectorDecodeError::InvalidData)?;
            if self.references.contains(&target) {
                return Err(VectorDecodeError::InvalidData);
            }
            let target_is_viewport =
                matches!(self.document.elements[target].name, "svg" | "symbol");
            let x = if target_is_viewport {
                0.0
            } else {
                style.geometry.x.map_or(0.0, |value| value.length_or(0.0))
            };
            let y = if target_is_viewport {
                0.0
            } else {
                style.geometry.y.map_or(0.0, |value| value.length_or(0.0))
            };
            // Each instance counts, which bounds nested references to the same content.
            self.claim(1)?;
            self.references.push(target);
            let result = self.render(
                context,
                target,
                &style,
                Transform {
                    e: x,
                    f: y,
                    ..Transform::IDENTITY
                }
                .then(transform),
                viewport,
                Some(index),
            );
            self.references.pop();
            return result;
        }
        let mut clips = viewport_clip.into_iter().collect::<Vec<_>>();
        if let Some(reference) = &style.clip_path {
            match self.resolve_clip(reference, transform, viewport) {
                Ok(clip) => clips.push(clip),
                Err(VectorDecodeError::UnsupportedFeature(_)) => {}
                Err(error) => return Err(error),
            }
        }
        let mut shape = match style.geometry.path(element)? {
            Some(path) if style.visible && !path.is_empty() => {
                self.shape(path, &style, parent_transform, viewport, &mut transform)?
            }
            Some(_) => None,
            None if !matches!(element.name, "svg" | "g" | "a" | "symbol") => {
                return Err(VectorDecodeError::UnsupportedFeature("element"));
            }
            None => None,
        };
        let scoped = style.opacity < 1.0
            || style.blend_mode != BlendMode::Normal
            || style.isolated
            || !clips.is_empty()
            || style.mask.is_some();
        let mut content = |this: &mut Self, context: &mut CanvasRenderingContext2d<'_>| {
            this.draw_content(
                context,
                index,
                &style,
                shape.as_mut(),
                transform,
                child_viewport,
            )
        };
        if !scoped {
            return content(self, context);
        }
        self.claim(1)?;
        let Some(reference) = &style.mask else {
            return self.scope(context, &style, &clips, None, content);
        };
        // The mask needs the bounds of the content, which is recorded first.
        let start = self.drawn.len();
        self.measuring += 1;
        let content = self.record(content);
        self.measuring -= 1;
        let content = content?;
        let inverse = inverse_transform(transform).ok_or(VectorDecodeError::InvalidData)?;
        let bounds = object_bounds(&self.drawn[start..], inverse).unwrap_or(Rect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        });
        if self.measuring == 0 {
            self.drawn.clear();
        }
        let mask = match self.resolve_mask(reference, transform, bounds, viewport) {
            Ok(mask) => Some(mask),
            Err(VectorDecodeError::UnsupportedFeature(_)) => None,
            Err(error) => return Err(error),
        };
        self.scope(context, &style, &clips, mask.as_ref(), |_, context| {
            context.reset_transform();
            context.draw_canvas(
                &content,
                0.0,
                0.0,
                content.logical_width(),
                content.logical_height(),
            );
            Ok(())
        })
    }

    // Resolves the paints of a visible shape, which may move the transform to its origin.
    fn shape(
        &mut self,
        path: Vec<PathSegment>,
        style: &Style,
        parent_transform: Transform,
        viewport: Size,
        transform: &mut Transform,
    ) -> Result<Option<Shape>, VectorDecodeError> {
        self.claim(path.len())?;
        let local_bounds = path_bounds(&path).ok_or(VectorDecodeError::InvalidData)?;
        if let (Some(local_transform), Some((origin_x, origin_y))) =
            (style.transform, style.transform_origin)
        {
            let box_bounds = if style.transform_fill_box {
                local_bounds
            } else {
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: viewport.width,
                    height: viewport.height,
                }
            };
            let origin = Point {
                x: origin_x.resolve(box_bounds.x, box_bounds.width),
                y: origin_y.resolve(box_bounds.y, box_bounds.height),
            };
            *transform = Transform {
                e: -origin.x,
                f: -origin.y,
                ..Transform::IDENTITY
            }
            .then(local_transform)
            .then(Transform {
                e: origin.x,
                f: origin.y,
                ..Transform::IDENTITY
            })
            .then(parent_transform);
        }
        let fill = self.resolve_paint(&style.fill, style.fill_opacity, local_bounds, viewport)?;
        let stroke = self
            .resolve_paint(&style.stroke, style.stroke_opacity, local_bounds, viewport)?
            .map(|paint| {
                let mut stroke_style = style.stroke_style.clone();
                if let Some(author_length) = style.path_length {
                    if author_length == 0.0 {
                        stroke_style.dash_array.clear();
                        stroke_style.dash_offset = 0.0;
                    } else {
                        let scale = PathMetrics::new(&path).length() / author_length;
                        for value in &mut stroke_style.dash_array {
                            *value *= scale;
                        }
                        stroke_style.dash_offset *= scale;
                    }
                }
                (paint, stroke_style)
            });
        let markers = (style.marker_start.is_some()
            || style.marker_mid.is_some()
            || style.marker_end.is_some())
        .then(|| PathMetrics::new(&path).marker_positions());
        Ok(
            (fill.is_some() || stroke.is_some() || markers.is_some()).then_some(Shape {
                path,
                viewport,
                bounds: local_bounds,
                fill,
                stroke,
                markers,
            }),
        )
    }

    // Draws a shape in its paint order, then the children.
    fn draw_content(
        &mut self,
        context: &mut CanvasRenderingContext2d<'_>,
        index: usize,
        style: &Style,
        shape: Option<&mut Shape>,
        transform: Transform,
        child_viewport: Size,
    ) -> Result<(), VectorDecodeError> {
        if let Some(shape) = shape {
            // The path stays current until markers draw their own.
            let mut current = false;
            for layer in style.paint_order {
                let paint = match layer {
                    PaintLayer::Fill => shape.fill.as_ref(),
                    PaintLayer::Stroke => shape.stroke.as_ref().map(|(paint, _)| paint),
                    PaintLayer::Markers => {
                        if let Some(positions) = shape.markers.take() {
                            self.render_shape_markers(
                                context,
                                style,
                                transform,
                                shape.viewport,
                                positions,
                            )?;
                            current = false;
                        }
                        continue;
                    }
                };
                let Some(paint) = paint else {
                    continue;
                };
                if !current {
                    set_transform(context, transform);
                    add_path(context, &shape.path);
                    if self.measuring > 0 {
                        self.drawn.push((shape.bounds, transform));
                    }
                    current = true;
                }
                if let (PaintLayer::Stroke, Some((_, stroke))) = (layer, &shape.stroke) {
                    draw::stroke(context, paint, stroke, self.mode);
                } else {
                    draw::fill(context, paint, style.fill_rule, self.mode);
                }
            }
        }
        let element = &self.document.elements[index];
        if self.styles.uses_z_index() && element.children.len() > 1 {
            let mut children = element
                .children
                .iter()
                .copied()
                .map(|child| {
                    match self.styles.compute(
                        &self.document.elements,
                        &self.document.elements[child],
                        style,
                        child_viewport,
                    ) {
                        Ok(child_style) => Ok((child_style.z_index, child)),
                        Err(VectorDecodeError::UnsupportedFeature(_)) => Ok((0, child)),
                        Err(error) => Err(error),
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;
            children.sort_by_key(|&(z_index, _)| z_index);
            for (_, child) in children {
                self.render(context, child, style, transform, child_viewport, None)?;
            }
        } else {
            for &child in &element.children {
                self.render(context, child, style, transform, child_viewport, None)?;
            }
        }
        Ok(())
    }

    // Draws content clipped, masked and composited with the opacity and blend mode of a style.
    fn scope(
        &mut self,
        context: &mut CanvasRenderingContext2d<'_>,
        style: &Style,
        clips: &[Clip],
        mask: Option<&Mask>,
        content: impl FnOnce(
            &mut Self,
            &mut CanvasRenderingContext2d<'_>,
        ) -> Result<(), VectorDecodeError>,
    ) -> Result<(), VectorDecodeError> {
        context.save();
        for clip in clips.iter().chain(mask.map(|mask| &mask.region)) {
            draw::clip(context, &clip.path, clip.transform, clip.rule);
        }
        context.begin_path();
        context.set_global_alpha(style.opacity as f32);
        context.set_global_composite_operation(composite_operation(style.blend_mode));
        let layer = mask.is_some() || style.opacity < 1.0 || style.isolated;
        if let Some(mask) = mask {
            context.begin_mask_layer();
            self.draw_mask(context, mask)?;
            context.end_mask();
        } else if layer {
            context.begin_layer();
        }
        content(self, context)?;
        if layer {
            context.end_layer();
        }
        context.restore();
        Ok(())
    }

    fn render_shape_markers(
        &mut self,
        context: &mut CanvasRenderingContext2d<'_>,
        style: &Style,
        transform: Transform,
        viewport: Size,
        positions: (
            Vec<MarkerPlacement>,
            Vec<MarkerPlacement>,
            Vec<MarkerPlacement>,
        ),
    ) -> Result<(), VectorDecodeError> {
        for (reference, placements) in [
            (style.marker_start.as_deref(), positions.0.as_slice()),
            (style.marker_mid.as_deref(), positions.1.as_slice()),
            (style.marker_end.as_deref(), positions.2.as_slice()),
        ] {
            let Some(reference) = reference else {
                continue;
            };
            let Ok(id) = local_reference(reference) else {
                continue;
            };
            let Some(&target) = self.document.ids.get(id) else {
                continue;
            };
            if self.document.elements[target].name != "marker" {
                continue;
            }
            for placement in placements {
                // A marker with an unsupported feature is skipped like an element.
                match self.render_marker(context, target, *placement, style, transform, viewport) {
                    Err(VectorDecodeError::UnsupportedFeature(_)) => {}
                    result => result?,
                }
            }
        }
        Ok(())
    }

    fn render_marker(
        &mut self,
        context: &mut CanvasRenderingContext2d<'_>,
        index: usize,
        placement: MarkerPlacement,
        context_style: &Style,
        parent_transform: Transform,
        viewport: Size,
    ) -> Result<(), VectorDecodeError> {
        if self.references.contains(&index) {
            return Err(VectorDecodeError::InvalidData);
        }
        let marker = &self.document.elements[index];
        let width = marker
            .attr("markerWidth")
            .map(|value| length(value, viewport.width))
            .transpose()?
            .unwrap_or(3.0);
        let height = marker
            .attr("markerHeight")
            .map(|value| length(value, viewport.height))
            .transpose()?
            .unwrap_or(3.0);
        if width <= 0.0 || height <= 0.0 {
            return Ok(());
        }
        let view = marker.attr("viewBox").map(parse_view_box).transpose()?;
        if view.is_some_and(|view| view.width == 0.0 || view.height == 0.0) {
            return Ok(());
        }
        let child_viewport = view.map_or(Size { width, height }, |view| Size {
            width: view.width,
            height: view.height,
        });
        let view_transform = if let Some(view) = view {
            view_box_transform(view, width, height, marker.attr("preserveAspectRatio"))?
        } else {
            Transform::IDENTITY
        };
        let reference = Point {
            x: marker
                .attr("refX")
                .map(|value| length(value, child_viewport.width))
                .transpose()?
                .unwrap_or(0.0),
            y: marker
                .attr("refY")
                .map(|value| length(value, child_viewport.height))
                .transpose()?
                .unwrap_or(0.0),
        };
        let reference = view_transform.map(reference);
        let scale = match marker.attr("markerUnits").unwrap_or("strokeWidth") {
            "strokeWidth" => context_style.stroke_style.width,
            "userSpaceOnUse" => 1.0,
            _ => return Err(VectorDecodeError::InvalidData),
        };
        let orient = marker.attr("orient").unwrap_or("0");
        let angle = if orient == "auto" || orient == "auto-start-reverse" {
            placement.angle
                + if placement.is_start && orient == "auto-start-reverse" {
                    std::f64::consts::PI
                } else {
                    0.0
                }
        } else {
            super::values::number(orient.trim_suffix("deg"))?.to_radians()
        };
        let (sin, cos) = angle.sin_cos();
        let placement_transform = Transform {
            e: -reference.x,
            f: -reference.y,
            ..Transform::IDENTITY
        }
        .then(Transform {
            a: scale,
            d: scale,
            ..Transform::IDENTITY
        })
        .then(Transform {
            a: cos,
            b: sin,
            c: -sin,
            d: cos,
            e: 0.0,
            f: 0.0,
        })
        .then(Transform {
            e: placement.point.x,
            f: placement.point.y,
            ..Transform::IDENTITY
        })
        .then(parent_transform);
        let transform = view_transform.then(placement_transform);
        let mut marker_parent = Style::default();
        marker_parent.context_fill.clone_from(&context_style.fill);
        marker_parent
            .context_stroke
            .clone_from(&context_style.stroke);
        marker_parent.context_paint_available = true;
        let marker_style = self.styles.compute(
            &self.document.elements,
            marker,
            &marker_parent,
            child_viewport,
        )?;
        let clip = (!marker_style.overflow_visible)
            .then(|| {
                self.viewport_clip(
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        width,
                        height,
                    },
                    placement_transform,
                )
            })
            .transpose()?;
        self.references.push(index);
        let draw = |this: &mut Self, context: &mut CanvasRenderingContext2d<'_>| {
            marker.children.iter().try_for_each(|&child| {
                this.render(
                    context,
                    child,
                    &marker_style,
                    transform,
                    child_viewport,
                    None,
                )
            })
        };
        let result = if marker_style.opacity < 1.0 || clip.is_some() {
            self.claim(1)?;
            self.scope(context, &marker_style, clip.as_slice(), None, draw)
        } else {
            draw(self, context)
        };
        self.references.pop();
        result
    }

    fn viewport_clip(
        &mut self,
        local_bounds: Rect,
        transform: Transform,
    ) -> Result<Clip, VectorDecodeError> {
        let path = rect_path(
            local_bounds.x,
            local_bounds.y,
            local_bounds.width,
            local_bounds.height,
            0.0,
            0.0,
        );
        self.claim(path.len())?;
        Ok(Clip {
            path,
            transform,
            rule: FillRule::NonZero,
        })
    }

    fn resolve_paint(
        &self,
        value: &PaintValue,
        opacity: f64,
        bounds: Rect,
        viewport: Size,
    ) -> Result<Option<Paint>, VectorDecodeError> {
        match value {
            PaintValue::None | PaintValue::ContextFill | PaintValue::ContextStroke => Ok(None),
            PaintValue::Color(color) => {
                let mut color = *color;
                color.alpha *= opacity;
                Ok(Some(Paint::Solid(color)))
            }
            PaintValue::Reference {
                reference,
                fallback,
            } => {
                let paint = local_reference(reference)
                    .ok()
                    .and_then(|reference| self.document.ids.get(reference).copied())
                    .filter(|index| {
                        matches!(
                            self.document.elements[*index].name,
                            "linearGradient" | "radialGradient"
                        )
                    })
                    .and_then(|index| {
                        self.gradient(index, bounds, opacity, viewport, &mut Vec::new())
                            .ok()
                    });
                if let Some(paint) = paint {
                    Ok(Some(paint))
                } else if let Some(fallback) = fallback {
                    self.resolve_paint(fallback, opacity, bounds, viewport)
                } else {
                    Ok(None)
                }
            }
        }
    }

    fn gradient(
        &self,
        index: usize,
        bounds: Rect,
        opacity: f64,
        viewport: Size,
        chain: &mut Vec<usize>,
    ) -> Result<Paint, VectorDecodeError> {
        let element = &self.document.elements[index];
        if !matches!(element.name, "linearGradient" | "radialGradient") {
            return Err(VectorDecodeError::InvalidData);
        }
        let kind = element.name;
        let mut current = index;
        loop {
            if chain.len() >= 64 {
                return Err(VectorDecodeError::ResourceLimit);
            }
            if chain.contains(&current) {
                return Err(VectorDecodeError::InvalidData);
            }
            chain.push(current);
            let current_element = &self.document.elements[current];
            let Some(reference) = current_element
                .attr("href")
                .or_else(|| current_element.attr_prefixed("xlink", "href"))
            else {
                break;
            };
            current = *self
                .document
                .ids
                .get(local_reference(reference)?)
                .ok_or(VectorDecodeError::InvalidData)?;
            if self.document.elements[current].name != kind {
                return Err(VectorDecodeError::InvalidData);
            }
        }
        let attr = |name: &str| {
            chain
                .iter()
                .find_map(|index| self.document.elements[*index].attr(name))
        };
        let mut stops = Vec::new();
        let mut last = 0.0;
        let stop_parent = chain
            .iter()
            .map(|index| &self.document.elements[*index])
            .find(|element| {
                element
                    .children
                    .iter()
                    .any(|child| self.document.elements[*child].name == "stop")
            })
            .ok_or(VectorDecodeError::InvalidData)?;
        let gradient_style = self.styles.compute(
            &self.document.elements,
            stop_parent,
            &Style::default(),
            viewport,
        )?;
        for &child in &stop_parent.children {
            let stop = &self.document.elements[child];
            if stop.name != "stop" {
                continue;
            }
            if stops.len() >= 65_536 {
                return Err(VectorDecodeError::ResourceLimit);
            }
            let offset = stop
                .attr("offset")
                .map(unit_number)
                .transpose()?
                .unwrap_or(0.0)
                .max(last);
            last = offset;
            let stop_style =
                self.styles
                    .compute(&self.document.elements, stop, &gradient_style, viewport)?;
            let mut color = stop_style.stop_color;
            color.color_space = stop_style.color_interpolation;
            color.alpha *= stop_style.stop_opacity * opacity;
            stops.push(crate::GradientStop { offset, color });
        }
        if stops.is_empty() {
            return Err(VectorDecodeError::InvalidData);
        }
        let object_units = match attr("gradientUnits").unwrap_or("objectBoundingBox") {
            "objectBoundingBox" => true,
            "userSpaceOnUse" => false,
            _ => return Err(VectorDecodeError::InvalidData),
        };
        let base = if object_units {
            Transform {
                a: bounds.width,
                b: 0.0,
                c: 0.0,
                d: bounds.height,
                e: bounds.x,
                f: bounds.y,
            }
        } else {
            Transform::IDENTITY
        };
        let transform = base.then(
            attr("gradientTransform")
                .map(parse_transform)
                .transpose()?
                .unwrap_or(Transform::IDENTITY),
        );
        if inverse_transform(transform).is_none() {
            return Err(VectorDecodeError::InvalidData);
        }
        let spread = match attr("spreadMethod").unwrap_or("pad") {
            "pad" => crate::SpreadMethod::Pad,
            "reflect" => crate::SpreadMethod::Reflect,
            "repeat" => crate::SpreadMethod::Repeat,
            _ => return Err(VectorDecodeError::InvalidData),
        };
        let x_axis = if object_units { 1.0 } else { viewport.width };
        let y_axis = if object_units { 1.0 } else { viewport.height };
        let radius_axis = if object_units {
            1.0
        } else {
            viewport.width.hypot(viewport.height) / std::f64::consts::SQRT_2
        };
        let coordinate =
            |name: &str, default: &str, axis: f64| length(attr(name).unwrap_or(default), axis);
        if element.name == "linearGradient" {
            let start = Point {
                x: coordinate("x1", "0%", x_axis)?,
                y: coordinate("y1", "0%", y_axis)?,
            };
            let end = Point {
                x: coordinate("x2", "100%", x_axis)?,
                y: coordinate("y2", "0%", y_axis)?,
            };
            Ok(Paint::LinearGradient {
                start,
                end,
                stops,
                spread,
                transform,
            })
        } else {
            let center = Point {
                x: coordinate("cx", "50%", x_axis)?,
                y: coordinate("cy", "50%", y_axis)?,
            };
            let focal = Point {
                x: attr("fx").map_or(Ok(center.x), |value| length(value, x_axis))?,
                y: attr("fy").map_or(Ok(center.y), |value| length(value, y_axis))?,
            };
            let radius = coordinate("r", "50%", radius_axis)?;
            Ok(Paint::RadialGradient {
                center,
                focal,
                radius,
                stops,
                spread,
                transform,
            })
        }
    }

    fn resolve_clip(
        &mut self,
        reference: &str,
        parent: Transform,
        viewport: Size,
    ) -> Result<Clip, VectorDecodeError> {
        let index = *self
            .document
            .ids
            .get(local_reference(reference)?)
            .ok_or(VectorDecodeError::InvalidData)?;
        let element = &self.document.elements[index];
        if element.name != "clipPath" {
            return Err(VectorDecodeError::InvalidData);
        }
        if element.attr("clipPathUnits") == Some("objectBoundingBox") {
            return Err(VectorDecodeError::UnsupportedFeature(
                "object bounding box clip paths",
            ));
        }
        let transform = element
            .attr("transform")
            .map(parse_transform)
            .transpose()?
            .unwrap_or(Transform::IDENTITY)
            .then(parent);
        let clip_style = self.styles.compute(
            &self.document.elements,
            element,
            &Style::default(),
            viewport,
        )?;
        let mut path = Vec::new();
        for &child in &element.children {
            let child = &self.document.elements[child];
            if skips_unsupported_subtree(child.name) {
                continue;
            }
            let child_transform = match child.attr("transform").map(parse_transform).transpose() {
                Ok(transform) => transform.unwrap_or(Transform::IDENTITY),
                Err(VectorDecodeError::UnsupportedFeature(_)) => continue,
                Err(error) => return Err(error),
            };
            let child_style =
                self.styles
                    .compute(&self.document.elements, child, &clip_style, viewport)?;
            let child_path = match child_style.geometry.path(child) {
                Ok(Some(path)) => path,
                Ok(None) | Err(VectorDecodeError::UnsupportedFeature(_)) => continue,
                Err(error) => return Err(error),
            };
            let mut child_path = child_path;
            for segment in &mut child_path {
                transform_segment(segment, child_transform);
            }
            path.extend(child_path);
        }
        if path.is_empty() {
            return Err(VectorDecodeError::UnsupportedFeature("clip path content"));
        }
        self.items = self
            .items
            .checked_add(path.len())
            .ok_or(VectorDecodeError::ResourceLimit)?;
        if self.items > MAX_ITEMS {
            return Err(VectorDecodeError::ResourceLimit);
        }
        Ok(Clip {
            path,
            transform,
            rule: match element.attr("clip-rule") {
                Some("evenodd") => FillRule::EvenOdd,
                _ => FillRule::NonZero,
            },
        })
    }

    fn resolve_mask(
        &mut self,
        reference: &str,
        parent: Transform,
        object_bounds: Rect,
        viewport: Size,
    ) -> Result<Mask, VectorDecodeError> {
        let index = *self
            .document
            .ids
            .get(local_reference(reference)?)
            .ok_or(VectorDecodeError::InvalidData)?;
        let element = &self.document.elements[index];
        if element.name != "mask" || self.references.contains(&index) {
            return Err(VectorDecodeError::InvalidData);
        }
        let object_units = match element.attr("maskUnits").unwrap_or("objectBoundingBox") {
            "objectBoundingBox" => true,
            "userSpaceOnUse" => false,
            _ => return Err(VectorDecodeError::InvalidData),
        };
        let content_transform = match element.attr("maskContentUnits").unwrap_or("userSpaceOnUse") {
            "userSpaceOnUse" => parent,
            "objectBoundingBox" => Transform {
                a: object_bounds.width,
                d: object_bounds.height,
                e: object_bounds.x,
                f: object_bounds.y,
                ..Transform::IDENTITY
            }
            .then(parent),
            _ => return Err(VectorDecodeError::InvalidData),
        };
        let coordinate = |name: &str, default: f64, axis: f64, origin: f64| {
            let value = element.attr(name);
            if object_units {
                value
                    .map(object_bbox_length)
                    .transpose()
                    .map(|value| origin + value.unwrap_or(default) * axis)
            } else {
                value
                    .map(|value| length(value, axis))
                    .transpose()
                    .map(|value| value.unwrap_or(default * axis))
            }
        };
        let x_axis = if object_units {
            object_bounds.width
        } else {
            viewport.width
        };
        let y_axis = if object_units {
            object_bounds.height
        } else {
            viewport.height
        };
        let x_origin = if object_units { object_bounds.x } else { 0.0 };
        let y_origin = if object_units { object_bounds.y } else { 0.0 };
        let x = coordinate("x", -0.1, x_axis, x_origin)?;
        let y = coordinate("y", -0.1, y_axis, y_origin)?;
        let width = coordinate("width", 1.2, x_axis, 0.0)?;
        let height = coordinate("height", 1.2, y_axis, 0.0)?;
        if width < 0.0 || height < 0.0 {
            return Err(VectorDecodeError::InvalidData);
        }
        let mask_style = self.styles.compute(
            &self.document.elements,
            element,
            &Style::default(),
            viewport,
        )?;
        let viewport = if element.attr("maskContentUnits") == Some("objectBoundingBox") {
            Size {
                width: 1.0,
                height: 1.0,
            }
        } else {
            viewport
        };
        Ok(Mask {
            index,
            region: Clip {
                path: rect_path(x, y, width, height, 0.0, 0.0),
                transform: parent,
                rule: FillRule::NonZero,
            },
            content_transform,
            viewport,
            style: mask_style,
            mode: match parse_mask_type(element)? {
                MaskType::Alpha => RenderMode::AlphaMask,
                MaskType::Luminance => RenderMode::LuminanceMask,
            },
        })
    }

    // Draws the content of a mask as its coverage.
    fn draw_mask(
        &mut self,
        context: &mut CanvasRenderingContext2d<'_>,
        mask: &Mask,
    ) -> Result<(), VectorDecodeError> {
        let mode = std::mem::replace(&mut self.mode, mask.mode);
        let drawn = self.drawn.len();
        self.references.push(mask.index);
        let result = self.document.elements[mask.index]
            .children
            .iter()
            .try_for_each(|&child| {
                self.render(
                    context,
                    child,
                    &mask.style,
                    mask.content_transform,
                    mask.viewport,
                    None,
                )
            });
        self.references.pop();
        // Mask content is not part of the bounds of the masked content.
        self.drawn.truncate(drawn);
        self.mode = mode;
        result
    }
}
