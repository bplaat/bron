/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

#[cfg(feature = "window")]
use bwindow::Window;

#[cfg(feature = "window")]
use crate::platforms::PlatformCanvas;
use crate::platforms::{PlatformCanvasContext, PlatformOffscreenCanvas};

/// A portable RGBA color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    /// Red component.
    pub red: u8,
    /// Green component.
    pub green: u8,
    /// Blue component.
    pub blue: u8,
    /// Alpha component.
    pub alpha: u8,
}

impl Color {
    /// Create an opaque color from a packed `0xRRGGBB` value.
    pub const fn from_rgb(rgb: u32) -> Self {
        Self::rgb(
            ((rgb >> 16) & 0xff) as u8,
            ((rgb >> 8) & 0xff) as u8,
            (rgb & 0xff) as u8,
        )
    }

    /// Create an opaque RGB color.
    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self {
            red,
            green,
            blue,
            alpha: 255,
        }
    }

    /// Create an RGBA color.
    pub const fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self {
            red,
            green,
            blue,
            alpha,
        }
    }

    pub(crate) fn components(self, global_alpha: f32) -> (f64, f64, f64, f64) {
        (
            f64::from(self.red) / 255.0,
            f64::from(self.green) / 255.0,
            f64::from(self.blue) / 255.0,
            f64::from(self.alpha) / 255.0 * f64::from(global_alpha.clamp(0.0, 1.0)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Color;

    #[test]
    fn saved_state_shares_font_storage_and_preserves_its_value() {
        let mut state = super::CanvasState::default();
        let saved = state.clone();
        assert!(super::Rc::ptr_eq(&state.font_family, &saved.font_family));
        state.font_family = "monospace".into();
        assert_eq!(saved.font_family.as_ref(), "sans-serif");
        assert_eq!(state.font_family.as_ref(), "monospace");
    }

    #[test]
    fn color_from_packed_rgb_uses_rrggbb_order() {
        assert_eq!(Color::from_rgb(0x12_34_56), Color::rgb(0x12, 0x34, 0x56));
        assert_eq!(Color::from_rgb(0), Color::rgba(0, 0, 0, 255));
    }

    #[test]
    fn offscreen_canvas_scales_drawing_and_preserves_scale_across_frames() {
        let mut canvas = super::OffscreenCanvas::with_scale_factor(4, 2, 2.625);
        assert_eq!((canvas.width(), canvas.height()), (11, 6));
        for _ in 0..2 {
            canvas.draw(|context| {
                assert_eq!((context.width(), context.height()), (4.0, 2.0));
                assert_eq!(context.scale_factor(), 2.625);
                context.clear_rect(0.0, 0.0, 4.0, 2.0);
                context.scale(3.0, 3.0);
                context.reset_transform();
                context.set_transform(1.0, 0.0, 0.0, 1.0, 0.0, 0.0);
                context.set_fill_style(Color::rgb(255, 0, 0));
                context.fill_rect(0.0, 0.0, 2.0, 2.0);
            });
            let pixels = canvas.pixels();
            // The interior spans five native pixels, not two logical pixels.
            assert_eq!(&pixels[4 * 4..5 * 4], &[255, 0, 0, 255]);
            assert_eq!(&pixels[7 * 4..8 * 4], &[0, 0, 0, 0]);
        }
    }

    #[test]
    fn offscreen_canvas_rejects_invalid_scale_factors() {
        for scale in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(
                std::panic::catch_unwind(|| {
                    super::OffscreenCanvas::with_scale_factor(4, 2, scale);
                })
                .is_err()
            );
        }
    }

    #[test]
    fn offscreen_canvas_returns_straight_rgba_pixels() {
        let mut canvas = super::OffscreenCanvas::new(2, 1);
        canvas.draw(|context| {
            context.set_fill_style(Color::rgba(120, 80, 40, 128));
            context.fill_rect(0.0, 0.0, 1.0, 1.0);
        });
        let pixels = canvas.pixels();
        assert!((i16::from(pixels[0]) - 120).abs() <= 2);
        assert!((i16::from(pixels[1]) - 80).abs() <= 2);
        assert!((i16::from(pixels[2]) - 40).abs() <= 2);
        assert_eq!(pixels[3], 128);
        assert_eq!(&pixels[4..8], &[0, 0, 0, 0]);
        let previous = pixels.to_vec();
        assert_eq!(canvas.pixels(), previous);
        canvas.draw(|context| {
            context.clear_rect(0.0, 0.0, 2.0, 1.0);
            context.set_fill_style(Color::rgb(1, 2, 3));
            context.fill_rect(1.0, 0.0, 1.0, 1.0);
        });
        assert_eq!(canvas.pixels(), &[0, 0, 0, 0, 1, 2, 3, 255]);
        #[cfg(feature = "image")]
        {
            let bitmap = canvas.to_bitmap().expect("valid bitmap");
            assert_eq!((bitmap.width(), bitmap.height()), (2, 1));
            assert_eq!(bitmap.data(), &[0, 0, 0, 0, 1, 2, 3, 255]);
        }
    }

    #[test]
    fn rounded_rectangles_preserve_winding_for_negative_dimensions() {
        for (width, height) in [(24.0, 24.0), (-24.0, 24.0), (24.0, -24.0), (-24.0, -24.0)] {
            let mut canvas = super::OffscreenCanvas::new(48, 48);
            canvas.draw(|ctx| {
                ctx.rect(2.0, 2.0, 44.0, 44.0);
                ctx.round_rect(
                    if width < 0.0 { 36.0 } else { 12.0 },
                    if height < 0.0 { 36.0 } else { 12.0 },
                    width,
                    height,
                    6.0,
                );
                ctx.fill();
            });
            let pixels = canvas.pixels();
            let alpha = |x: usize, y: usize| pixels[(y * 48 + x) * 4 + 3];
            assert_eq!(alpha(24, 24), if width * height < 0.0 { 0 } else { 255 });
            assert_eq!(alpha(4, 4), 255);
            assert_eq!(alpha(0, 0), 0);
        }
    }

    #[test]
    fn rounded_rectangle_clamps_radius_and_preserves_transform() {
        let mut canvas = super::OffscreenCanvas::new(48, 32);
        canvas.draw(|ctx| {
            ctx.translate(8.0, 4.0);
            ctx.round_rect(0.0, 0.0, 24.0, 24.0, 100.0);
            ctx.fill();
            ctx.fill_rect(32.0, 0.0, 4.0, 4.0);
        });
        let pixels = canvas.pixels();
        let alpha = |x: usize, y: usize| pixels[(y * 48 + x) * 4 + 3];
        assert_eq!(alpha(8, 4), 0);
        assert_eq!(alpha(20, 16), 255);
        assert_eq!(alpha(42, 6), 255);
        assert_eq!(alpha(34, 2), 0);
    }

    #[test]
    fn rounded_rectangle_leaves_current_point_at_its_origin() {
        let mut canvas = super::OffscreenCanvas::new(40, 40);
        canvas.draw(|ctx| {
            ctx.round_rect(20.0, 20.0, 12.0, 12.0, 6.0);
            ctx.line_to(20.0, 4.0);
            ctx.line_to(4.0, 4.0);
            ctx.close_path();
            ctx.fill();
        });
        assert_eq!(canvas.pixels()[(16 * 40 + 18) * 4 + 3], 255);
    }

    #[test]
    fn nonfinite_arguments_leave_path_and_transform_usable() {
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut canvas = super::OffscreenCanvas::new(8, 4);
            canvas.draw(|ctx| {
                ctx.rect(0.0, 0.0, 2.0, 2.0);
                ctx.move_to(invalid, 0.0);
                ctx.line_to(0.0, invalid);
                ctx.rect(0.0, 0.0, invalid, 4.0);
                ctx.round_rect(0.0, 0.0, 4.0, 4.0, invalid);
                ctx.round_rect(invalid, 0.0, 4.0, 4.0, -1.0);
                ctx.arc(0.0, 0.0, invalid, 0.0, 1.0, false);
                ctx.arc(invalid, 0.0, -1.0, 0.0, 1.0, false);
                ctx.ellipse(0.0, 0.0, invalid, 1.0);
                ctx.ellipse(invalid, 0.0, -1.0, 1.0);
                ctx.quadratic_curve_to(0.0, invalid, 1.0, 1.0);
                ctx.bezier_curve_to(0.0, 0.0, 1.0, invalid, 2.0, 2.0);
                ctx.translate(invalid, 0.0);
                ctx.rotate(invalid);
                ctx.scale(1.0, invalid);
                ctx.set_transform(1.0, 0.0, 0.0, 1.0, invalid, 0.0);
                ctx.fill();
                ctx.fill_rect(0.0, 0.0, invalid, 4.0);
                ctx.stroke_rect(invalid, 0.0, 4.0, 4.0);
                ctx.clear_rect(0.0, 0.0, 8.0, invalid);
                ctx.fill_text("ignored", invalid, 0.0);
                ctx.fill_rect(6.0, 2.0, 2.0, 2.0);
            });
            let pixels = canvas.pixels();
            for y in 0..4 {
                for x in 0..8 {
                    assert_eq!(
                        pixels[(y * 8 + x) * 4 + 3],
                        if (x < 2 && y < 2) || (x >= 6 && y >= 2) {
                            255
                        } else {
                            0
                        }
                    );
                }
            }
        }
    }

    fn alpha_at(canvas: &mut super::OffscreenCanvas, x: usize, y: usize) -> u8 {
        let width = canvas.width() as usize;
        canvas.pixels()[(y * width + x) * 4 + 3]
    }

    #[test]
    fn even_odd_fill_leaves_nested_areas_empty() {
        let mut canvas = super::OffscreenCanvas::new(12, 12);
        canvas.draw(|ctx| {
            ctx.rect(0.0, 0.0, 12.0, 12.0);
            ctx.rect(4.0, 4.0, 4.0, 4.0);
            ctx.fill_with(super::FillRule::EvenOdd);
        });
        assert_eq!(alpha_at(&mut canvas, 1, 1), 255);
        assert_eq!(alpha_at(&mut canvas, 6, 6), 0);
    }

    #[test]
    fn even_odd_clip_excludes_nested_areas() {
        let mut canvas = super::OffscreenCanvas::new(12, 12);
        canvas.draw(|ctx| {
            ctx.rect(0.0, 0.0, 12.0, 12.0);
            ctx.rect(4.0, 4.0, 4.0, 4.0);
            ctx.clip_with(super::FillRule::EvenOdd);
            ctx.begin_path();
            ctx.fill_rect(0.0, 0.0, 12.0, 12.0);
        });
        assert_eq!(alpha_at(&mut canvas, 1, 1), 255);
        assert_eq!(alpha_at(&mut canvas, 6, 6), 0);
    }

    #[test]
    fn linear_gradients_blend_between_stops_and_pad() {
        let mut canvas = super::OffscreenCanvas::new(40, 1);
        canvas.draw(|ctx| {
            let mut gradient = ctx.create_linear_gradient(10.0, 0.0, 30.0, 0.0);
            gradient.add_color_stop(0.0, Color::rgb(255, 0, 0));
            gradient.add_color_stop(1.0, Color::rgb(0, 0, 255));
            ctx.set_fill_style(gradient);
            ctx.fill_rect(0.0, 0.0, 40.0, 1.0);
        });
        let pixels = canvas.pixels();
        let pixel = |x: usize| &pixels[x * 4..][..4];
        assert_eq!(pixel(2), [255, 0, 0, 255]);
        assert_eq!(pixel(38), [0, 0, 255, 255]);
        let middle = pixel(20);
        assert!((100..156).contains(&middle[0]) && (100..156).contains(&middle[2]));
    }

    #[test]
    fn repeated_and_reflected_gradients_continue_past_their_stops() {
        for (spread, expected) in [
            (super::GradientSpread::Repeat, [255, 0, 0, 255]),
            (super::GradientSpread::Reflect, [0, 0, 255, 255]),
        ] {
            let mut canvas = super::OffscreenCanvas::new(40, 1);
            canvas.draw(|ctx| {
                let mut gradient = super::CanvasGradient::linear(0.0, 0.0, 10.0, 0.0);
                gradient.add_color_stop(0.0, Color::rgb(255, 0, 0));
                gradient.add_color_stop(1.0, Color::rgb(0, 0, 255));
                gradient.set_spread(spread);
                ctx.set_fill_style(gradient);
                ctx.fill_rect(0.0, 0.0, 40.0, 1.0);
            });
            // Just past the start of the second period.
            let pixel = &canvas.pixels()[10 * 4..][..4];
            for (actual, expected) in pixel.iter().zip(expected) {
                assert!(
                    (i16::from(*actual) - expected).abs() <= 40,
                    "{spread:?}: {pixel:?}"
                );
            }
        }
    }

    #[test]
    fn gradients_follow_their_transform() {
        let mut canvas = super::OffscreenCanvas::new(1, 20);
        canvas.draw(|ctx| {
            // A horizontal gradient rotated to run from top to bottom.
            let mut gradient = super::CanvasGradient::linear(0.0, 0.0, 20.0, 0.0);
            gradient.add_color_stop(0.0, Color::rgb(255, 0, 0));
            gradient.add_color_stop(1.0, Color::rgb(0, 0, 255));
            gradient.set_transform(0.0, 1.0, -1.0, 0.0, 0.0, 0.0);
            ctx.set_fill_style(gradient);
            ctx.fill_rect(0.0, 0.0, 1.0, 20.0);
        });
        let pixels = canvas.pixels();
        assert!(pixels[0] > 200 && pixels[2] < 55);
        assert!(pixels[19 * 4] < 55 && pixels[19 * 4 + 2] > 200);
    }

    #[test]
    fn radial_gradients_spread_from_the_focal_point() {
        let mut canvas = super::OffscreenCanvas::new(21, 21);
        canvas.draw(|ctx| {
            let mut gradient = ctx.create_radial_gradient(10.0, 10.0, 10.0, 10.0, 10.0);
            gradient.add_color_stop(0.0, Color::rgb(255, 255, 255));
            gradient.add_color_stop(1.0, Color::rgba(255, 255, 255, 0));
            ctx.set_fill_style(gradient);
            ctx.fill_rect(0.0, 0.0, 21.0, 21.0);
        });
        assert!(alpha_at(&mut canvas, 10, 10) > 230);
        assert!(alpha_at(&mut canvas, 15, 10) > 90 && alpha_at(&mut canvas, 15, 10) < 165);
        assert_eq!(alpha_at(&mut canvas, 0, 0), 0);
    }

    #[test]
    fn gradient_strokes_paint_the_outline() {
        let mut canvas = super::OffscreenCanvas::new(20, 20);
        canvas.draw(|ctx| {
            let mut gradient = super::CanvasGradient::linear(0.0, 0.0, 20.0, 0.0);
            gradient.add_color_stop(0.0, Color::rgb(0, 255, 0));
            gradient.add_color_stop(1.0, Color::rgb(0, 255, 0));
            ctx.set_stroke_style(gradient);
            ctx.set_line_width(4.0);
            ctx.stroke_rect(4.0, 4.0, 12.0, 12.0);
        });
        assert_eq!(&canvas.pixels()[(4 * 20 + 10) * 4..][..4], [0, 255, 0, 255]);
        assert_eq!(alpha_at(&mut canvas, 10, 10), 0);
    }

    #[test]
    fn dashed_lines_leave_gaps() {
        let mut canvas = super::OffscreenCanvas::new(20, 4);
        canvas.draw(|ctx| {
            ctx.set_line_dash(&[4.0]);
            ctx.set_line_width(2.0);
            ctx.move_to(0.0, 2.0);
            ctx.line_to(20.0, 2.0);
            ctx.stroke();
        });
        assert_eq!(alpha_at(&mut canvas, 1, 2), 255);
        assert_eq!(alpha_at(&mut canvas, 6, 2), 0);
        assert_eq!(alpha_at(&mut canvas, 9, 2), 255);
    }

    #[test]
    fn layers_composite_overlapping_drawing_once() {
        let mut canvas = super::OffscreenCanvas::new(3, 1);
        canvas.draw(|ctx| {
            ctx.set_global_alpha(0.5);
            ctx.begin_layer();
            assert_eq!(ctx.global_alpha(), 1.0);
            ctx.fill_rect(0.0, 0.0, 2.0, 1.0);
            ctx.fill_rect(1.0, 0.0, 2.0, 1.0);
            ctx.end_layer();
            assert_eq!(ctx.global_alpha(), 0.5);
        });
        for x in 0..3 {
            assert!((i16::from(alpha_at(&mut canvas, x, 0)) - 128).abs() <= 1);
        }
    }

    #[test]
    fn mask_layers_show_content_through_the_mask_alpha() {
        let mut canvas = super::OffscreenCanvas::new(4, 1);
        canvas.draw(|ctx| {
            ctx.begin_mask_layer();
            ctx.fill_rect(0.0, 0.0, 1.0, 1.0);
            ctx.set_fill_style(Color::rgba(0, 0, 0, 128));
            ctx.fill_rect(1.0, 0.0, 1.0, 1.0);
            ctx.end_mask();
            ctx.set_fill_style(Color::rgb(255, 0, 0));
            ctx.fill_rect(0.0, 0.0, 3.0, 1.0);
            ctx.end_layer();
            ctx.fill_rect(3.0, 0.0, 1.0, 1.0);
        });
        let pixels = canvas.pixels();
        assert_eq!(&pixels[..4], [255, 0, 0, 255]);
        assert!((i16::from(pixels[7]) - 128).abs() <= 1);
        assert_eq!(pixels[11], 0);
        // The fill style of the mask does not leak out of the layer.
        assert_eq!(&pixels[12..16], [0, 0, 0, 255]);
    }

    #[test]
    fn unbalanced_layers_are_closed_with_the_frame() {
        let mut canvas = super::OffscreenCanvas::new(2, 1);
        canvas.draw(|ctx| {
            ctx.end_layer();
            ctx.begin_mask_layer();
            ctx.save();
            ctx.fill_rect(0.0, 0.0, 1.0, 1.0);
            ctx.restore();
            ctx.restore();
            ctx.end_mask();
            ctx.begin_layer();
            ctx.fill_rect(0.0, 0.0, 2.0, 1.0);
        });
        assert_eq!(alpha_at(&mut canvas, 0, 0), 255);
        assert_eq!(alpha_at(&mut canvas, 1, 0), 0);
    }

    #[test]
    fn images_are_drawn_upright_and_scaled() {
        // Red on top of blue, as premultiplied BGRA.
        let pixels: std::sync::Arc<[u8]> = [0, 0, 255, 255, 255, 0, 0, 255].into();
        let image = super::CanvasImage::from_premultiplied_bgra(1, 2, pixels);
        assert_eq!((image.width(), image.height()), (1, 2));
        let mut canvas = super::OffscreenCanvas::new(4, 8);
        canvas.draw(|ctx| {
            ctx.set_image_smoothing_enabled(false);
            ctx.draw_image(&image, 0.0, 0.0, 4.0, 8.0);
        });
        let pixels = canvas.pixels();
        assert_eq!(&pixels[(4 + 1) * 4..][..4], [255, 0, 0, 255]);
        assert_eq!(&pixels[(6 * 4 + 2) * 4..][..4], [0, 0, 255, 255]);
    }

    #[test]
    fn text_is_painted_with_gradients() {
        let mut canvas = super::OffscreenCanvas::new(60, 30);
        canvas.draw(|ctx| {
            let mut gradient = super::CanvasGradient::linear(0.0, 0.0, 60.0, 0.0);
            gradient.add_color_stop(0.0, Color::rgb(0, 0, 255));
            gradient.add_color_stop(1.0, Color::rgb(0, 0, 255));
            ctx.set_fill_style(gradient);
            ctx.set_font("sans-serif", 24.0);
            ctx.fill_text("WW", 4.0, 24.0);
        });
        let pixels = canvas.pixels();
        let ink = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[3] > 200);
        assert!(ink.clone().count() > 20);
        assert!(ink.into_iter().all(|pixel| pixel[2] > 200 && pixel[0] < 40));
    }

    #[test]
    fn composite_operations_blend_with_the_content() {
        let mut canvas = super::OffscreenCanvas::new(1, 1);
        canvas.draw(|ctx| {
            ctx.set_fill_style(Color::rgb(255, 128, 0));
            ctx.fill_rect(0.0, 0.0, 1.0, 1.0);
            ctx.set_global_composite_operation(super::CompositeOperation::Multiply);
            ctx.set_fill_style(Color::rgb(128, 255, 255));
            ctx.fill_rect(0.0, 0.0, 1.0, 1.0);
        });
        let pixel = &canvas.pixels()[..4];
        assert!((i16::from(pixel[0]) - 128).abs() <= 2 && (i16::from(pixel[1]) - 128).abs() <= 2);
        assert_eq!(pixel[2], 0);
    }

    // Draws a red background and blue over its left half with an operation.
    fn composite_pixels(
        operation: super::CompositeOperation,
        draw: fn(&mut super::CanvasRenderingContext2d<'_>),
    ) -> Vec<u8> {
        let mut canvas = super::OffscreenCanvas::new(4, 4);
        canvas.draw(|ctx| {
            ctx.set_fill_style(Color::rgb(255, 0, 0));
            ctx.fill_rect(0.0, 0.0, 4.0, 4.0);
            ctx.set_global_composite_operation(operation);
            ctx.set_fill_style(Color::rgb(0, 0, 255));
            ctx.set_stroke_style(Color::rgb(0, 0, 255));
            draw(ctx);
        });
        canvas.pixels().to_vec()
    }

    #[test]
    fn composite_operations_only_change_the_drawn_area() {
        let draws: [fn(&mut super::CanvasRenderingContext2d<'_>); 3] = [
            |ctx| ctx.fill_rect(0.0, 0.0, 2.0, 4.0),
            |ctx| {
                ctx.rect(0.0, 0.0, 2.0, 4.0);
                ctx.fill();
            },
            |ctx| {
                ctx.set_line_width(2.0);
                ctx.move_to(1.0, 0.0);
                ctx.line_to(1.0, 4.0);
                ctx.stroke();
            },
        ];
        for draw in draws {
            for (operation, expected) in [
                (super::CompositeOperation::SourceIn, [0, 0, 255, 255]),
                (super::CompositeOperation::DestinationIn, [255, 0, 0, 255]),
                (super::CompositeOperation::SourceOut, [0, 0, 0, 0]),
                (super::CompositeOperation::DestinationOut, [0, 0, 0, 0]),
                (super::CompositeOperation::Copy, [0, 0, 255, 255]),
            ] {
                let pixels = composite_pixels(operation, draw);
                let pixel = |x: usize| &pixels[(4 + x) * 4..][..4];
                assert_eq!(pixel(0), expected, "{operation:?}");
                assert_eq!(pixel(3), [255, 0, 0, 255], "{operation:?}");
            }
        }
    }

    #[test]
    fn nested_layers_blend_with_their_own_content() {
        let mut canvas = super::OffscreenCanvas::new(3, 1);
        canvas.draw(|ctx| {
            ctx.set_fill_style(Color::rgb(255, 0, 0));
            ctx.fill_rect(0.0, 0.0, 3.0, 1.0);
            ctx.begin_layer();
            ctx.begin_layer();
            ctx.set_fill_style(Color::rgb(0, 255, 0));
            ctx.fill_rect(0.0, 0.0, 2.0, 1.0);
            ctx.set_global_composite_operation(super::CompositeOperation::Screen);
            ctx.set_fill_style(Color::rgb(0, 0, 255));
            ctx.fill_rect(1.0, 0.0, 2.0, 1.0);
            ctx.end_layer();
            ctx.end_layer();
        });
        assert_eq!(
            canvas.pixels(),
            [0, 255, 0, 255, 0, 255, 255, 255, 0, 0, 255, 255]
        );
    }

    #[test]
    fn layers_composite_over_the_whole_canvas() {
        let pixels = composite_pixels(super::CompositeOperation::SourceIn, |ctx| {
            ctx.begin_layer();
            ctx.fill_rect(0.0, 0.0, 2.0, 4.0);
            ctx.end_layer();
        });
        assert_eq!(&pixels[4 * 4..][..4], [0, 0, 255, 255]);
        assert_eq!(pixels[7 * 4 + 3], 0);
    }

    #[test]
    fn blend_mode_layers_mix_with_the_content() {
        let pixels = composite_pixels(super::CompositeOperation::Screen, |ctx| {
            ctx.begin_layer();
            ctx.fill_rect(0.0, 0.0, 2.0, 4.0);
            ctx.end_layer();
        });
        assert_eq!(&pixels[4 * 4..][..4], [255, 0, 255, 255]);
        assert_eq!(&pixels[7 * 4..][..4], [255, 0, 0, 255]);
    }

    #[test]
    fn gradient_stops_keep_their_order_and_reflect_in_odd_periods() {
        let mut gradient = super::CanvasGradient::linear(0.0, 0.0, 1.0, 0.0);
        gradient.add_color_stop(0.5, Color::rgb(0, 0, 255));
        gradient.add_color_stop(0.0, Color::rgb(255, 0, 0));
        gradient.add_color_stop(0.5, Color::rgb(0, 255, 0));
        gradient.add_color_stop(f32::NAN, Color::rgb(1, 1, 1));
        let offsets: Vec<f32> = gradient.0.stops.iter().map(|(offset, _)| *offset).collect();
        assert_eq!(offsets, [0.0, 0.5, 0.5]);
        assert_eq!(gradient.0.stops[2].1, Color::rgb(0, 255, 0));
        gradient.set_spread(super::GradientSpread::Reflect);
        let stops = gradient.0.spread_stops(0, 2);
        assert_eq!(stops.len(), gradient.0.stops.len() * 2);
        // The second period is mirrored.
        assert_eq!(stops[3], (0.75, Color::rgb(0, 255, 0)));
        assert_eq!(stops[5], (1.0, Color::rgb(255, 0, 0)));
    }

    #[test]
    #[should_panic(expected = "rounded rectangle radius must be nonnegative")]
    fn rounded_rectangle_rejects_negative_radius() {
        super::OffscreenCanvas::new(4, 4).draw(|ctx| ctx.round_rect(0.0, 0.0, 4.0, 4.0, -1.0));
    }

    #[test]
    #[should_panic(expected = "arc radius must be nonnegative")]
    fn arc_rejects_negative_radius() {
        super::OffscreenCanvas::new(4, 4).draw(|ctx| ctx.arc(0.0, 0.0, -1.0, 0.0, 1.0, false));
    }

    #[test]
    #[should_panic(expected = "ellipse radii must be nonnegative")]
    fn ellipse_rejects_negative_horizontal_radius() {
        super::OffscreenCanvas::new(4, 4).draw(|ctx| ctx.ellipse(0.0, 0.0, -1.0, 1.0));
    }

    #[test]
    #[should_panic(expected = "ellipse radii must be nonnegative")]
    fn ellipse_rejects_negative_vertical_radius() {
        super::OffscreenCanvas::new(4, 4).draw(|ctx| ctx.ellipse(0.0, 0.0, 1.0, -1.0));
    }
}

/// How a gradient continues beyond its first and last color stops.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GradientSpread {
    /// Extend the first and last stop colors, like HTML canvas.
    #[default]
    Pad,
    /// Mirror the gradient in every following period.
    Reflect,
    /// Repeat the gradient in every following period.
    Repeat,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum GradientShape {
    Linear {
        start: (f32, f32),
        end: (f32, f32),
    },
    // A focal circle of radius zero inside an end circle, like SVG radial gradients.
    Radial {
        focal: (f32, f32),
        center: (f32, f32),
        radius: f32,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Gradient {
    pub(crate) shape: GradientShape,
    pub(crate) stops: Vec<(f32, Color)>,
    pub(crate) spread: GradientSpread,
    pub(crate) transform: [f32; 6],
}

/// A linear or radial gradient fill or stroke style, like the HTML `CanvasGradient`.
#[derive(Debug, Clone, PartialEq)]
pub struct CanvasGradient(pub(crate) Rc<Gradient>);

impl CanvasGradient {
    fn new(shape: GradientShape) -> Self {
        Self(Rc::new(Gradient {
            shape,
            stops: Vec::new(),
            spread: GradientSpread::Pad,
            transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        }))
    }

    /// Create a linear gradient along the line from the start to the end point.
    pub fn linear(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        Self::new(GradientShape::Linear {
            start: (x0, y0),
            end: (x1, y1),
        })
    }

    /// Create a radial gradient from a focal point to the circle around the center.
    pub fn radial(fx: f32, fy: f32, cx: f32, cy: f32, radius: f32) -> Self {
        Self::new(GradientShape::Radial {
            focal: (fx, fy),
            center: (cx, cy),
            radius: radius.max(0.0),
        })
    }

    /// Add a color stop. Offsets are clamped to 0.0..=1.0 and stops with the same offset keep
    /// their order, which makes hard color transitions. Non-finite offsets are ignored.
    pub fn add_color_stop(&mut self, offset: f32, color: Color) {
        if !offset.is_finite() {
            return;
        }
        let offset = offset.clamp(0.0, 1.0);
        let stops = &mut Rc::make_mut(&mut self.0).stops;
        let index = stops.partition_point(|(stop, _)| *stop <= offset);
        stops.insert(index, (offset, color));
    }

    /// Set how the gradient continues beyond its first and last stops.
    pub fn set_spread(&mut self, spread: GradientSpread) {
        Rc::make_mut(&mut self.0).spread = spread;
    }

    /// Set the transform from gradient space to the user space of the painted shape.
    /// Non-finite values are ignored.
    pub fn set_transform(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) {
        if [a, b, c, d, e, f].iter().all(|value| value.is_finite()) {
            Rc::make_mut(&mut self.0).transform = [a, b, c, d, e, f];
        }
    }
}

impl Gradient {
    /// The stops covering the periods `first..last` of a reflected or repeated gradient as
    /// one padded gradient, for graphics APIs without native spread modes.
    #[cfg_attr(not(target_vendor = "apple"), allow(dead_code))]
    pub(crate) fn spread_stops(&self, first: i32, last: i32) -> Vec<(f32, Color)> {
        let periods = (last - first).max(1);
        let mut stops = Vec::with_capacity(self.stops.len() * periods as usize);
        for period in first..first + periods {
            let reversed = self.spread == GradientSpread::Reflect && period.rem_euclid(2) == 1;
            let base = (period - first) as f32;
            let mut add = |offset: f32, color| {
                stops.push(((base + offset) / periods as f32, color));
            };
            if reversed {
                self.stops
                    .iter()
                    .rev()
                    .for_each(|&(offset, color)| add(1.0 - offset, color));
            } else {
                self.stops
                    .iter()
                    .for_each(|&(offset, color)| add(offset, color));
            }
        }
        stops
    }
}

/// A fill or stroke style.
#[derive(Debug, Clone, PartialEq)]
pub enum CanvasStyle {
    /// A uniform color.
    Color(Color),
    /// A linear or radial gradient.
    Gradient(CanvasGradient),
}

impl From<Color> for CanvasStyle {
    fn from(color: Color) -> Self {
        Self::Color(color)
    }
}

impl From<CanvasGradient> for CanvasStyle {
    fn from(gradient: CanvasGradient) -> Self {
        Self::Gradient(gradient)
    }
}

/// The rule that decides which areas a path encloses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FillRule {
    /// Areas with a nonzero winding number are inside.
    #[default]
    NonZero,
    /// Areas crossed an odd number of times are inside.
    EvenOdd,
}

/// How drawing is composited onto the existing content, like the HTML
/// `globalCompositeOperation`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CompositeOperation {
    /// Draw over the existing content.
    #[default]
    SourceOver,
    /// Keep the drawing only where there is existing content.
    SourceIn,
    /// Keep the drawing only where there is no existing content.
    SourceOut,
    /// Draw only over existing content, keeping the content elsewhere.
    SourceAtop,
    /// Draw behind the existing content.
    DestinationOver,
    /// Keep existing content only where the drawing is.
    DestinationIn,
    /// Keep existing content only where the drawing is not.
    DestinationOut,
    /// Keep existing content inside the drawing and draw the rest behind it.
    DestinationAtop,
    /// Add the colors.
    Lighter,
    /// Replace the existing content with the drawing.
    Copy,
    /// Keep the drawing and the content only where they do not overlap.
    Xor,
    /// Multiply the colors.
    Multiply,
    /// Invert, multiply and invert the colors.
    Screen,
    /// Multiply or screen depending on the existing color.
    Overlay,
    /// Keep the darker color.
    Darken,
    /// Keep the lighter color.
    Lighten,
    /// Brighten the existing color.
    ColorDodge,
    /// Darken the existing color.
    ColorBurn,
    /// Multiply or screen depending on the drawn color.
    HardLight,
    /// A softer hard light.
    SoftLight,
    /// Subtract the darker color from the lighter one.
    Difference,
    /// A lower contrast difference.
    Exclusion,
    /// The hue of the drawing with the saturation and luminosity of the content.
    Hue,
    /// The saturation of the drawing with the hue and luminosity of the content.
    Saturation,
    /// The hue and saturation of the drawing with the luminosity of the content.
    Color,
    /// The luminosity of the drawing with the hue and saturation of the content.
    Luminosity,
}

/// A premultiplied BGRA8 bitmap that canvases draw with `draw_image`, which shares its pixels.
#[derive(Clone)]
pub struct CanvasImage(pub(crate) Rc<crate::platforms::PlatformImage>);

impl CanvasImage {
    /// Create an image from tightly packed, row-major, premultiplied BGRA8 pixels.
    ///
    /// # Panics
    /// Panics if the dimensions are zero or do not match the pixel data.
    pub fn from_premultiplied_bgra(width: u32, height: u32, pixels: std::sync::Arc<[u8]>) -> Self {
        assert!(width > 0 && height > 0, "image dimensions must be nonzero");
        assert_eq!(
            Some(pixels.len()),
            (width as usize)
                .checked_mul(height as usize)
                .and_then(|pixels| pixels.checked_mul(4)),
            "image pixels must match the dimensions"
        );
        Self(Rc::new(crate::platforms::PlatformImage::new(
            width, height, pixels,
        )))
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.0.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.0.height
    }
}

/// Canvas line cap style.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LineCap {
    /// Flat line ends.
    #[default]
    Butt,
    /// Round line ends.
    Round,
    /// Square line ends.
    Square,
}

/// Canvas line join style.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LineJoin {
    /// Mitered joins.
    #[default]
    Miter,
    /// Rounded joins.
    Round,
    /// Beveled joins.
    Bevel,
}

/// Horizontal text alignment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextAlign {
    /// Align at the text start.
    #[default]
    Start,
    /// Align at the text end.
    End,
    /// Align left.
    Left,
    /// Align right.
    Right,
    /// Center text.
    Center,
}

/// Vertical text alignment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextBaseline {
    /// Top edge.
    Top,
    /// Hanging baseline.
    Hanging,
    /// Middle.
    Middle,
    /// Alphabetic baseline.
    #[default]
    Alphabetic,
    /// Ideographic baseline.
    Ideographic,
    /// Bottom edge.
    Bottom,
}

/// Canvas font weight.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum FontWeight {
    /// Normal font weight.
    #[default]
    Normal,
    /// Bold font weight.
    Bold,
}

#[derive(Clone)]
pub(crate) struct CanvasState {
    pub fill: CanvasStyle,
    pub stroke: CanvasStyle,
    pub alpha: f32,
    pub composite: CompositeOperation,
    pub image_smoothing: bool,
    pub line_width: f32,
    pub line_cap: LineCap,
    pub line_join: LineJoin,
    pub miter_limit: f32,
    pub line_dash: Rc<[f32]>,
    pub line_dash_offset: f32,
    pub font_family: Rc<str>,
    pub font_size: f32,
    pub font_weight: FontWeight,
    pub text_align: TextAlign,
    pub text_baseline: TextBaseline,
}

impl Default for CanvasState {
    fn default() -> Self {
        Self {
            fill: Color::rgb(0, 0, 0).into(),
            stroke: Color::rgb(0, 0, 0).into(),
            alpha: 1.0,
            composite: CompositeOperation::SourceOver,
            image_smoothing: true,
            line_width: 1.0,
            line_cap: LineCap::Butt,
            line_join: LineJoin::Miter,
            miter_limit: 10.0,
            line_dash: Rc::from([]),
            line_dash_offset: 0.0,
            font_family: "sans-serif".into(),
            font_size: 10.0,
            font_weight: FontWeight::Normal,
            text_align: TextAlign::Start,
            text_baseline: TextBaseline::Alphabetic,
        }
    }
}

/// Builder for one native 2D canvas attached to a window.
#[cfg(feature = "window")]
pub struct CanvasBuilder<'a> {
    window: &'a Window,
}

#[cfg(feature = "window")]
impl<'a> CanvasBuilder<'a> {
    /// Create a canvas builder.
    pub const fn new(window: &'a Window) -> Self {
        Self { window }
    }

    /// Build the canvas, panicking if the window is closed or already has content.
    pub fn build(self) -> Canvas {
        self.try_build().expect("cannot attach canvas")
    }

    /// Build the canvas with a recoverable attachment error.
    pub fn try_build(self) -> Result<Canvas, bwindow::AttachError> {
        Ok(Canvas {
            platform: PlatformCanvas::new(self.window.attach_content()?),
        })
    }
}

/// A native 2D canvas. All access is confined to the main thread.
#[cfg(feature = "window")]
pub struct Canvas {
    platform: PlatformCanvas,
}

#[cfg(feature = "window")]
impl Canvas {
    /// Schedule a repaint. Repeated requests coalesce.
    pub fn request_redraw(&self) {
        self.platform.request_redraw();
    }

    /// Schedule one repaint at the next native display frame.
    pub fn request_animation_frame(&self) {
        self.platform.request_animation_frame();
    }

    /// Paint once during this canvas window's RedrawRequested callback.
    ///
    /// Returns false outside a native paint callback, after close, or if this frame
    /// was already drawn. The closure is not called in those cases. Native frame
    /// scheduling and recovery are handled by the library; callers must not assume
    /// that every redraw produced a completed draw. The context cannot escape the closure.
    ///
    /// Ignoring whether a frame was drawn is diagnosed by Rust:
    ///
    /// ```compile_fail
    /// #![deny(unused_must_use)]
    /// fn paint(canvas: &mut bcanvas::Canvas) {
    ///     canvas.draw(|_| {});
    /// }
    /// ```
    #[must_use = "only update frame-dependent state when the draw closure ran"]
    pub fn draw(
        &mut self,
        draw: impl for<'frame> FnOnce(&mut CanvasRenderingContext2d<'frame>),
    ) -> bool {
        self.platform.draw(draw)
    }
}

/// A thread-bound native 2D canvas backed by tightly packed, straight-alpha RGBA8 pixels.
pub struct OffscreenCanvas {
    platform: PlatformOffscreenCanvas,
    width: u32,
    height: u32,
    scale_factor: f32,
    logical_size: (u32, u32),
}

impl OffscreenCanvas {
    /// Create a transparent offscreen canvas with one logical pixel per output pixel.
    pub fn new(width: u32, height: u32) -> Self {
        Self::with_scale_factor(width, height, 1.0)
    }

    /// Create a canvas with logical dimensions and a native pixel scale.
    /// Bitmap dimensions are rounded up; drawing coordinates remain logical pixels.
    pub fn with_scale_factor(width: u32, height: u32, scale_factor: f32) -> Self {
        assert!(
            scale_factor.is_finite() && scale_factor > 0.0,
            "invalid canvas scale factor"
        );
        let logical_size = (width, height);
        let width = (f64::from(width) * f64::from(scale_factor)).ceil();
        let height = (f64::from(height) * f64::from(scale_factor)).ceil();
        assert!(
            width <= f64::from(i32::MAX) && height <= f64::from(i32::MAX),
            "canvas dimensions are too large"
        );
        let (width, height) = (width as u32, height as u32);
        assert!(
            width > 0 && height > 0 && width <= i32::MAX as u32 && height <= i32::MAX as u32,
            "canvas dimensions must be nonzero and fit in i32"
        );
        let length = width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(4))
            .and_then(|bytes| usize::try_from(bytes).ok())
            .expect("canvas dimensions are too large");
        Self {
            platform: PlatformOffscreenCanvas::new(width, height, length),
            width,
            height,
            scale_factor,
            logical_size,
        }
    }

    /// Native pixels per logical pixel.
    pub const fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    /// Width in pixels.
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Number of bytes between adjacent rows.
    pub const fn bytes_per_row(&self) -> u32 {
        self.width * 4
    }

    /// Draw into the bitmap with the same API as a window canvas.
    pub fn draw(&mut self, draw: impl for<'frame> FnOnce(&mut CanvasRenderingContext2d<'frame>)) {
        let platform = self.platform.begin_draw(self.scale_factor);
        let mut context = CanvasRenderingContext2d::new(
            platform,
            self.logical_size.0 as f32,
            self.logical_size.1 as f32,
            self.scale_factor,
        );
        draw(&mut context);
        drop(context);
        self.platform.end_draw();
    }

    /// Return tightly packed pixels in RGBA byte order with straight alpha.
    pub fn pixels(&mut self) -> &[u8] {
        self.platform.pixels()
    }

    /// Copy the current pixels into an image bitmap for encoding or further processing.
    #[cfg(feature = "image")]
    pub fn to_bitmap(&mut self) -> Result<image::Bitmap, image::EncodeError> {
        image::Bitmap::new(self.width, self.height, self.pixels().to_vec())
    }
}

/// A native drawing context owned by the caller, like the one of a native view drawing itself.
#[cfg(any(target_vendor = "apple", not(any(target_os = "android", windows))))]
#[derive(Clone, Copy, Debug)]
pub enum NativeContext {
    /// A Core Graphics `CGContextRef` with a top left origin, like a flipped NSView's.
    #[cfg(target_vendor = "apple")]
    CoreGraphics(*mut std::ffi::c_void),
    /// A `cairo_t`, like the one of a GTK draw signal.
    #[cfg(not(target_vendor = "apple"))]
    Cairo(*mut std::ffi::c_void),
}

/// Draw into a native context with logical coordinates, starting from its current transform
/// and clip. The context keeps its state after drawing.
///
/// # Safety
/// The native context must be valid for drawing during this call.
#[cfg(any(target_vendor = "apple", not(any(target_os = "android", windows))))]
pub unsafe fn draw_native(
    context: NativeContext,
    width: f32,
    height: f32,
    scale_factor: f32,
    draw: impl for<'frame> FnOnce(&mut CanvasRenderingContext2d<'frame>),
) {
    let platform = unsafe { PlatformCanvasContext::from_native(context) };
    let mut context = CanvasRenderingContext2d::new(platform, width, height, scale_factor);
    draw(&mut context);
}

/// A canvas painting a native Win32 window, like a scroll view, with a Direct2D render target
/// that follows the size and DPI of its client area.
#[cfg(windows)]
pub struct HwndCanvas(crate::platforms::PlatformHwndCanvas);

#[cfg(windows)]
impl HwndCanvas {
    /// Create a canvas for a window.
    ///
    /// # Safety
    /// The window must stay valid while the canvas is alive.
    pub unsafe fn new(hwnd: *mut std::ffi::c_void) -> Self {
        Self(crate::platforms::PlatformHwndCanvas::new(hwnd))
    }

    /// Paint the whole client area in logical pixels, call this for the `WM_PAINT` of the
    /// window, which it validates.
    pub fn paint(&mut self, draw: impl for<'frame> FnOnce(&mut CanvasRenderingContext2d<'frame>)) {
        self.0.paint(draw);
    }
}

#[cfg(not(any(target_vendor = "apple", target_os = "android")))]
pub(crate) fn copy_bgra_premultiplied_to_rgba(source: &[u8], output: &mut [u8]) {
    let (source, source_remainder) = source.as_chunks::<4>();
    let (output, output_remainder) = output.as_chunks_mut::<4>();
    debug_assert!(source_remainder.is_empty() && output_remainder.is_empty());
    for (source, output) in source.iter().zip(output) {
        let alpha = source[3];
        output[0] = unpremultiply(source[2], alpha);
        output[1] = unpremultiply(source[1], alpha);
        output[2] = unpremultiply(source[0], alpha);
        output[3] = alpha;
    }
}

#[cfg(any(target_vendor = "apple", target_os = "android"))]
pub(crate) fn copy_rgba_premultiplied_to_straight(source: &[u8], output: &mut [u8]) {
    let (source, source_remainder) = source.as_chunks::<4>();
    let (output, output_remainder) = output.as_chunks_mut::<4>();
    debug_assert!(source_remainder.is_empty() && output_remainder.is_empty());
    for (source, output) in source.iter().zip(output) {
        let alpha = source[3];
        output[0] = unpremultiply(source[0], alpha);
        output[1] = unpremultiply(source[1], alpha);
        output[2] = unpremultiply(source[2], alpha);
        output[3] = alpha;
    }
}

fn unpremultiply(component: u8, alpha: u8) -> u8 {
    if alpha == 0 {
        0
    } else {
        ((u16::from(component) * 255 + u16::from(alpha) / 2) / u16::from(alpha)).min(255) as u8
    }
}

/// A frame-scoped HTML5 Canvas 2D-like drawing context.
///
/// A borrowed context cannot be retained beyond its draw closure:
///
/// ```compile_fail
/// let mut canvas = bcanvas::OffscreenCanvas::new(1, 1);
/// let mut saved = None;
/// canvas.draw(|context| saved = Some(context));
/// ```
///
/// Drawing contexts cannot be sent to another thread, even within the frame:
///
/// ```compile_fail
/// let mut canvas = bcanvas::OffscreenCanvas::new(1, 1);
/// canvas.draw(|context| {
///     std::thread::scope(|scope| {
///         scope.spawn(move || context.fill_rect(0.0, 0.0, 1.0, 1.0));
///     });
/// });
/// ```
pub struct CanvasRenderingContext2d<'a> {
    pub(crate) platform: PlatformCanvasContext,
    pub(crate) state: CanvasState,
    pub(crate) stack: Vec<(CanvasState, Option<LayerPhase>)>,
    width: f32,
    height: f32,
    scale_factor: f32,
    timestamp: Duration,
    _lifetime: PhantomData<(&'a mut (), Rc<()>)>,
}

impl<'a> CanvasRenderingContext2d<'a> {
    pub(crate) fn new(
        platform: PlatformCanvasContext,
        width: f32,
        height: f32,
        scale_factor: f32,
    ) -> Self {
        static START: OnceLock<Instant> = OnceLock::new();
        Self {
            platform,
            state: CanvasState::default(),
            stack: Vec::new(),
            width,
            height,
            scale_factor,
            timestamp: START.get_or_init(Instant::now).elapsed(),
            _lifetime: PhantomData,
        }
    }
    /// Width of this frame in logical pixels.
    pub const fn width(&self) -> f32 {
        self.width
    }
    /// Height of this frame in logical pixels.
    pub const fn height(&self) -> f32 {
        self.height
    }
    /// Native pixels per logical pixel for this frame.
    pub const fn scale_factor(&self) -> f32 {
        self.scale_factor
    }
    /// Monotonic time since the first canvas frame in this process.
    pub const fn timestamp(&self) -> Duration {
        self.timestamp
    }
    /// Save styles, the transform, and the clipping region, but not the current path.
    pub fn save(&mut self) {
        self.platform.save();
        self.stack.push((self.state.clone(), None));
    }
    /// Restore the most recently saved drawing state. An empty stack and a state saved by
    /// a layer, which `end_layer` restores, are no-ops.
    pub fn restore(&mut self) {
        if matches!(self.stack.last(), Some((_, None))) {
            let (state, _) = self.stack.pop().expect("saved state");
            self.platform.restore();
            self.state = state;
        }
    }
    /// Start a layer that is drawn into on its own and composited when it ends, with the
    /// global alpha and composite operation of now. Like `save`, the layer keeps the drawing
    /// state, and its drawing starts with full alpha and source over compositing.
    pub fn begin_layer(&mut self) {
        self.platform.begin_layer(&self.state);
        self.push_layer(LayerPhase::Content);
    }
    /// Start a layer that is masked: drawing until `end_mask` makes the mask, whose alpha
    /// is the coverage of the drawing after it until `end_layer`.
    pub fn begin_mask_layer(&mut self) {
        self.platform.begin_mask_layer(&self.state);
        self.push_layer(LayerPhase::Mask);
    }
    /// Finish the mask of a masked layer and start its content, with the drawing state of
    /// the layer start. Without a mask being drawn, this is a no-op.
    pub fn end_mask(&mut self) {
        let Some(index) = self.layer_index() else {
            return;
        };
        if self.stack[index].1 != Some(LayerPhase::Mask) {
            return;
        }
        self.unwind_saves(index);
        self.platform.end_mask();
        self.stack[index].1 = Some(LayerPhase::Content);
        self.state = self.layer_state(index);
    }
    /// Composite the most recent layer and restore the drawing state of its start. Without
    /// a layer this is a no-op.
    pub fn end_layer(&mut self) {
        let Some(index) = self.layer_index() else {
            return;
        };
        self.unwind_saves(index);
        if self.stack[index].1 == Some(LayerPhase::Mask) {
            self.platform.end_mask();
        }
        self.platform.end_layer();
        let (state, _) = self.stack.pop().expect("layer state");
        self.state = state;
    }
    fn push_layer(&mut self, phase: LayerPhase) {
        self.stack.push((self.state.clone(), Some(phase)));
        self.state = self.layer_state(self.stack.len() - 1);
    }
    // The drawing state inside a layer, which composites with the alpha and operation itself.
    fn layer_state(&self, index: usize) -> CanvasState {
        CanvasState {
            alpha: 1.0,
            composite: CompositeOperation::SourceOver,
            ..self.stack[index].0.clone()
        }
    }
    fn layer_index(&self) -> Option<usize> {
        self.stack.iter().rposition(|(_, layer)| layer.is_some())
    }
    // Restore the states saved inside the layer at `index`.
    fn unwind_saves(&mut self, index: usize) {
        while self.stack.len() > index + 1 {
            self.stack.pop();
            self.platform.restore();
        }
    }
    /// Current fill style.
    pub const fn fill_style(&self) -> &CanvasStyle {
        &self.state.fill
    }
    /// Set the fill color or gradient.
    pub fn set_fill_style(&mut self, style: impl Into<CanvasStyle>) {
        self.state.fill = style.into();
    }
    /// Current stroke style.
    pub const fn stroke_style(&self) -> &CanvasStyle {
        &self.state.stroke
    }
    /// Set the stroke color or gradient.
    pub fn set_stroke_style(&mut self, style: impl Into<CanvasStyle>) {
        self.state.stroke = style.into();
    }
    /// Create a linear gradient, like [`CanvasGradient::linear`].
    pub fn create_linear_gradient(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> CanvasGradient {
        CanvasGradient::linear(x0, y0, x1, y1)
    }
    /// Create a radial gradient, like [`CanvasGradient::radial`].
    pub fn create_radial_gradient(
        &self,
        fx: f32,
        fy: f32,
        cx: f32,
        cy: f32,
        radius: f32,
    ) -> CanvasGradient {
        CanvasGradient::radial(fx, fy, cx, cy, radius)
    }
    /// Current composite operation.
    pub const fn global_composite_operation(&self) -> CompositeOperation {
        self.state.composite
    }
    /// Set how drawing and layers composite onto the existing content.
    pub const fn set_global_composite_operation(&mut self, operation: CompositeOperation) {
        self.state.composite = operation;
    }
    /// Whether scaled images are smoothed, otherwise their pixels stay sharp.
    pub const fn image_smoothing_enabled(&self) -> bool {
        self.state.image_smoothing
    }
    /// Set whether scaled images are smoothed.
    pub const fn set_image_smoothing_enabled(&mut self, enabled: bool) {
        self.state.image_smoothing = enabled;
    }
    /// Current global alpha, from 0.0 to 1.0.
    pub const fn global_alpha(&self) -> f32 {
        self.state.alpha
    }
    /// Set global alpha. Non-finite values and values outside 0.0..=1.0 are ignored.
    pub const fn set_global_alpha(&mut self, alpha: f32) {
        if alpha.is_finite() && alpha >= 0.0 && alpha <= 1.0 {
            self.state.alpha = alpha;
        }
    }
    /// Current line width in logical pixels.
    pub const fn line_width(&self) -> f32 {
        self.state.line_width
    }
    /// Set line width. Non-finite, zero, and negative values are ignored.
    pub const fn set_line_width(&mut self, width: f32) {
        if width.is_finite() && width > 0.0 {
            self.state.line_width = width;
        }
    }
    /// Current line cap.
    pub const fn line_cap(&self) -> LineCap {
        self.state.line_cap
    }
    /// Set line cap.
    pub const fn set_line_cap(&mut self, cap: LineCap) {
        self.state.line_cap = cap;
    }
    /// Current line join.
    pub const fn line_join(&self) -> LineJoin {
        self.state.line_join
    }
    /// Set line join.
    pub const fn set_line_join(&mut self, join: LineJoin) {
        self.state.line_join = join;
    }
    /// Current miter limit.
    pub const fn miter_limit(&self) -> f32 {
        self.state.miter_limit
    }
    /// Set the miter limit. Non-finite, zero, and negative values are ignored.
    pub const fn set_miter_limit(&mut self, limit: f32) {
        if limit.is_finite() && limit > 0.0 {
            self.state.miter_limit = limit;
        }
    }
    /// Current dash pattern of alternating dash and gap lengths.
    pub fn line_dash(&self) -> &[f32] {
        &self.state.line_dash
    }
    /// Set the dash pattern, an odd number of lengths is repeated to make it even. Patterns
    /// with non-finite or negative lengths are ignored and an empty one draws solid lines.
    pub fn set_line_dash(&mut self, segments: &[f32]) {
        if !segments
            .iter()
            .all(|length| length.is_finite() && *length >= 0.0)
        {
            return;
        }
        self.state.line_dash = if segments.len() % 2 == 1 {
            segments.iter().chain(segments).copied().collect()
        } else {
            segments.into()
        };
    }
    /// Current dash offset.
    pub const fn line_dash_offset(&self) -> f32 {
        self.state.line_dash_offset
    }
    /// Set the dash offset. Non-finite values are ignored.
    pub const fn set_line_dash_offset(&mut self, offset: f32) {
        if offset.is_finite() {
            self.state.line_dash_offset = offset;
        }
    }
    /// Current font family.
    pub fn font_family(&self) -> &str {
        &self.state.font_family
    }
    /// Current font size in logical pixels.
    pub const fn font_size(&self) -> f32 {
        self.state.font_size
    }
    /// Set text font family and size in logical pixels, rather than a CSS font string.
    /// Non-finite, zero, and negative sizes leave both properties unchanged.
    pub fn set_font(&mut self, family: impl Into<String>, size: f32) {
        if !size.is_finite() || size <= 0.0 {
            return;
        }
        let family = family.into();
        if self.state.font_family.as_ref() != family {
            self.state.font_family = family.into();
        }
        self.state.font_size = size;
    }
    /// Current font weight.
    pub const fn font_weight(&self) -> FontWeight {
        self.state.font_weight
    }
    /// Set text font weight.
    pub const fn set_font_weight(&mut self, weight: FontWeight) {
        self.state.font_weight = weight;
    }
    /// Current horizontal text alignment.
    pub const fn text_align(&self) -> TextAlign {
        self.state.text_align
    }
    /// Set horizontal text alignment.
    pub const fn set_text_align(&mut self, align: TextAlign) {
        self.state.text_align = align;
    }
    /// Current text baseline.
    pub const fn text_baseline(&self) -> TextBaseline {
        self.state.text_baseline
    }
    /// Set vertical text baseline.
    pub const fn set_text_baseline(&mut self, baseline: TextBaseline) {
        self.state.text_baseline = baseline;
    }
    /// Clear a rectangle.
    pub fn clear_rect(&mut self, x: f32, y: f32, width: f32, height: f32) {
        if ![x, y, width, height].iter().all(|v| v.is_finite()) {
            return;
        }
        self.platform.clear_rect(x, y, width, height);
    }
    /// Fill a rectangle.
    pub fn fill_rect(&mut self, x: f32, y: f32, width: f32, height: f32) {
        if ![x, y, width, height].iter().all(|v| v.is_finite()) {
            return;
        }
        self.platform.fill_rect(&self.state, x, y, width, height);
    }
    /// Stroke a rectangle.
    pub fn stroke_rect(&mut self, x: f32, y: f32, width: f32, height: f32) {
        if ![x, y, width, height].iter().all(|v| v.is_finite()) {
            return;
        }
        self.platform.stroke_rect(&self.state, x, y, width, height);
    }
    /// Begin a new path.
    pub fn begin_path(&mut self) {
        self.platform.begin_path();
    }
    /// Close the current path.
    pub fn close_path(&mut self) {
        self.platform.close_path();
    }
    /// Move the current point.
    pub fn move_to(&mut self, x: f32, y: f32) {
        if ![x, y].iter().all(|v| v.is_finite()) {
            return;
        }
        self.platform.move_to(x, y);
    }
    /// Add a line segment.
    pub fn line_to(&mut self, x: f32, y: f32) {
        if ![x, y].iter().all(|v| v.is_finite()) {
            return;
        }
        self.platform.line_to(x, y);
    }
    /// Add a rectangle to the path.
    pub fn rect(&mut self, x: f32, y: f32, width: f32, height: f32) {
        if ![x, y, width, height].iter().all(|v| v.is_finite()) {
            return;
        }
        self.platform.rect(x, y, width, height);
    }
    /// Add a rounded rectangle to the current path.
    /// Negative dimensions reverse the corresponding axis. Non-finite arguments are ignored.
    ///
    /// # Panics
    /// Panics if the radius is negative and all arguments are finite.
    pub fn round_rect(&mut self, x: f32, y: f32, width: f32, height: f32, radius: f32) {
        if ![x, y, width, height, radius].iter().all(|v| v.is_finite()) {
            return;
        }
        assert!(
            radius >= 0.0,
            "rounded rectangle radius must be nonnegative"
        );
        let radius = radius.min(width.abs() / 2.0).min(height.abs() / 2.0);
        let rx = radius.copysign(width);
        let ry = radius.copysign(height);
        // Cubic approximation of a quarter circle, shared by every native backend.
        let k = 4.0 / 3.0 * (std::f32::consts::FRAC_PI_2 / 4.0).tan();
        let (right, bottom) = (x + width, y + height);
        self.move_to(x + rx, y);
        self.line_to(right - rx, y);
        self.bezier_curve_to(
            right - rx + k * rx,
            y,
            right,
            y + ry - k * ry,
            right,
            y + ry,
        );
        self.line_to(right, bottom - ry);
        self.bezier_curve_to(
            right,
            bottom - ry + k * ry,
            right - rx + k * rx,
            bottom,
            right - rx,
            bottom,
        );
        self.line_to(x + rx, bottom);
        self.bezier_curve_to(
            x + rx - k * rx,
            bottom,
            x,
            bottom - ry + k * ry,
            x,
            bottom - ry,
        );
        self.line_to(x, y + ry);
        self.bezier_curve_to(x, y + ry - k * ry, x + rx - k * rx, y, x + rx, y);
        self.close_path();
        self.move_to(x, y);
    }
    /// Add a complete, axis-aligned ellipse to the current path.
    /// Unlike browser Canvas 2D, this does not accept rotation or arc angles.
    /// Non-finite arguments are ignored.
    ///
    /// # Panics
    /// Panics if either radius is negative and all arguments are finite.
    pub fn ellipse(&mut self, x: f32, y: f32, radius_x: f32, radius_y: f32) {
        if ![x, y, radius_x, radius_y].iter().all(|v| v.is_finite()) {
            return;
        }
        assert!(
            radius_x >= 0.0 && radius_y >= 0.0,
            "ellipse radii must be nonnegative"
        );
        self.platform.ellipse(x, y, radius_x, radius_y);
    }
    /// Add a circular arc, with angles in radians.
    /// Non-finite arguments are ignored.
    ///
    /// # Panics
    /// Panics if the radius is negative and all arguments are finite.
    pub fn arc(
        &mut self,
        x: f32,
        y: f32,
        radius: f32,
        start: f32,
        end: f32,
        counterclockwise: bool,
    ) {
        if ![x, y, radius, start, end].iter().all(|v| v.is_finite()) {
            return;
        }
        assert!(radius >= 0.0, "arc radius must be nonnegative");
        self.platform
            .arc(x, y, radius, start, end, counterclockwise);
    }
    /// Add a quadratic Bezier segment.
    pub fn quadratic_curve_to(&mut self, cpx: f32, cpy: f32, x: f32, y: f32) {
        if ![cpx, cpy, x, y].iter().all(|v| v.is_finite()) {
            return;
        }
        self.platform.quadratic_curve_to(cpx, cpy, x, y);
    }
    /// Add a cubic Bezier segment.
    pub fn bezier_curve_to(&mut self, cp1x: f32, cp1y: f32, cp2x: f32, cp2y: f32, x: f32, y: f32) {
        if ![cp1x, cp1y, cp2x, cp2y, x, y].iter().all(|v| v.is_finite()) {
            return;
        }
        self.platform.bezier_curve_to(cp1x, cp1y, cp2x, cp2y, x, y);
    }
    /// Fill the current path without consuming it.
    pub fn fill(&mut self) {
        self.fill_with(FillRule::NonZero);
    }
    /// Fill the current path with a fill rule without consuming it.
    pub fn fill_with(&mut self, rule: FillRule) {
        self.platform.fill(&self.state, rule);
    }
    /// Stroke the current path without consuming it.
    pub fn stroke(&mut self) {
        self.platform.stroke(&self.state);
    }
    /// Clip future drawing to the current path without consuming it.
    pub fn clip(&mut self) {
        self.clip_with(FillRule::NonZero);
    }
    /// Clip future drawing to the current path with a fill rule without consuming it.
    pub fn clip_with(&mut self, rule: FillRule) {
        self.platform.clip(rule);
    }
    /// Draw an image scaled into a rectangle.
    pub fn draw_image(&mut self, image: &CanvasImage, x: f32, y: f32, width: f32, height: f32) {
        if ![x, y, width, height].iter().all(|v| v.is_finite()) {
            return;
        }
        self.platform
            .draw_image(&self.state, &image.0, x, y, width, height);
    }
    /// Translate the current transform.
    pub fn translate(&mut self, x: f32, y: f32) {
        if ![x, y].iter().all(|v| v.is_finite()) {
            return;
        }
        self.platform.translate(x, y);
    }
    /// Rotate the current transform by radians.
    pub fn rotate(&mut self, angle: f32) {
        if !angle.is_finite() {
            return;
        }
        self.platform.rotate(angle);
    }
    /// Scale the current transform.
    pub fn scale(&mut self, x: f32, y: f32) {
        if ![x, y].iter().all(|v| v.is_finite()) {
            return;
        }
        self.platform.scale(x, y);
    }
    /// Multiply the current transform with an affine transform.
    pub fn transform(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) {
        if ![a, b, c, d, e, f].iter().all(|v| v.is_finite()) {
            return;
        }
        self.platform.transform(a, b, c, d, e, f);
    }
    /// Replace the current affine transform.
    pub fn set_transform(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) {
        if ![a, b, c, d, e, f].iter().all(|v| v.is_finite()) {
            return;
        }
        self.platform.set_transform(a, b, c, d, e, f);
    }
    /// Reset the current transform.
    pub fn reset_transform(&mut self) {
        self.platform.reset_transform();
    }
    /// Draw filled text.
    pub fn fill_text(&mut self, text: impl AsRef<str>, x: f32, y: f32) {
        if ![x, y].iter().all(|v| v.is_finite()) {
            return;
        }
        self.platform.fill_text(&self.state, text.as_ref(), x, y);
    }
    /// Measure text width in logical pixels.
    pub fn measure_text(&mut self, text: impl AsRef<str>) -> f32 {
        self.platform.measure_text(&self.state, text.as_ref())
    }
}

impl Drop for CanvasRenderingContext2d<'_> {
    fn drop(&mut self) {
        while let Some((_, layer)) = self.stack.pop() {
            match layer {
                None => self.platform.restore(),
                Some(phase) => {
                    if phase == LayerPhase::Mask {
                        self.platform.end_mask();
                    }
                    self.platform.end_layer();
                }
            }
        }
    }
}

// Whether a layer is drawing its mask or its content.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LayerPhase {
    Mask,
    Content,
}
