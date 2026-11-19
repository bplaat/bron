/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The native Rust edition of PlaatSoft's RedSquare.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::time::{Duration, Instant};

use bcanvas::CanvasBuilder;
use bwindow::{
    CursorIcon, Event, EventLoopBuilder, KeyCode, LogicalInsets, LogicalPoint as Point,
    LogicalSize, MouseButton, WindowBuilder, WindowEvent,
};
use preferences::{Preferences, Value};

use crate::game::{Game, SIZE, TICK};
use crate::sound::Sounds;
use crate::view::{Action, Assets, Screen};

mod game;
mod sound;
mod view;

const SCORE_LIMIT: usize = 10;

#[derive(Clone, Copy)]
struct Score {
    points: u32,
    elapsed: Duration,
}

#[derive(Clone, Copy)]
struct Layout {
    origin: Point,
    scale: f32,
}

impl Layout {
    fn new(size: LogicalSize, insets: LogicalInsets) -> Self {
        let width = (size.width - insets.left - insets.right).max(0.0);
        let height = (size.height - insets.top - insets.bottom).max(0.0);
        let scale = (width / SIZE.width).min(height / SIZE.height).max(0.001);
        Self {
            origin: Point::new(
                insets.left + (width - SIZE.width * scale) / 2.0,
                insets.top + (height - SIZE.height * scale) / 2.0,
            ),
            scale,
        }
    }

    const fn point(self, point: Point) -> Point {
        Point::new(
            (point.x - self.origin.x) / self.scale,
            (point.y - self.origin.y) / self.scale,
        )
    }
}

struct App {
    screen: Screen,
    game: Game,
    prefs: Option<Preferences>,
    scores: Vec<Score>,
    rank: Option<usize>,
    sounds: Option<Sounds>,
    muted: bool,
    hover: Option<Action>,
    pressed: Option<Action>,
    last_frame: Option<Instant>,
    remainder: Duration,
    focused: bool,
}

impl App {
    fn new() -> Self {
        let prefs = Preferences::new("nl.bplaat.RedSquare").ok();
        let get = |key| {
            prefs
                .as_ref()
                .and_then(|prefs| prefs.get(key).ok().flatten())
        };
        let muted = matches!(get("muted"), Some(Value::Bool(true)));
        let scores = match get("scores") {
            Some(Value::String(data)) => data
                .lines()
                .filter_map(|line| {
                    let (points, millis) = line.split_once(',')?;
                    Some(Score {
                        points: points.parse().ok()?,
                        elapsed: Duration::from_millis(millis.parse().ok()?),
                    })
                })
                .take(SCORE_LIMIT)
                .collect(),
            _ => Vec::new(),
        };
        Self {
            screen: Screen::Home,
            game: Game::demo(),
            prefs,
            scores,
            rank: None,
            sounds: Sounds::new(),
            muted,
            hover: None,
            pressed: None,
            last_frame: None,
            remainder: Duration::ZERO,
            focused: true,
        }
    }

    fn save(&self, key: &str, value: impl Into<Value>) {
        if let Some(prefs) = &self.prefs
            && let Err(error) = prefs.set(key, value)
        {
            eprintln!("Failed to save RedSquare preferences: {error}");
        }
    }

    fn change_screen(&mut self, screen: Screen) {
        self.screen = screen;
        self.game.release();
        self.hover = None;
        self.pressed = None;
        self.last_frame = Some(Instant::now());
        self.remainder = Duration::ZERO;
    }

    fn start(&mut self) {
        self.game = Game::new();
        self.rank = None;
        self.change_screen(Screen::Playing);
        self.play(|sounds| &sounds.start);
    }

    fn play(&self, sound: impl FnOnce(&Sounds) -> &baudio::Sound) {
        if !self.muted
            && let Some(sounds) = &self.sounds
        {
            sounds.play(sound(sounds));
        }
    }

    fn finish(&mut self) {
        self.game.lose();
        self.change_screen(Screen::Results);
        self.play(|sounds| &sounds.game_over);
        let score = Score {
            points: self.game.score,
            elapsed: self.game.elapsed,
        };
        let index = self
            .scores
            .partition_point(|entry| entry.points >= score.points);
        if index < SCORE_LIMIT {
            self.rank = Some(index + 1);
            self.scores.insert(index, score);
            self.scores.truncate(SCORE_LIMIT);
            let mut data = String::new();
            for entry in &self.scores {
                use std::fmt::Write as _;
                let points = entry.points;
                let millis = entry.elapsed.as_millis();
                let _ = writeln!(data, "{points},{millis}");
            }
            self.save("scores", data);
        }
    }

    fn action(&mut self, action: Action) {
        match action {
            Action::Play => self.start(),
            Action::Home => {
                self.game = Game::demo();
                self.change_screen(Screen::Home);
            }
            Action::Scores => self.change_screen(Screen::Scores),
            Action::Help => self.change_screen(Screen::Help),
            Action::Credits => self.change_screen(Screen::Credits),
            Action::Settings => self.change_screen(Screen::Settings),
            Action::Sound => {
                self.muted = !self.muted;
                self.save("muted", self.muted);
                self.play(|sounds| &sounds.bounce);
            }
            Action::Quit => {}
        }
    }

    const fn animating(&self) -> bool {
        self.focused && matches!(self.screen, Screen::Home | Screen::Playing)
    }

    fn tick(&mut self, now: Instant) {
        if !self.animating() {
            return;
        }
        let previous = self.last_frame.replace(now).unwrap_or(now);
        // Drop long gaps caused by system suspend or a blocked event loop.
        self.remainder += now.duration_since(previous).min(Duration::from_millis(100));
        let mut bounced = false;
        while self.remainder >= TICK {
            self.remainder -= TICK;
            if self.screen == Screen::Home {
                self.game.move_blocks(1.0);
                continue;
            }
            bounced |= self.game.step();
            if self.game.lost {
                self.finish();
                return;
            }
        }
        if bounced {
            self.play(|sounds| &sounds.bounce);
        }
    }

    fn update_hover(&mut self, point: Point) {
        self.hover = self.screen.hit(point);
    }

    fn cursor(&self, point: Option<Point>) -> CursorIcon {
        if self.hover.is_some()
            || self.screen == Screen::Playing
                && (self.game.dragging()
                    || point.is_some_and(|point| self.game.player.contains(point)))
        {
            CursorIcon::Pointer
        } else {
            CursorIcon::Default
        }
    }
}

fn main() {
    let event_loop = EventLoopBuilder::new()
        .app_id("nl", "bplaat", "RedSquare")
        .build();
    let mut window = WindowBuilder::new()
        .title("RedSquare")
        .size(SIZE)
        .min_size(LogicalSize::new(320.0, 240.0))
        .background_color(0x282828)
        .center()
        .build();
    let mut canvas = CanvasBuilder::new(&window).build();
    let mut app = App::new();
    let assets = Assets::new();
    let mut layout = Layout::new(window.size(), window.safe_area());
    let mut pointer = None;
    window.request_animation_frame();

    event_loop.run(move |event| {
        let Event::Window(_, event) = event else {
            return;
        };
        match event {
            WindowEvent::RedrawRequested => {
                let insets = window.safe_area();
                let _ = canvas.draw(|context| {
                    app.tick(Instant::now());
                    layout =
                        Layout::new(LogicalSize::new(context.width(), context.height()), insets);
                    view::draw(context, &assets, &app, layout);
                });
                window.set_cursor(app.cursor(pointer));
                if app.animating() {
                    window.request_animation_frame();
                }
                return;
            }
            WindowEvent::Resize(_) | WindowEvent::SafeAreaChanged(_) => {
                layout = Layout::new(window.size(), window.safe_area());
                pointer = None;
                app.hover = None;
                app.pressed = None;
                app.game.release();
            }
            WindowEvent::Focus => {
                app.focused = true;
                app.last_frame = None;
            }
            WindowEvent::Blur => {
                app.focused = false;
                app.last_frame = None;
                pointer = None;
                app.hover = None;
                app.pressed = None;
                app.game.release();
            }
            WindowEvent::MouseMove { position, .. } => {
                let point = layout.point(position);
                pointer = Some(point);
                app.update_hover(point);
                app.game.drag(point);
            }
            WindowEvent::MouseDown(event) if event.button == MouseButton::Left => {
                let point = layout.point(event.position);
                pointer = Some(point);
                app.update_hover(point);
                app.pressed = app.hover;
                if app.screen == Screen::Playing {
                    app.game.grab(point);
                }
            }
            WindowEvent::MouseUp(event) if event.button == MouseButton::Left => {
                let point = layout.point(event.position);
                pointer = Some(point);
                app.game.drag(point);
                app.game.release();
                app.update_hover(point);
                if let Some(action) = app
                    .pressed
                    .take()
                    .filter(|action| Some(*action) == app.hover)
                {
                    if action == Action::Quit {
                        window.close();
                        return;
                    }
                    app.action(action);
                }
            }
            WindowEvent::MouseLeave => {
                pointer = None;
                app.hover = None;
                app.pressed = None;
                if app.game.dragging() {
                    app.finish();
                }
            }
            WindowEvent::KeyDown(event) if !event.repeat => match event.code {
                Some(KeyCode::Enter | KeyCode::Space)
                    if matches!(app.screen, Screen::Home | Screen::Results) =>
                {
                    app.start()
                }
                Some(KeyCode::KeyR) if matches!(app.screen, Screen::Playing | Screen::Results) => {
                    app.start();
                }
                Some(KeyCode::Escape) if app.screen != Screen::Home => app.action(Action::Home),
                _ => return,
            },
            _ => return,
        }
        window.set_cursor(app.cursor(pointer));
        window.request_redraw();
    });
}
