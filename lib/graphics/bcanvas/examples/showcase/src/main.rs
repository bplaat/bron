/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A visual tour of the native Bassie Canvas API.

use std::f32::consts::{PI, TAU};
use std::time::Duration;

use bcanvas::{
    CanvasBuilder, CanvasRenderingContext2d, Color, FontWeight, LineCap, LineJoin, TextAlign,
    TextBaseline,
};
use bwindow::{Event, EventLoop, LogicalInsets, LogicalSize, Theme, WindowBuilder, WindowEvent};

const WIDTH: f32 = 1_000.0;
const HEIGHT: f32 = 720.0;

#[derive(Clone, Copy)]
struct Rect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl Rect {
    const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

#[derive(Clone, Copy)]
struct Palette {
    background: u32,
    surface: Color,
    surface_high: Color,
    text: Color,
    muted: Color,
    border: Color,
    blue: Color,
    cyan: Color,
    violet: Color,
    coral: Color,
    gold: Color,
}

impl Palette {
    const fn for_theme(theme: Theme) -> Self {
        match theme {
            Theme::Light => Self {
                background: 0xf3f5f9,
                surface: Color::rgb(255, 255, 255),
                surface_high: Color::rgb(244, 247, 252),
                text: Color::rgb(28, 35, 48),
                muted: Color::rgb(104, 115, 135),
                border: Color::rgb(218, 224, 235),
                blue: Color::rgb(55, 111, 238),
                cyan: Color::rgb(28, 183, 196),
                violet: Color::rgb(132, 91, 230),
                coral: Color::rgb(239, 103, 91),
                gold: Color::rgb(232, 169, 49),
            },
            Theme::Dark => Self {
                background: 0x11151d,
                surface: Color::rgb(27, 33, 45),
                surface_high: Color::rgb(35, 43, 58),
                text: Color::rgb(238, 242, 249),
                muted: Color::rgb(157, 168, 190),
                border: Color::rgb(57, 67, 86),
                blue: Color::rgb(92, 143, 255),
                cyan: Color::rgb(57, 207, 216),
                violet: Color::rgb(167, 126, 255),
                coral: Color::rgb(255, 126, 113),
                gold: Color::rgb(246, 190, 73),
            },
        }
    }
}

#[derive(Clone, Copy)]
struct State {
    theme: Theme,
    safe_area: LogicalInsets,
    pointer_x: f32,
    pointer_y: f32,
}

#[derive(Default)]
struct FrameRate {
    previous: Option<Duration>,
    value: f32,
}

impl FrameRate {
    fn sample(&mut self, timestamp: Duration) -> u32 {
        if let Some(elapsed) = self
            .previous
            .and_then(|previous| timestamp.checked_sub(previous))
            .filter(|elapsed| !elapsed.is_zero())
        {
            let current = 1.0 / elapsed.as_secs_f32();
            self.value = if self.value == 0.0 {
                current
            } else {
                self.value * 0.9 + current * 0.1
            };
        }
        self.previous = Some(timestamp);
        self.value.round() as u32
    }
}

fn main() {
    let event_loop = EventLoop::new();
    let theme = event_loop.theme();
    let palette = Palette::for_theme(theme);
    let mut window = WindowBuilder::new()
        .title("Bassie Canvas Showcase")
        .size(LogicalSize::new(WIDTH, HEIGHT))
        .min_size(LogicalSize::new(680.0, 490.0))
        .resizable(true)
        .background_color(palette.background)
        .center()
        .build();
    let mut canvas = CanvasBuilder::new(&window).build();
    let mut state = State {
        theme,
        safe_area: window.safe_area(),
        pointer_x: WIDTH * 0.5,
        pointer_y: HEIGHT * 0.5,
    };
    let mut frame_rate = FrameRate::default();
    let id = window.id();
    canvas.request_animation_frame();
    event_loop.run(move |event| match event {
        Event::Window(window_id, WindowEvent::RedrawRequested) if window_id == id => {
            if canvas.draw(|ctx| {
                let fps = frame_rate.sample(ctx.timestamp());
                draw(ctx, state, fps);
            }) {
                canvas.request_animation_frame();
            }
        }
        Event::Window(window_id, WindowEvent::MouseMove { position, .. }) if window_id == id => {
            state.pointer_x = position.x;
            state.pointer_y = position.y;
        }
        Event::Window(window_id, WindowEvent::SafeAreaChanged(safe_area)) if window_id == id => {
            state.safe_area = safe_area;
        }
        Event::Window(window_id, WindowEvent::ThemeChanged(theme)) if window_id == id => {
            state.theme = theme;
            window.set_background_color(Palette::for_theme(theme).background);
        }
        _ => {}
    });
}

fn draw(ctx: &mut CanvasRenderingContext2d<'_>, state: State, fps: u32) {
    let palette = Palette::for_theme(state.theme);
    // Fit the design inside the safe area, the background fills the whole window.
    let safe_area = state.safe_area;
    let width = (ctx.width() - safe_area.left - safe_area.right).max(0.0);
    let height = (ctx.height() - safe_area.top - safe_area.bottom).max(0.0);
    let scale = (width / WIDTH).min(height / HEIGHT).max(0.01);
    let offset_x = safe_area.left + (width - WIDTH * scale) * 0.5;
    let offset_y = safe_area.top + (height - HEIGHT * scale) * 0.5;
    let time = ctx.timestamp().as_secs_f32();
    let pointer_x = ((state.pointer_x - offset_x) / scale).clamp(0.0, WIDTH);
    let pointer_y = ((state.pointer_y - offset_y) / scale).clamp(0.0, HEIGHT);

    ctx.set_fill_style(Color::from_rgb(palette.background));
    ctx.fill_rect(0.0, 0.0, ctx.width(), ctx.height());
    ctx.save();
    ctx.translate(offset_x, offset_y);
    ctx.scale(scale, scale);

    draw_header(ctx, &palette, fps);
    draw_shapes(ctx, &palette, time);
    draw_paths(ctx, &palette, time);
    draw_typography(ctx, &palette, time);
    draw_motion(ctx, &palette, time, pointer_x, pointer_y);

    ctx.restore();
}

fn draw_header(ctx: &mut CanvasRenderingContext2d<'_>, palette: &Palette, fps: u32) {
    ctx.set_fill_style(palette.text);
    ctx.set_font("sans-serif", 34.0);
    ctx.set_font_weight(FontWeight::Bold);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Bassie Canvas", 32.0, 24.0);

    let title_width = ctx.measure_text("Bassie Canvas");
    ctx.set_fill_style(palette.blue);
    ctx.fill_rect(32.0, 65.0, title_width, 3.0);
    ctx.set_fill_style(palette.muted);
    ctx.set_font("sans-serif", 15.0);
    ctx.set_font_weight(FontWeight::Normal);
    ctx.fill_text(
        "Native paths, text, transforms, clipping and animation",
        32.0,
        76.0,
    );

    ctx.set_font("sans-serif", 12.0);
    ctx.set_text_align(TextAlign::Right);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(format!("{fps} FPS"), 968.0, 24.0);
    ctx.set_text_align(TextAlign::Left);
}

fn draw_shapes(ctx: &mut CanvasRenderingContext2d<'_>, palette: &Palette, time: f32) {
    let bounds = Rect::new(32.0, 118.0, 292.0, 230.0);
    card(ctx, palette, bounds, "Shapes & alpha");

    round_rect(ctx, Rect::new(54.0, 173.0, 104.0, 64.0), 16.0, palette.blue);
    ctx.set_stroke_style(palette.cyan);
    ctx.set_line_width(4.0);
    ctx.begin_path();
    ctx.round_rect(174.0, 173.0, 104.0, 64.0, 16.0);
    ctx.stroke();

    ctx.set_global_alpha(0.78);
    ctx.begin_path();
    ctx.ellipse(103.0, 282.0, 49.0, 28.0);
    ctx.set_fill_style(palette.violet);
    ctx.fill();
    ctx.set_global_alpha(1.0);

    let sweep = (time * 1.5).sin() * 0.25 + 0.72;
    ctx.set_stroke_style(palette.coral);
    ctx.set_line_width(10.0);
    ctx.set_line_cap(LineCap::Round);
    ctx.begin_path();
    ctx.arc(
        226.0,
        282.0,
        38.0,
        -PI * 0.85,
        -PI * 0.85 + TAU * sweep,
        false,
    );
    ctx.stroke();
    ctx.set_line_cap(LineCap::Butt);
}

fn draw_paths(ctx: &mut CanvasRenderingContext2d<'_>, palette: &Palette, time: f32) {
    let bounds = Rect::new(340.0, 118.0, 292.0, 230.0);
    card(ctx, palette, bounds, "Paths & line styles");

    let wave = (time * 2.0).sin() * 13.0;
    ctx.set_stroke_style(palette.blue);
    ctx.set_line_width(4.0);
    ctx.set_line_cap(LineCap::Round);
    ctx.begin_path();
    ctx.move_to(364.0, 196.0);
    ctx.bezier_curve_to(420.0, 145.0 + wave, 490.0, 247.0 - wave, 608.0, 190.0);
    ctx.stroke();

    ctx.set_stroke_style(palette.violet);
    ctx.set_line_width(3.0);
    ctx.begin_path();
    ctx.move_to(364.0, 254.0);
    ctx.quadratic_curve_to(486.0, 315.0 + wave, 608.0, 250.0);
    ctx.stroke();

    for (index, cap) in [LineCap::Butt, LineCap::Round, LineCap::Square]
        .into_iter()
        .enumerate()
    {
        let y = 300.0 + index as f32 * 14.0;
        ctx.set_line_cap(cap);
        ctx.set_line_width(7.0);
        ctx.set_stroke_style([palette.coral, palette.gold, palette.cyan][index]);
        ctx.begin_path();
        ctx.move_to(376.0, y);
        ctx.line_to(438.0, y);
        ctx.stroke();
    }
    ctx.set_line_cap(LineCap::Butt);

    ctx.set_line_join(LineJoin::Round);
    ctx.set_line_width(7.0);
    ctx.set_stroke_style(palette.gold);
    ctx.begin_path();
    ctx.move_to(508.0, 319.0);
    ctx.line_to(536.0, 288.0);
    ctx.line_to(564.0, 319.0);
    ctx.line_to(592.0, 288.0);
    ctx.stroke();
    ctx.set_line_join(LineJoin::Miter);
}

fn draw_typography(ctx: &mut CanvasRenderingContext2d<'_>, palette: &Palette, time: f32) {
    let bounds = Rect::new(648.0, 118.0, 320.0, 230.0);
    card(ctx, palette, bounds, "Text & measurement");

    ctx.set_fill_style(palette.text);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_font("sans-serif", 42.0);
    ctx.set_font_weight(FontWeight::Bold);
    ctx.fill_text("Aa 1234", 808.0, 205.0);

    ctx.set_font("sans-serif", 15.0);
    ctx.set_font_weight(FontWeight::Normal);
    let label = "measured text";
    let width = ctx.measure_text(label);
    ctx.set_fill_style(palette.surface_high);
    ctx.fill_rect(808.0 - width * 0.5 - 10.0, 250.0, width + 20.0, 34.0);
    ctx.set_fill_style(palette.muted);
    ctx.fill_text(label, 808.0, 267.0);

    let marker_x = 808.0 + (time * 1.3).sin() * 90.0;
    ctx.set_stroke_style(palette.border);
    ctx.set_line_width(1.0);
    ctx.stroke_rect(682.0, 303.0, 252.0, 1.0);
    circle(ctx, marker_x, 303.0, 5.0, palette.coral);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Alphabetic);
}

fn draw_motion(
    ctx: &mut CanvasRenderingContext2d<'_>,
    palette: &Palette,
    time: f32,
    pointer_x: f32,
    pointer_y: f32,
) {
    let bounds = Rect::new(32.0, 364.0, 936.0, 324.0);
    card(ctx, palette, bounds, "Rendering diagnostics");

    let clip_panel = Rect::new(52.0, 417.0, 288.0, 246.0);
    let transform_panel = Rect::new(356.0, 417.0, 288.0, 246.0);
    let pointer_panel = Rect::new(660.0, 417.0, 288.0, 246.0);
    diagnostic_panel(ctx, palette, clip_panel, "CLIP");
    diagnostic_panel(ctx, palette, transform_panel, "TRANSFORM");
    diagnostic_panel(ctx, palette, pointer_panel, "POINTER");

    // A single marker should travel inside this rounded viewport and disappear
    // cleanly at both edges. Any paint outside the border indicates broken clipping.
    let viewport = Rect::new(68.0, 463.0, 256.0, 180.0);
    ctx.save();
    ctx.begin_path();
    ctx.round_rect(
        viewport.x,
        viewport.y,
        viewport.width,
        viewport.height,
        12.0,
    );
    ctx.clip();
    ctx.set_fill_style(palette.surface_high);
    ctx.fill_rect(viewport.x, viewport.y, viewport.width, viewport.height);
    ctx.set_stroke_style(palette.border);
    ctx.set_line_width(1.0);
    for index in 1..4 {
        let x = viewport.x + viewport.width * index as f32 / 4.0;
        ctx.begin_path();
        ctx.move_to(x, viewport.y);
        ctx.line_to(x, viewport.y + viewport.height);
        ctx.stroke();
    }
    let travel = (time * 1.1).sin() * 0.5 + 0.5;
    let marker_x = viewport.x - 14.0 + travel * (viewport.width + 28.0);
    circle(
        ctx,
        marker_x,
        viewport.y + viewport.height * 0.5,
        14.0,
        palette.cyan,
    );
    ctx.restore();
    ctx.set_stroke_style(palette.border);
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.round_rect(
        viewport.x,
        viewport.y,
        viewport.width,
        viewport.height,
        12.0,
    );
    ctx.stroke();

    // Fixed crosshairs make incorrect rotation centers and transform leakage clear.
    let center_x = transform_panel.x + transform_panel.width * 0.5;
    let center_y = 553.0;
    ctx.set_stroke_style(palette.border);
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.move_to(center_x - 90.0, center_y);
    ctx.line_to(center_x + 90.0, center_y);
    ctx.move_to(center_x, center_y - 72.0);
    ctx.line_to(center_x, center_y + 72.0);
    ctx.stroke();
    ctx.save();
    ctx.translate(center_x, center_y);
    ctx.rotate(time * 0.65);
    ctx.scale(1.0, 0.72);
    round_rect(ctx, Rect::new(-42.0, -42.0, 84.0, 84.0), 14.0, palette.blue);
    ctx.set_stroke_style(palette.cyan);
    ctx.set_line_width(4.0);
    ctx.begin_path();
    ctx.move_to(0.0, 0.0);
    ctx.line_to(72.0, 0.0);
    ctx.stroke();
    circle(ctx, 72.0, 0.0, 8.0, palette.coral);
    ctx.restore();

    // The target follows the pointer but remains constrained to this viewport.
    let pointer_view = Rect::new(676.0, 463.0, 256.0, 180.0);
    ctx.set_fill_style(palette.surface_high);
    ctx.fill_rect(
        pointer_view.x,
        pointer_view.y,
        pointer_view.width,
        pointer_view.height,
    );
    ctx.set_stroke_style(palette.border);
    ctx.set_line_width(1.0);
    ctx.stroke_rect(
        pointer_view.x,
        pointer_view.y,
        pointer_view.width,
        pointer_view.height,
    );
    let target_x = pointer_x.clamp(
        pointer_view.x + 18.0,
        pointer_view.x + pointer_view.width - 18.0,
    );
    let target_y = pointer_y.clamp(
        pointer_view.y + 18.0,
        pointer_view.y + pointer_view.height - 18.0,
    );
    ctx.set_stroke_style(palette.violet);
    ctx.set_line_width(2.0);
    ctx.begin_path();
    ctx.arc(target_x, target_y, 18.0, 0.0, TAU, false);
    ctx.move_to(target_x - 28.0, target_y);
    ctx.line_to(target_x + 28.0, target_y);
    ctx.move_to(target_x, target_y - 28.0);
    ctx.line_to(target_x, target_y + 28.0);
    ctx.stroke();
    let pulse = 5.0 + ((time * 2.4).sin() * 0.5 + 0.5) * 4.0;
    circle(ctx, target_x, target_y, pulse, palette.gold);
}

fn diagnostic_panel(
    ctx: &mut CanvasRenderingContext2d<'_>,
    palette: &Palette,
    bounds: Rect,
    label: &str,
) {
    round_rect(ctx, bounds, 14.0, palette.surface_high);
    ctx.set_stroke_style(palette.border);
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.round_rect(bounds.x, bounds.y, bounds.width, bounds.height, 14.0);
    ctx.stroke();
    ctx.set_fill_style(palette.muted);
    ctx.set_font("sans-serif", 11.0);
    ctx.set_font_weight(FontWeight::Bold);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, bounds.x + 16.0, bounds.y + 23.0);
    ctx.set_font_weight(FontWeight::Normal);
    ctx.set_text_baseline(TextBaseline::Alphabetic);
}

fn card(ctx: &mut CanvasRenderingContext2d<'_>, palette: &Palette, bounds: Rect, title: &str) {
    round_rect(ctx, bounds, 16.0, palette.surface);
    ctx.set_stroke_style(palette.border);
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.round_rect(bounds.x, bounds.y, bounds.width, bounds.height, 16.0);
    ctx.stroke();
    ctx.set_fill_style(palette.text);
    ctx.set_font("sans-serif", 16.0);
    ctx.set_font_weight(FontWeight::Bold);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(title, bounds.x + 20.0, bounds.y + 30.0);
}

fn round_rect(ctx: &mut CanvasRenderingContext2d<'_>, bounds: Rect, radius: f32, color: Color) {
    ctx.begin_path();
    ctx.round_rect(bounds.x, bounds.y, bounds.width, bounds.height, radius);
    ctx.set_fill_style(color);
    ctx.fill();
}

fn circle(ctx: &mut CanvasRenderingContext2d<'_>, x: f32, y: f32, radius: f32, color: Color) {
    ctx.begin_path();
    ctx.arc(x, y, radius, 0.0, TAU, false);
    ctx.set_fill_style(color);
    ctx.fill();
}
