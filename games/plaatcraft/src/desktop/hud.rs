/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use bcanvas::{CanvasRenderingContext2d, Color, OffscreenCanvas, TextAlign, TextBaseline};
use bwindow::LogicalInsets;

const STATS_WIDTH: f32 = 360.0;
const STATS_INTERVAL: Duration = Duration::from_millis(250);
const STATS_LINES: u32 = 6;
const LINE_HEIGHT: f32 = 18.0;
pub(crate) const MARGIN: f32 = 16.0;

// A cached drawing of the HUD layer, recorded in logical pixels and drawn enlarged by its
// zoom, so it stays sharp at every display scale.
pub(crate) struct Sprite {
    canvas: OffscreenCanvas,
    zoom: f32,
}

impl Sprite {
    pub(crate) fn new(width: f32, height: f32, zoom: f32) -> Self {
        Self {
            canvas: OffscreenCanvas::new_recording(width, height),
            zoom,
        }
    }

    // The logical size it is drawn at.
    pub(crate) fn size(&self) -> (f32, f32) {
        (
            self.canvas.logical_width() * self.zoom,
            self.canvas.logical_height() * self.zoom,
        )
    }

    // Records the drawing again.
    pub(crate) fn redraw(&mut self, draw: impl FnOnce(&mut CanvasRenderingContext2d<'_>)) {
        self.canvas = OffscreenCanvas::new_recording(
            self.canvas.logical_width(),
            self.canvas.logical_height(),
        );
        self.canvas.draw(draw);
    }

    // Draws it with its top left corner at a logical position, kept inside the layer.
    pub(crate) fn draw(&self, ctx: &mut CanvasRenderingContext2d<'_>, (x, y): (f32, f32)) {
        let (width, height) = self.size();
        if width > ctx.width() || height > ctx.height() {
            return;
        }
        let x = x.round().clamp(0.0, ctx.width() - width);
        let y = y.round().clamp(0.0, ctx.height() - height);
        ctx.draw_canvas(&self.canvas, x, y, width, height);
    }

    // Stretches it over the whole layer, for a uniform overlay.
    pub(crate) fn fill(&self, ctx: &mut CanvasRenderingContext2d<'_>) {
        ctx.draw_canvas(&self.canvas, 0.0, 0.0, ctx.width(), ctx.height());
    }
}

// Multi-line stats text in the top right corner.
pub(crate) struct Stats {
    sprite: Sprite,
    lines: Vec<String>,
    drawn: Instant,
}

impl Default for Stats {
    fn default() -> Self {
        let height = STATS_LINES as f32 * LINE_HEIGHT + MARGIN / 2.0;
        Self {
            sprite: Sprite::new(STATS_WIDTH, height, 1.0),
            lines: Vec::new(),
            drawn: Instant::now(),
        }
    }
}

impl Stats {
    // Redraws the text only when the lines changed.
    pub(crate) fn update(&mut self, lines: Vec<String>) {
        // The position changes every frame while moving, so redraw a few times a second.
        if self.lines == lines || !self.lines.is_empty() && self.drawn.elapsed() < STATS_INTERVAL {
            return;
        }
        self.lines = lines;
        self.drawn = Instant::now();
        self.sprite.redraw(|ctx| {
            ctx.set_font("sans-serif", 14.0);
            ctx.set_text_align(TextAlign::Right);
            ctx.set_text_baseline(TextBaseline::Top);
            let x = ctx.width() - MARGIN / 2.0;
            for (index, line) in self.lines.iter().enumerate() {
                let y = MARGIN / 4.0 + index as f32 * LINE_HEIGHT;
                // A dark offset copy keeps the text readable over bright sky and terrain.
                ctx.set_fill_style(Color::rgba(0, 0, 0, 160));
                ctx.fill_text(line, x + 1.0, y + 1.0);
                ctx.set_fill_style(Color::rgb(255, 255, 255));
                ctx.fill_text(line, x, y);
            }
        });
    }

    pub(crate) fn draw(&self, ctx: &mut CanvasRenderingContext2d<'_>, safe_area: LogicalInsets) {
        let x = ctx.width() - safe_area.right - MARGIN / 2.0 - STATS_WIDTH;
        self.sprite.draw(ctx, (x, safe_area.top + MARGIN / 2.0));
    }
}

// A sprite that is only redrawn when the key of what it shows changes.
pub(crate) struct Cached {
    key: [u32; 5],
    zoom: f32,
    sprite: Sprite,
}

impl Cached {
    pub(crate) const fn sprite(&self) -> &Sprite {
        &self.sprite
    }
}

// Creates and redraws cached sprites enlarged by a zoom.
#[derive(Clone, Copy)]
pub(crate) struct Painter {
    pub(crate) zoom: f32,
}

impl Painter {
    // Redraws a sprite when its key changed, the first two key values set its size.
    pub(crate) fn paint(
        self,
        cached: &mut Option<Cached>,
        key: [u32; 5],
        (width, height): (f32, f32),
        draw: impl FnOnce(&mut CanvasRenderingContext2d<'_>),
    ) {
        if cached
            .as_ref()
            .is_some_and(|cached| cached.key == key && cached.zoom == self.zoom)
        {
            return;
        }
        let mut sprite = Sprite::new(width, height, self.zoom);
        sprite.redraw(draw);
        *cached = Some(Cached {
            key,
            zoom: self.zoom,
            sprite,
        });
    }
}

const CHAT_TIME: Duration = Duration::from_secs(10);
const CHAT_HISTORY: usize = 50;
const CHAT_RECENT: usize = 5;
const CHAT_OPEN: usize = 10;
const CHAT_WIDTH: f32 = 440.0;
const CHAT_LINE: f32 = 20.0;
// The chat stays above the hotbar.
const CHAT_BOTTOM: f32 = 84.0;

struct ChatLine {
    // Who said it, world messages have no name.
    name: Option<String>,
    text: String,
    received: Instant,
}

// The chat in the bottom left: recent messages, and all of them with an input bar while
// typing.
#[derive(Default)]
pub(crate) struct Chat {
    lines: VecDeque<ChatLine>,
    input: Option<String>,
    cached: Option<Cached>,
}

impl Chat {
    pub(crate) fn push(&mut self, name: Option<String>, text: String) {
        if self.lines.len() == CHAT_HISTORY {
            self.lines.pop_front();
        }
        self.lines.push_back(ChatLine {
            name,
            text,
            received: Instant::now(),
        });
    }

    pub(crate) fn clear(&mut self) {
        self.lines.clear();
        self.input = None;
    }

    pub(crate) const fn typing(&self) -> bool {
        self.input.is_some()
    }

    pub(crate) fn open(&mut self, text: &str) {
        self.input = Some(text.to_owned());
    }

    pub(crate) fn close(&mut self) {
        self.input = None;
    }

    pub(crate) fn text(&self) -> &str {
        self.input.as_deref().unwrap_or_default()
    }

    pub(crate) fn backspace(&mut self) {
        if let Some(input) = &mut self.input {
            input.pop();
        }
    }

    pub(crate) fn type_text(&mut self, text: &str, length: usize) {
        if let Some(input) = &mut self.input {
            for c in text.chars().filter(|c| !c.is_control()) {
                if input.chars().count() < length {
                    input.push(c);
                }
            }
        }
    }

    pub(crate) fn prepare(&mut self, painter: Painter) {
        let typing = self.typing();
        let shown: Vec<_> = if typing {
            self.lines.iter().rev().take(CHAT_OPEN).rev().collect()
        } else {
            let recent: Vec<_> = self
                .lines
                .iter()
                .filter(|line| line.received.elapsed() < CHAT_TIME)
                .collect();
            recent[recent.len().saturating_sub(CHAT_RECENT)..].to_vec()
        };
        if shown.is_empty() && !typing {
            self.cached = None;
            return;
        }
        let mut hasher = std::hash::DefaultHasher::new();
        for line in &shown {
            std::hash::Hash::hash(&(&line.name, &line.text), &mut hasher);
        }
        std::hash::Hash::hash(&self.input, &mut hasher);
        let hash = std::hash::Hasher::finish(&hasher);
        let rows = shown.len() + usize::from(typing);
        let height = rows as f32 * CHAT_LINE;
        let input = self.input.as_deref();
        painter.paint(
            &mut self.cached,
            [height.to_bits(), 0, hash as u32, (hash >> 32) as u32, 0],
            (CHAT_WIDTH, height),
            |ctx| {
                ctx.set_font("sans-serif", 14.0);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                for (row, line) in shown.iter().enumerate() {
                    let y = row as f32 * CHAT_LINE;
                    // Wii players type tabs, which show as four spaces
                    let text = line.text.replace('\t', "    ");
                    let text = match &line.name {
                        Some(name) => format!("<{name}> {text}"),
                        None => text,
                    };
                    let width = (ctx.measure_text(&text).width + 12.0).min(CHAT_WIDTH);
                    ctx.set_fill_style(Color::rgba(0, 0, 0, 120));
                    ctx.fill_rect(0.0, y, width, CHAT_LINE);
                    ctx.set_fill_style(if line.name.is_some() {
                        Color::rgb(255, 255, 255)
                    } else {
                        Color::rgb(255, 255, 160)
                    });
                    ctx.fill_text(&text, 6.0, y + CHAT_LINE / 2.0);
                }
                if let Some(input) = input {
                    let y = shown.len() as f32 * CHAT_LINE;
                    ctx.set_fill_style(Color::rgba(0, 0, 0, 170));
                    ctx.fill_rect(0.0, y, CHAT_WIDTH, CHAT_LINE);
                    ctx.set_fill_style(Color::rgb(255, 255, 255));
                    ctx.fill_text(format!("> {input}_"), 6.0, y + CHAT_LINE / 2.0);
                }
            },
        );
    }

    pub(crate) fn draw(&self, ctx: &mut CanvasRenderingContext2d<'_>, safe_area: LogicalInsets) {
        if let Some(cached) = &self.cached {
            let (_, height) = cached.sprite.size();
            let y = ctx.height() - safe_area.bottom - CHAT_BOTTOM - height;
            cached.sprite.draw(ctx, (safe_area.left + MARGIN / 2.0, y));
        }
    }
}

const TAG_WIDTH: f32 = 200.0;
const TAG_HEIGHT: f32 = 24.0;

// The names above the other players.
#[derive(Default)]
pub(crate) struct NameTags {
    cached: HashMap<String, Cached>,
}

impl NameTags {
    pub(crate) fn prepare<'a>(&mut self, painter: Painter, names: impl Iterator<Item = &'a str>) {
        let names: Vec<_> = names.collect();
        self.cached.retain(|name, _| names.contains(&name.as_str()));
        for name in names {
            let mut cached = self.cached.remove(name);
            painter.paint(&mut cached, [0; 5], (TAG_WIDTH, TAG_HEIGHT), |ctx| {
                ctx.set_font("sans-serif", 14.0);
                ctx.set_text_align(TextAlign::Center);
                ctx.set_text_baseline(TextBaseline::Middle);
                let width = ctx.measure_text(name).width.min(TAG_WIDTH - 12.0) + 12.0;
                ctx.set_fill_style(Color::rgba(0, 0, 0, 110));
                ctx.fill_rect((TAG_WIDTH - width) / 2.0, 0.0, width, TAG_HEIGHT);
                ctx.set_fill_style(Color::rgb(255, 255, 255));
                ctx.fill_text(name, TAG_WIDTH / 2.0, TAG_HEIGHT / 2.0);
            });
            if let Some(cached) = cached {
                self.cached.insert(name.to_owned(), cached);
            }
        }
    }

    // Draws the tags centered above logical positions.
    pub(crate) fn draw(&self, ctx: &mut CanvasRenderingContext2d<'_>, tags: &[(&str, [f32; 2])]) {
        for (name, [x, y]) in tags {
            if let Some(cached) = self.cached.get(*name) {
                let position = (x - TAG_WIDTH / 2.0, y - TAG_HEIGHT);
                if position.0 >= 0.0 && position.1 >= 0.0 && position.0 + TAG_WIDTH <= ctx.width() {
                    cached.sprite.draw(ctx, position);
                }
            }
        }
    }
}
