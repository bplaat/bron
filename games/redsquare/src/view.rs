/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::time::Duration;

use bcanvas::{
    CanvasGradient, CanvasImage, CanvasRenderingContext2d as Context, Color, FontWeight, TextAlign,
    TextBaseline, premultiplied_bgra,
};
use bwindow::LogicalPoint as Point;

use crate::game::{ARENA, Rect, SIZE};
use crate::{App, Layout};

const WHITE: Color = Color::rgb(255, 255, 255);
const INK: Color = Color::rgb(33, 33, 33);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    Play,
    Home,
    Scores,
    Help,
    Credits,
    Settings,
    Sound,
    Quit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Screen {
    Home,
    Playing,
    Results,
    Scores,
    Help,
    Credits,
    Settings,
}

#[derive(Clone, Copy)]
struct Button {
    bounds: Rect,
    label: &'static str,
    action: Action,
}

impl Button {
    const fn new(x: f32, y: f32, label: &'static str, action: Action) -> Self {
        Self {
            bounds: Rect::new(x, y, 160.0, 32.0),
            label,
            action,
        }
    }
}

const MENU: [Button; 6] = [
    Button::new(450.0, 30.0, "Play", Action::Play),
    Button::new(450.0, 70.0, "High Score", Action::Scores),
    Button::new(450.0, 110.0, "Help", Action::Help),
    Button::new(450.0, 150.0, "Credits", Action::Credits),
    Button::new(450.0, 190.0, "Sound Settings", Action::Settings),
    Button::new(450.0, 420.0, "Exit", Action::Quit),
];
const RESULTS: [Button; 2] = [
    Button::new(152.0, 386.0, "Retry", Action::Play),
    Button::new(328.0, 386.0, "Back", Action::Home),
];
const BACK: Button = Button::new(240.0, 420.0, "Back", Action::Home);
const SETTINGS: [Button; 2] = [Button::new(240.0, 204.0, "Sound: On", Action::Sound), BACK];

impl Screen {
    const fn buttons(self) -> &'static [Button] {
        match self {
            Self::Home => &MENU,
            Self::Playing => &[],
            Self::Results => &RESULTS,
            Self::Settings => &SETTINGS,
            _ => &[BACK],
        }
    }

    pub(crate) fn hit(self, point: Point) -> Option<Action> {
        self.buttons()
            .iter()
            .find(|button| button.bounds.contains(point))
            .map(|button| button.action)
    }
}

pub(crate) struct Assets {
    background: CanvasImage,
    title: CanvasImage,
    logo: CanvasImage,
    star: CanvasImage,
}

impl Assets {
    pub(crate) fn new() -> Self {
        let png = |bytes: &[u8]| {
            let image = image::decode(bytes).expect("valid RedSquare PNG");
            let bitmap = image.largest_frame().bitmap();
            CanvasImage::from_premultiplied_bgra(
                bitmap.width(),
                bitmap.height(),
                premultiplied_bgra(bitmap.data()),
            )
        };
        Self {
            background: png(include_bytes!("../assets/background.png")),
            title: png(include_bytes!("../assets/logo.png")),
            logo: png(include_bytes!("../assets/logo2.png")),
            star: png(include_bytes!("../assets/star.png")),
        }
    }
}

pub(crate) fn draw(ctx: &mut Context<'_>, assets: &Assets, app: &App, layout: Layout) {
    ctx.set_fill_style(Color::rgb(40, 40, 40));
    ctx.fill_rect(0.0, 0.0, ctx.width(), ctx.height());
    ctx.save();
    ctx.translate(layout.origin.x, layout.origin.y);
    ctx.scale(layout.scale, layout.scale);
    ctx.begin_path();
    ctx.rect(0.0, 0.0, SIZE.width, SIZE.height);
    ctx.clip();
    ctx.draw_image(&assets.background, 0.0, 0.0, SIZE.width, SIZE.height);
    ctx.set_text_baseline(TextBaseline::Middle);

    match app.screen {
        Screen::Home => {
            for (index, rect) in app.game.blues().enumerate() {
                block(ctx, rect, index < 2, false);
            }
            text(ctx, "RedSquare", 30.0, 45.0, 36.0, WHITE, true);
        }
        Screen::Playing | Screen::Results => {
            draw_game(ctx, app);
            if app.screen == Screen::Results {
                draw_results(ctx, assets, app);
            }
        }
        screen => {
            ctx.set_fill_style(Color::rgba(0, 0, 0, 122));
            ctx.fill_rect(0.0, 0.0, SIZE.width, SIZE.height);
            ctx.set_text_align(TextAlign::Center);
            let title = match screen {
                Screen::Scores => "Local High Score",
                Screen::Help => "Help",
                Screen::Credits => "Credits",
                Screen::Settings => "Sound Settings",
                _ => unreachable!(),
            };
            text(ctx, title, 320.0, 58.0, 52.0, WHITE, true);
            match screen {
                Screen::Scores => draw_scores(ctx, app),
                Screen::Help => {
                    for (index, line) in [
                        "Click and hold the red square, or drag it with a finger.",
                        "Avoid the walls and the four moving blue blocks.",
                        "The blocks speed up every ten seconds.",
                        "Can you last 31 seconds?",
                    ]
                    .iter()
                    .enumerate()
                    {
                        text(
                            ctx,
                            line,
                            320.0,
                            161.0 + index as f32 * 31.0,
                            21.0,
                            WHITE,
                            true,
                        );
                    }
                }
                Screen::Credits => {
                    ctx.draw_image(&assets.title, 192.0, 110.0, 256.0, 82.0);
                    ctx.draw_image(&assets.logo, 270.0, 207.0, 100.0, 80.0);
                    text(
                        ctx,
                        "Original game, graphics and sound: Willem van der Plaat",
                        320.0,
                        314.0,
                        19.0,
                        WHITE,
                        true,
                    );
                    text(
                        ctx,
                        "Rust port: Bastiaan van der Plaat",
                        320.0,
                        346.0,
                        19.0,
                        WHITE,
                        false,
                    );
                }
                Screen::Settings => {}
                _ => unreachable!(),
            }
        }
    }
    for &button in app.screen.buttons() {
        draw_button(ctx, app, button);
    }
    ctx.restore();
}

fn draw_game(ctx: &mut Context<'_>, app: &App) {
    ctx.set_fill_style(Color::rgba(255, 255, 255, 122));
    fill_rect(ctx, ARENA);
    ctx.set_stroke_style(Color::rgba(30, 30, 30, 215));
    ctx.set_line_width(1.0);
    ctx.stroke_rect(ARENA.x, ARENA.y, ARENA.width, ARENA.height);

    for (index, rect) in app.game.blues().enumerate() {
        block(ctx, rect, index < 2, false);
    }
    block(ctx, app.game.player, false, true);
    ctx.set_fill_style(Color::rgba(0, 0, 0, 60));
    ctx.fill_rect(0.0, 0.0, SIZE.width, ARENA.y);
    let game = &app.game;
    text(
        ctx,
        &format!("Score: {}", game.score),
        10.0,
        17.0,
        18.0,
        WHITE,
        true,
    );
    ctx.set_text_align(TextAlign::Center);
    text(
        ctx,
        &format!("Time: {}", time(game.elapsed)),
        320.0,
        17.0,
        18.0,
        WHITE,
        true,
    );
    ctx.set_text_align(TextAlign::Right);
    text(
        ctx,
        &format!("Level: {}", game.level()),
        630.0,
        17.0,
        18.0,
        WHITE,
        true,
    );
}

fn draw_results(ctx: &mut Context<'_>, assets: &Assets, app: &App) {
    ctx.set_text_align(TextAlign::Center);
    text(ctx, "GAME OVER", 320.0, 118.0, 54.0, INK, true);
    text(ctx, "Play Time", 320.0, 174.0, 27.0, INK, true);
    text(ctx, &time(app.game.elapsed), 320.0, 203.0, 26.0, INK, true);
    text(ctx, "Score", 320.0, 247.0, 27.0, INK, true);
    text(
        ctx,
        &app.game.score.to_string(),
        320.0,
        276.0,
        26.0,
        INK,
        true,
    );
    if let Some(rank) = app.rank {
        text(
            ctx,
            &format!("Rank {rank} on the local high score table"),
            320.0,
            321.0,
            21.0,
            INK,
            true,
        );
        let count = 6_usize.saturating_sub(rank);
        for index in 0..count {
            let x = 320.0 + (index as f32 - count as f32 / 2.0) * 32.0;
            ctx.draw_image(&assets.star, x, 340.0, 32.0, 32.0);
        }
    }
}

fn draw_scores(ctx: &mut Context<'_>, app: &App) {
    if app.scores.is_empty() {
        text(
            ctx,
            "Play a round to set your first high score.",
            320.0,
            208.0,
            22.0,
            WHITE,
            true,
        );
        return;
    }
    let columns = [75.0, 232.0, 404.0, 557.0];
    for (label, x) in ["Rank", "Time", "Score", "Level"].into_iter().zip(columns) {
        text(ctx, label, x, 112.0, 20.0, WHITE, true);
    }
    for (index, score) in app.scores.iter().enumerate() {
        let row = [
            (index + 1).to_string(),
            time(score.elapsed),
            score.points.to_string(),
            (1 + score.elapsed.as_secs() / 10).to_string(),
        ];
        for (value, x) in row.iter().zip(columns) {
            text(
                ctx,
                value,
                x,
                140.0 + index as f32 * 26.0,
                18.0,
                WHITE,
                false,
            );
        }
    }
}

fn draw_button(ctx: &mut Context<'_>, app: &App, button: Button) {
    let rect = button.bounds;
    let active = app.hover == Some(button.action);
    let down = active && app.pressed == app.hover;
    let mut gradient = CanvasGradient::linear(0.0, rect.y, 0.0, rect.bottom());
    gradient.add_color_stop(0.0, Color::from_rgb(if down { 0xc6d6e0 } else { 0xf4f4f4 }));
    gradient.add_color_stop(1.0, Color::from_rgb(if down { 0xe2eaf0 } else { 0xcfcfcf }));
    ctx.begin_path();
    ctx.round_rect(rect.x, rect.y, rect.width, rect.height, 4.0);
    ctx.set_fill_style(gradient);
    ctx.fill();
    ctx.set_stroke_style(Color::from_rgb(if active { 0x1684b9 } else { 0x8c8c8c }));
    ctx.set_line_width(if active { 2.0 } else { 1.0 });
    ctx.stroke();
    ctx.begin_path();
    ctx.round_rect(
        rect.x + 1.5,
        rect.y + 1.5,
        rect.width - 3.0,
        rect.height - 3.0,
        2.5,
    );
    ctx.set_line_width(1.0);
    ctx.set_stroke_style(Color::rgba(255, 255, 255, 175));
    ctx.stroke();

    let label = if button.action == Action::Sound && app.muted {
        "Sound: Off"
    } else {
        button.label
    };
    ctx.set_text_align(TextAlign::Center);
    text(
        ctx,
        label,
        rect.x + rect.width / 2.0,
        rect.y + rect.height / 2.0,
        16.0,
        INK,
        false,
    );
}

fn block(ctx: &mut Context<'_>, rect: Rect, light_blue: bool, red: bool) {
    let rect = Rect::new(
        rect.x + 1.5,
        rect.y + 1.5,
        rect.width - 3.0,
        rect.height - 3.0,
    );
    let (body, highlight) = if red {
        (Color::rgba(255, 0, 0, 144), Color::rgba(255, 0, 0, 35))
    } else if light_blue {
        (
            Color::rgba(73, 119, 255, 223),
            Color::rgba(216, 226, 255, 247),
        )
    } else {
        (Color::rgba(0, 64, 255, 160), Color::rgba(0, 64, 255, 38))
    };
    let top = rect.y + 3.0;
    let mut gradient = CanvasGradient::linear(0.0, top, 0.0, top + 18.0);
    gradient.add_color_stop(0.0, highlight);
    gradient.add_color_stop(1.0, body);
    ctx.set_fill_style(body);
    ctx.fill_rect(rect.x, rect.y, rect.width, 3.0);
    ctx.fill_rect(rect.x, top, 3.0, rect.height - 3.0);
    ctx.fill_rect(rect.right() - 3.0, top, 3.0, rect.height - 3.0);
    ctx.set_fill_style(gradient);
    ctx.fill_rect(rect.x + 3.0, top, rect.width - 6.0, rect.height - 3.0);
}

fn fill_rect(ctx: &mut Context<'_>, rect: Rect) {
    ctx.fill_rect(rect.x, rect.y, rect.width, rect.height);
}

fn text(ctx: &mut Context<'_>, value: &str, x: f32, y: f32, size: f32, color: Color, bold: bool) {
    ctx.set_font("Arial", size);
    ctx.set_font_weight(if bold {
        FontWeight::Bold
    } else {
        FontWeight::Normal
    });
    ctx.set_fill_style(color);
    ctx.fill_text(value, x, y);
}

fn time(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}
