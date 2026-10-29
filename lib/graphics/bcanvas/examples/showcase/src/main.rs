/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! An animated tour of native paths, gradient paints, blending, masks, and images.

use std::f32::consts::{PI, TAU};
use std::time::Duration;

use bcanvas::{
    CanvasBuilder, CanvasGradient, CanvasImage, CanvasRenderingContext2d, Color,
    CompositeOperation, FillRule, FontWeight, GradientSpread, LineCap, LineJoin, TextAlign,
    TextBaseline,
};
use bwindow::{
    Event, EventLoop, LogicalInsets, LogicalSize, MouseButton, Theme, WindowBuilder, WindowEvent,
};

const WIDTH: f32 = 1_000.0;
const HEIGHT: f32 = 860.0;

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
    pointer_down: bool,
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
        .min_size(LogicalSize::new(680.0, 585.0))
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
        pointer_down: false,
    };
    let mut frame_rate = FrameRate::default();
    let stars = starfield();
    let id = window.id();
    canvas.request_animation_frame();
    event_loop.run(move |event| match event {
        Event::Window(window_id, WindowEvent::RedrawRequested) if window_id == id => {
            if canvas.draw(|ctx| {
                let fps = frame_rate.sample(ctx.timestamp());
                draw(ctx, state, fps, &stars);
            }) {
                canvas.request_animation_frame();
            }
        }
        Event::Window(window_id, WindowEvent::MouseMove { position, .. }) if window_id == id => {
            state.pointer_x = position.x;
            state.pointer_y = position.y;
        }
        Event::Window(window_id, WindowEvent::MouseDown(event))
            if window_id == id && event.button == MouseButton::Left =>
        {
            state.pointer_x = event.position.x;
            state.pointer_y = event.position.y;
            state.pointer_down = true;
        }
        Event::Window(window_id, WindowEvent::MouseUp(event))
            if window_id == id && event.button == MouseButton::Left =>
        {
            state.pointer_x = event.position.x;
            state.pointer_y = event.position.y;
            state.pointer_down = false;
        }
        Event::Window(window_id, WindowEvent::Blur | WindowEvent::MouseLeave)
            if window_id == id =>
        {
            state.pointer_down = false;
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

fn draw(ctx: &mut CanvasRenderingContext2d<'_>, state: State, fps: u32, stars: &CanvasImage) {
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
    draw_gallery(ctx, &palette, time, pointer_x, pointer_y, stars);
    draw_input(
        ctx,
        &palette,
        time,
        pointer_x,
        pointer_y,
        state.pointer_down,
    );

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
        "Native drawing / gradients, layers, masks & motion",
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

    let mut gradient = CanvasGradient::linear(54.0, 173.0, 158.0, 237.0);
    gradient.add_color_stop(0.0, palette.cyan);
    gradient.add_color_stop(1.0, palette.blue);
    ctx.set_fill_style(gradient);
    ctx.begin_path();
    ctx.round_rect(54.0, 173.0, 104.0, 64.0, 16.0);
    ctx.fill();
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
    ctx.set_line_dash(&[10.0, 7.0]);
    ctx.set_line_dash_offset(-time * 24.0);
    ctx.stroke();
    ctx.set_line_dash(&[]);

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
    // Bound the mask before opening its layers so their buffers cover only the text.
    ctx.save();
    ctx.begin_path();
    ctx.rect(668.0, 165.0, 280.0, 80.0);
    ctx.clip();
    ctx.begin_mask_layer();
    ctx.set_fill_style(Color::rgb(255, 255, 255));
    ctx.fill_text("Aa 1234", 808.0, 205.0);
    ctx.end_mask();
    let mut ink = CanvasGradient::linear(710.0, 0.0, 906.0, 0.0);
    ink.add_color_stop(0.0, palette.blue);
    ink.add_color_stop(0.5, palette.violet);
    ink.add_color_stop(1.0, palette.coral);
    ctx.set_fill_style(ink);
    ctx.fill_rect(668.0, 165.0, 280.0, 80.0);
    ctx.end_layer();
    ctx.restore();

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

// Generate once and share the native image across frames. Opaque pixels are already
// premultiplied; the byte order here is BGRA, rather than the RGBA of offscreen output.
fn starfield() -> CanvasImage {
    let mut pixels = Vec::with_capacity(256 * 192 * 4);
    for y in 0..192_u32 {
        for x in 0..256_u32 {
            let hash = x.wrapping_mul(374_761_393) ^ y.wrapping_mul(668_265_263);
            let star = hash.wrapping_mul(hash ^ (hash >> 13)) % 701 < 3;
            let glow = if star { 150 + (hash % 106) as u8 } else { 0 };
            pixels.extend_from_slice(&[
                38_u8.saturating_add(glow),
                20_u8.saturating_add(glow),
                12_u8.saturating_add(glow),
                255,
            ]);
        }
    }
    CanvasImage::from_premultiplied_bgra(256, 192, pixels.into())
}

fn draw_gallery(
    ctx: &mut CanvasRenderingContext2d<'_>,
    palette: &Palette,
    time: f32,
    pointer_x: f32,
    pointer_y: f32,
    stars: &CanvasImage,
) {
    card(
        ctx,
        palette,
        Rect::new(32.0, 364.0, 936.0, 324.0),
        "Paint in motion",
    );
    for (x, label) in [
        (52.0, "01 / GRADIENT FLOW"),
        (356.0, "02 / BLENDED ORBITS"),
        (660.0, "03 / MASKED LANDSCAPE"),
    ] {
        gallery_panel(ctx, palette, Rect::new(x, 417.0, 288.0, 246.0), label);
    }
    draw_flow(ctx, palette, time);
    draw_orbits(ctx, palette, time, pointer_x, pointer_y);
    draw_landscape(ctx, palette, time, stars);
}

fn draw_flow(ctx: &mut CanvasRenderingContext2d<'_>, palette: &Palette, time: f32) {
    ctx.save();
    ctx.begin_path();
    ctx.round_rect(68.0, 459.0, 256.0, 166.0, 12.0);
    ctx.clip();
    let mut wash = CanvasGradient::linear(0.0, 459.0, 0.0, 625.0);
    wash.add_color_stop(0.0, Color::from_rgb(0x122544));
    wash.add_color_stop(1.0, Color::from_rgb(0x261c46));
    ctx.set_fill_style(wash);
    ctx.fill_rect(68.0, 459.0, 256.0, 166.0);

    let mut ribbon = CanvasGradient::linear(0.0, 0.0, 90.0, 0.0);
    ribbon.add_color_stop(0.0, palette.cyan);
    ribbon.add_color_stop(0.5, palette.blue);
    ribbon.add_color_stop(1.0, palette.violet);
    ribbon.set_spread(GradientSpread::Reflect);
    ribbon.set_transform(1.0, 0.2, 0.0, 1.0, time * 26.0 % 180.0, 0.0);
    ctx.set_stroke_style(ribbon);
    ctx.set_line_width(9.0);
    // The ribbons share one paint: rasterize their combined outline in one pass.
    ctx.begin_path();
    for index in 0..9 {
        let y = 458.0 + index as f32 * 23.0;
        let wave = (time * 0.9 + index as f32 * 0.4).sin() * 40.0;
        ctx.move_to(48.0, y);
        ctx.bezier_curve_to(136.0, y - 65.0 + wave, 235.0, y + 65.0 - wave, 344.0, y);
    }
    ctx.stroke();
    ctx.restore();
    caption(ctx, palette, "Reflected color / transformed paint", 196.0);
}

fn draw_orbits(
    ctx: &mut CanvasRenderingContext2d<'_>,
    palette: &Palette,
    time: f32,
    pointer_x: f32,
    pointer_y: f32,
) {
    ctx.save();
    ctx.translate(500.0, 542.0);
    // Even-odd clipping cuts a moving aperture out of all the overlapping paint.
    ctx.begin_path();
    ctx.arc(0.0, 0.0, 81.0, 0.0, TAU, false);
    ctx.close_path();
    ctx.arc(0.0, 0.0, 17.0 + time.sin() * 4.0, 0.0, TAU, false);
    ctx.close_path();
    ctx.clip_with(FillRule::EvenOdd);
    ctx.set_fill_style(Color::from_rgb(0x101a30));
    ctx.fill_rect(-90.0, -90.0, 180.0, 180.0);
    ctx.set_global_alpha(0.9);
    ctx.begin_layer();
    let drift_x = ((pointer_x - 500.0) * 0.08).clamp(-14.0, 14.0);
    let drift_y = ((pointer_y - 542.0) * 0.08).clamp(-12.0, 12.0);
    for (index, color) in [palette.cyan, palette.violet, palette.coral]
        .into_iter()
        .enumerate()
    {
        let angle = time * 0.6 + index as f32 * TAU / 3.0;
        let x = angle.cos() * 31.0 + drift_x;
        let y = angle.sin() * 31.0 + drift_y;
        let mut light = CanvasGradient::radial(x - 12.0, y - 15.0, x, y, 63.0);
        light.add_color_stop(0.0, color);
        light.add_color_stop(1.0, Color::rgba(0, 0, 0, 0));
        ctx.set_global_composite_operation(CompositeOperation::Screen);
        ctx.set_fill_style(light);
        ctx.begin_path();
        ctx.arc(x, y, 63.0, 0.0, TAU, false);
        ctx.fill();
    }
    ctx.end_layer();
    ctx.restore();

    ctx.save();
    ctx.set_line_dash(&[3.0, 7.0]);
    ctx.set_line_dash_offset(-time * 12.0);
    ctx.set_line_width(1.5);
    ctx.set_stroke_style(palette.muted);
    ctx.begin_path();
    ctx.arc(500.0, 542.0, 87.0, 0.0, TAU, false);
    ctx.stroke();
    ctx.restore();
    caption(ctx, palette, "Screen blend / move your pointer", 500.0);
}

fn draw_landscape(
    ctx: &mut CanvasRenderingContext2d<'_>,
    palette: &Palette,
    time: f32,
    stars: &CanvasImage,
) {
    ctx.save();
    // Limit both mask buffers to the scene rather than the whole window.
    ctx.begin_path();
    ctx.rect(716.0, 450.0, 176.0, 176.0);
    ctx.clip();
    // A soft alpha mask fades the image and every mountain together at the edge.
    ctx.begin_mask_layer();
    let mut mask = CanvasGradient::radial(804.0, 538.0, 804.0, 538.0, 88.0);
    mask.add_color_stop(0.0, Color::rgb(255, 255, 255));
    mask.add_color_stop(0.8, Color::rgb(255, 255, 255));
    mask.add_color_stop(1.0, Color::rgba(255, 255, 255, 0));
    ctx.set_fill_style(mask);
    ctx.fill_rect(716.0, 450.0, 176.0, 176.0);
    ctx.end_mask();
    ctx.draw_image(stars, 676.0, 450.0, 256.0, 192.0);

    let moon_x = 814.0 + (time * 0.3).sin() * 20.0;
    let mut moon = CanvasGradient::radial(moon_x - 5.0, 480.0, moon_x, 487.0, 20.0);
    moon.add_color_stop(0.0, Color::from_rgb(0xfff4d4));
    moon.add_color_stop(1.0, palette.gold);
    ctx.set_fill_style(moon);
    ctx.begin_path();
    ctx.arc(moon_x, 487.0, 20.0, 0.0, TAU, false);
    ctx.fill();

    for index in 0..3 {
        let depth = index as f32;
        let mut ridge = CanvasGradient::linear(0.0, 520.0 + depth * 24.0, 0.0, 635.0);
        ridge.add_color_stop(0.0, [palette.violet, palette.blue, palette.cyan][index]);
        ridge.add_color_stop(1.0, Color::from_rgb(0x101a30));
        ctx.set_fill_style(ridge);
        ctx.begin_path();
        ctx.move_to(700.0, 640.0);
        for step in 0..=40 {
            let x = 700.0 + step as f32 * 5.2;
            let phase = x * 0.035 + time * (0.18 + depth * 0.12);
            let y = 548.0 + depth * 25.0 + phase.sin() * 17.0 + (phase * 1.7).cos() * 9.0;
            ctx.line_to(x, y);
        }
        ctx.line_to(908.0, 640.0);
        ctx.close_path();
        ctx.fill();
    }
    ctx.end_layer();
    ctx.restore();
    caption(ctx, palette, "Image pixels / soft alpha mask", 804.0);
}

fn caption(ctx: &mut CanvasRenderingContext2d<'_>, palette: &Palette, text: &str, x: f32) {
    ctx.save();
    ctx.set_font("sans-serif", 11.0);
    ctx.set_font_weight(FontWeight::Normal);
    ctx.set_fill_style(palette.muted);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(text, x, 646.0);
    ctx.restore();
}

fn draw_input(
    ctx: &mut CanvasRenderingContext2d<'_>,
    palette: &Palette,
    time: f32,
    pointer_x: f32,
    pointer_y: f32,
    pressed: bool,
) {
    card(
        ctx,
        palette,
        Rect::new(32.0, 704.0, 936.0, 124.0),
        "Input playground",
    );
    let viewport = Rect::new(52.0, 752.0, 596.0, 56.0);
    let x = pointer_x.clamp(viewport.x + 22.0, viewport.x + viewport.width - 22.0);
    let y = pointer_y.clamp(viewport.y + 18.0, viewport.y + viewport.height - 18.0);
    let color = if pressed { palette.gold } else { palette.cyan };
    ctx.save();
    ctx.begin_path();
    ctx.round_rect(
        viewport.x,
        viewport.y,
        viewport.width,
        viewport.height,
        10.0,
    );
    ctx.clip();
    ctx.set_fill_style(palette.surface_high);
    ctx.fill_rect(viewport.x, viewport.y, viewport.width, viewport.height);
    ctx.set_stroke_style(palette.border);
    ctx.set_line_width(1.0);
    ctx.begin_path();
    for index in 1..20 {
        let grid_x = viewport.x + index as f32 * 30.0;
        ctx.move_to(grid_x, viewport.y);
        ctx.line_to(grid_x, viewport.y + viewport.height);
    }
    ctx.stroke();
    ctx.set_stroke_style(color);
    ctx.set_line_dash(&[3.0, 5.0]);
    ctx.begin_path();
    ctx.move_to(viewport.x, y);
    ctx.line_to(viewport.x + viewport.width, y);
    ctx.move_to(x, viewport.y);
    ctx.line_to(x, viewport.y + viewport.height);
    ctx.stroke();
    ctx.set_line_dash(&[]);
    ctx.set_line_width(2.0);
    ctx.begin_path();
    ctx.arc(x, y, 14.0, 0.0, TAU, false);
    ctx.stroke();
    if pressed {
        // Expanding rings make held and dragged input visible beyond the crosshair.
        for index in 0..3 {
            let phase = (time * 0.8 + index as f32 / 3.0).fract();
            ctx.set_global_alpha(1.0 - phase);
            ctx.begin_path();
            ctx.arc(x, y, 18.0 + phase * 65.0, 0.0, TAU, false);
            ctx.stroke();
        }
    }
    ctx.set_global_alpha(1.0);
    circle(ctx, x, y, if pressed { 7.0 } else { 4.0 }, color);
    ctx.restore();

    ctx.save();
    ctx.set_font("sans-serif", 13.0);
    ctx.set_font_weight(FontWeight::Normal);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_style(color);
    ctx.fill_text(
        format!(
            "x {pointer_x:03.0}   y {pointer_y:03.0}   /   {}",
            if pressed { "HELD" } else { "READY" }
        ),
        672.0,
        766.0,
    );
    ctx.set_fill_style(palette.muted);
    ctx.set_font("sans-serif", 11.0);
    ctx.fill_text("Move pointer / click and drag to energize", 672.0, 793.0);
    ctx.restore();
}

fn gallery_panel(
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
