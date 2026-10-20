/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::f32::consts::TAU;

use bcanvas::{
    CanvasBuilder, CanvasRenderingContext2d, Color, CursorIcon, FontWeight, TextAlign, TextBaseline,
};
use bwindow::{
    Accelerator, Event, EventLoopBuilder, Key, KeyCode, LogicalPoint, LogicalSize, MenuBarBuilder,
    MenuBuilder, MenuItem, Modifiers, MouseButton, MouseEvent, Theme, WindowBuilder, WindowEvent,
};
use reversi_engine::{CellState, Move, Othello, Player};

use crate::worker::{AiJob, AiResult, AiWorker};

mod worker;

const WINDOW_WIDTH: f32 = 600.0;
const WINDOW_HEIGHT: f32 = 600.0;
const MIN_WINDOW_WIDTH: f32 = 360.0;
const MIN_WINDOW_HEIGHT: f32 = 360.0;
const GRID_OFFSET: f32 = 31.0;
const CELL: f32 = 52.0;
const CELL_GAP: f32 = 2.0;
const CELL_STEP: f32 = CELL + CELL_GAP;
const BOARD_X: f32 = GRID_OFFSET + CELL_STEP;
const BOARD_Y: f32 = GRID_OFFSET + CELL_STEP;
const BOARD_BACKGROUND_X: f32 = GRID_OFFSET + CELL;
const BOARD_BACKGROUND_Y: f32 = GRID_OFFSET + CELL;
const BOARD_BACKGROUND_SIZE: f32 = CELL * 8.0 + CELL_GAP * 9.0;

#[derive(Clone, Copy)]
struct Palette {
    background: u32,
    text: Color,
    grid: Color,
    board: Color,
    board_hover: Color,
    move_hint: Color,
}

impl Palette {
    const fn for_theme(theme: Theme) -> Self {
        match theme {
            Theme::Light => Self {
                background: 0xededed,
                text: Color::rgb(18, 18, 18),
                grid: Color::rgb(20, 20, 20),
                board: Color::rgb(27, 137, 86),
                board_hover: Color::rgb(37, 165, 105),
                move_hint: Color::rgba(4, 49, 31, 130),
            },
            Theme::Dark => Self {
                background: 0x1b201d,
                text: Color::rgb(235, 240, 237),
                grid: Color::rgb(72, 79, 75),
                board: Color::rgb(24, 126, 80),
                board_hover: Color::rgb(34, 151, 97),
                move_hint: Color::rgba(210, 239, 226, 155),
            },
        }
    }
}

#[derive(Clone)]
struct Game {
    board: Othello,
    turn: Player,
    hover: Option<(usize, usize)>,
    game_over: bool,
    window_size: LogicalSize,
    top_inset: f32,
    theme: Theme,
    revision: u64,
    thinking: bool,
}
impl Default for Game {
    fn default() -> Self {
        Self::new(Theme::Light, 0.0)
    }
}

impl Game {
    fn set_hover(&mut self, hover: Option<(usize, usize)>) -> bool {
        let previous = self.interactive_hover();
        self.hover = hover;
        previous != self.interactive_hover()
    }

    fn new(theme: Theme, top_inset: f32) -> Self {
        Self {
            board: Othello::default(),
            turn: Player::Black,
            hover: None,
            game_over: false,
            window_size: LogicalSize::new(WINDOW_WIDTH, WINDOW_HEIGHT),
            top_inset,
            theme,
            revision: 0,
            thinking: false,
        }
    }
}

#[derive(Clone, Copy)]
struct GameLayout {
    scale: f32,
    offset_x: f32,
    offset_y: f32,
    window_size: LogicalSize,
}

impl GameLayout {
    fn new(window_size: LogicalSize, top_inset: f32) -> Self {
        let top_inset = top_inset.clamp(0.0, window_size.height);
        let usable_height = window_size.height - top_inset;
        let scale = (window_size.width / WINDOW_WIDTH)
            .min(usable_height / WINDOW_HEIGHT)
            .max(0.01);
        Self {
            scale,
            offset_x: (window_size.width - WINDOW_WIDTH * scale) / 2.0,
            offset_y: top_inset + (usable_height - WINDOW_HEIGHT * scale) / 2.0,
            window_size,
        }
    }

    fn game_point(self, event: &LogicalPoint) -> Option<(f32, f32)> {
        let x = (event.x - self.offset_x) / self.scale;
        let y = (event.y - self.offset_y) / self.scale;
        ((0.0..WINDOW_WIDTH).contains(&x) && (0.0..WINDOW_HEIGHT).contains(&y)).then_some((x, y))
    }
}

impl Game {
    fn play(&mut self, row: usize, col: usize) -> bool {
        if !self.board.make_move(self.turn, Move { row, col }) {
            return false;
        }
        self.turn = self.turn.other();
        self.revision = self
            .revision
            .checked_add(1)
            .expect("game revision exhausted");
        self.advance_passes();
        true
    }

    fn interactive_hover(&self) -> Option<(usize, usize)> {
        self.hover.filter(|&(row, col)| {
            !self.game_over
                && !self.thinking
                && self.turn == Player::Black
                && self.board.is_valid_move(Player::Black, Move { row, col })
        })
    }

    fn cursor(&self) -> CursorIcon {
        if self.thinking {
            CursorIcon::Progress
        } else if self.interactive_hover().is_some() {
            CursorIcon::Pointer
        } else {
            CursorIcon::Default
        }
    }
    fn advance_passes(&mut self) {
        if !self.board.has_valid_move(self.turn) {
            self.turn = self.turn.other();
            if !self.board.has_valid_move(self.turn) {
                self.game_over = true
            }
        }
    }
    fn ai_job(&mut self) -> Option<AiJob> {
        if self.turn != Player::White || self.game_over || self.thinking {
            return None;
        }
        self.thinking = true;
        Some(AiJob {
            revision: self.revision,
            board: self.board,
        })
    }

    fn apply_ai(&mut self, result: AiResult) -> bool {
        if !self.thinking
            || self.game_over
            || self.turn != Player::White
            || result.job.revision != self.revision
            || result.job.board != self.board
        {
            return false;
        }
        self.thinking = false;
        if let Some(movement) = result.movement {
            self.play(movement.row, movement.col)
        } else {
            self.advance_passes();
            true
        }
    }

    fn reset(&mut self) {
        let revision = self
            .revision
            .checked_add(1)
            .expect("game revision exhausted");
        *self = Self {
            revision,
            window_size: self.window_size,
            ..Self::new(self.theme, self.top_inset)
        };
    }

    fn status(&self) -> String {
        if self.game_over {
            let (b, w) = (
                self.board.score(Player::Black),
                self.board.score(Player::White),
            );
            if b > w {
                format!("Human wins {b}-{w}!")
            } else if w > b {
                format!("Computer wins {w}-{b}!")
            } else {
                "Draw!".into()
            }
        } else if self.turn == Player::Black {
            "Human's move.".into()
        } else {
            "Computer's move...".into()
        }
    }
}

enum AppEvent {
    Ai(AiResult),
}

fn main() {
    let smoke = std::env::args().any(|arg| arg == "--smoke");
    let event_loop = EventLoopBuilder::new()
        .with_user_event::<AppEvent>()
        .app_id("nl", "bplaat", "Reversi")
        .menu(
            MenuBarBuilder::new().menu(
                MenuBuilder::new("File").item(
                    MenuItem::new("New Game", "game.new")
                        .accelerator(Accelerator::new(Modifiers::PRIMARY, KeyCode::KeyN)),
                ),
            ),
        )
        .build();
    let worker = AiWorker::new(event_loop.create_proxy());
    let theme = event_loop.theme();
    let window_builder = WindowBuilder::new()
        .title("Reversi")
        .size(LogicalSize::new(WINDOW_WIDTH, WINDOW_HEIGHT))
        .min_size(LogicalSize::new(MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT))
        .background_color(Palette::for_theme(theme).background)
        .center();
    let window_builder = if smoke {
        window_builder
    } else {
        window_builder.remember_window_state()
    };
    let mut window = window_builder.build();
    let window_id = window.id();
    #[cfg(target_os = "macos")]
    if smoke {
        window.set_theme(Theme::Light);
    }
    let mut canvas = CanvasBuilder::new(&window).build();
    let mut game = Game::new(theme, 0.0);
    let completed = std::rc::Rc::new(std::cell::Cell::new(false));
    let mut smoke_started = false;
    let mut smoke_ai_done = false;
    let mut showed_progress_cursor = false;
    #[cfg(target_os = "macos")]
    let mut smoke_themes = 0;
    if smoke {
        let watchdog = event_loop.create_proxy();
        std::thread::Builder::new()
            .name("smoke-watchdog".to_string())
            .spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(15));
                let _ = watchdog.exit();
            })
            .expect("Can't spawn smoke watchdog thread");
    }
    event_loop.run(move |event| {
        match event {
            Event::Window(id, WindowEvent::RedrawRequested) if id == window_id => {
                assert!(canvas.draw(|context| {
                    game.window_size = LogicalSize::new(context.width(), context.height());
                    draw(
                        context,
                        &game,
                        GameLayout::new(game.window_size, game.top_inset),
                    );
                }));
                if smoke && !smoke_started {
                    #[cfg(target_os = "macos")]
                    window.set_theme(Theme::Dark);
                    // Exercise cancellation/reset while one search is queued.
                    assert!(game.play(2, 3));
                    worker.submit(game.ai_job().expect("AI job"));
                    game.reset();
                    worker.cancel(game.revision);
                    assert!(game.play(2, 3));
                    smoke_started = true;
                } else if smoke && smoke_ai_done {
                    assert!(showed_progress_cursor, "loading cursor was never set");
                    #[cfg(target_os = "macos")]
                    if smoke_themes != 3 {
                        window.request_redraw();
                        return;
                    }
                    assert!(game.board.score(Player::Black) + game.board.score(Player::White) >= 6);
                    completed.set(true);
                    window.close();
                }
            }
            Event::Window(_, WindowEvent::Resize(size)) => {
                game.window_size = size;
                game.hover = None;
                window.request_redraw();
            }
            Event::Window(_, WindowEvent::ThemeChanged(theme)) => {
                game.theme = theme;
                window.set_background_color(Palette::for_theme(theme).background);
                window.request_redraw();
                #[cfg(target_os = "macos")]
                if smoke && smoke_started {
                    match theme {
                        Theme::Dark => {
                            smoke_themes |= 1;
                            window.set_theme(Theme::Light);
                        }
                        Theme::Light => smoke_themes |= 2,
                    }
                }
            }
            Event::Window(_, WindowEvent::MouseMove { position, .. }) => {
                let hover =
                    board_cell(&position, GameLayout::new(game.window_size, game.top_inset));
                if game.set_hover(hover) {
                    window.request_redraw();
                }
            }
            Event::Window(_, WindowEvent::MouseLeave) => {
                if game.set_hover(None) {
                    window.request_redraw();
                }
            }
            Event::Window(
                _,
                WindowEvent::MouseDown(MouseEvent {
                    button: MouseButton::Left,
                    position,
                    ..
                }),
            ) => {
                if game.turn == Player::Black
                    && !game.game_over
                    && let Some((row, col)) =
                        board_cell(&position, GameLayout::new(game.window_size, game.top_inset))
                    && game.play(row, col)
                {
                    window.request_redraw();
                }
            }
            Event::Window(_, WindowEvent::KeyDown(event))
                if !event.repeat
                    && matches!(&event.key,Key::Character(key) if key.eq_ignore_ascii_case("r")) =>
            {
                game.reset();
                worker.cancel(game.revision);
                window.request_redraw();
            }
            Event::Action(action) if action == "game.new" => {
                game.reset();
                worker.cancel(game.revision);
                window.request_redraw();
            }
            Event::UserEvent(AppEvent::Ai(result)) => {
                if game.apply_ai(result) {
                    smoke_ai_done = true;
                    window.request_redraw();
                }
            }
            Event::LoopExiting if smoke => {
                assert!(completed.get(), "Reversi smoke test did not finish");
            }
            _ => {}
        }
        if !window.is_closed()
            && let Some(job) = game.ai_job()
        {
            worker.submit(job);
            window.request_redraw();
        }
        let cursor = game.cursor();
        canvas.set_cursor(cursor);
        showed_progress_cursor |= cursor == CursorIcon::Progress;
    });
}

fn board_cell(event: &LogicalPoint, layout: GameLayout) -> Option<(usize, usize)> {
    let (x, y) = layout.game_point(event)?;
    let (x, y) = (x - BOARD_X, y - BOARD_Y);
    if x < 0.0 || y < 0.0 || x % CELL_STEP >= CELL || y % CELL_STEP >= CELL {
        return None;
    }
    let col = (x / CELL_STEP).floor() as isize;
    let row = (y / CELL_STEP).floor() as isize;
    ((0..8).contains(&row) && (0..8).contains(&col)).then_some((row as usize, col as usize))
}

fn board_grid_rects() -> impl Iterator<Item = (f32, f32, f32, f32)> {
    (0..9).flat_map(|line| {
        let offset = line as f32 * CELL_STEP;
        [
            (
                BOARD_BACKGROUND_X + offset,
                BOARD_BACKGROUND_Y,
                CELL_GAP,
                BOARD_BACKGROUND_SIZE,
            ),
            (
                BOARD_BACKGROUND_X,
                BOARD_BACKGROUND_Y + offset,
                BOARD_BACKGROUND_SIZE,
                CELL_GAP,
            ),
        ]
    })
}

fn draw(ctx: &mut CanvasRenderingContext2d<'_>, game: &Game, layout: GameLayout) {
    let palette = Palette::for_theme(game.theme);
    ctx.set_fill_style(Color::from_rgb(palette.background));
    ctx.fill_rect(
        0.0,
        0.0,
        layout.window_size.width,
        layout.window_size.height,
    );
    ctx.save();
    ctx.translate(layout.offset_x, layout.offset_y);
    ctx.scale(layout.scale, layout.scale);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_font("sans-serif", 19.0);
    ctx.set_font_weight(FontWeight::Bold);
    ctx.set_fill_style(palette.text);
    for col in 0..8 {
        ctx.fill_text(
            ["A", "B", "C", "D", "E", "F", "G", "H"][col],
            BOARD_X + col as f32 * CELL_STEP + CELL / 2.0,
            GRID_OFFSET + CELL / 2.0,
        );
    }
    for row in 0..8 {
        ctx.fill_text(
            ["1", "2", "3", "4", "5", "6", "7", "8"][row],
            GRID_OFFSET + CELL / 2.0,
            BOARD_Y + row as f32 * CELL_STEP + CELL / 2.0,
        );
    }

    // One board fill and grid lines replace 64 individual cell backgrounds.
    ctx.set_fill_style(palette.board);
    ctx.fill_rect(
        BOARD_BACKGROUND_X,
        BOARD_BACKGROUND_Y,
        BOARD_BACKGROUND_SIZE,
        BOARD_BACKGROUND_SIZE,
    );
    ctx.set_fill_style(palette.grid);
    for (x, y, width, height) in board_grid_rects() {
        ctx.fill_rect(x, y, width, height);
    }
    let legal_moves = if game.turn == Player::Black {
        game.board.valid_move_bits(Player::Black)
    } else {
        0
    };
    let interactive_hover = game.interactive_hover();
    if let Some((row, col)) = interactive_hover {
        ctx.set_fill_style(palette.board_hover);
        ctx.fill_rect(
            BOARD_X + col as f32 * CELL_STEP,
            BOARD_Y + row as f32 * CELL_STEP,
            CELL,
            CELL,
        );
    }
    for row in 0..8 {
        for col in 0..8 {
            let x = BOARD_X + col as f32 * CELL_STEP;
            let y = BOARD_Y + row as f32 * CELL_STEP;
            let (cx, cy) = (x + CELL / 2.0, y + CELL / 2.0);
            let cell = game.board.cell_state(row, col);
            if cell != CellState::Empty {
                draw_disc(ctx, cx, cy, cell);
            } else if legal_moves & (1_u64 << (row * 8 + col)) != 0 {
                circle(
                    ctx,
                    cx,
                    cy,
                    if interactive_hover == Some((row, col)) {
                        CELL * 0.12
                    } else {
                        CELL * 0.075
                    },
                    palette.move_hint,
                );
            }
        }
    }
    ctx.set_fill_style(palette.text);
    ctx.fill_text(
        game.status(),
        WINDOW_WIDTH / 2.0,
        GRID_OFFSET + CELL * 9.5 + CELL_GAP * 9.0,
    );
    ctx.restore();
}

fn draw_disc(ctx: &mut CanvasRenderingContext2d<'_>, cx: f32, cy: f32, cell: CellState) {
    let radius = CELL * 0.46;

    // The game builds its small contact shadow from the same path primitives
    // as every other shape; Canvas itself only owns drawing fundamentals.
    circle(ctx, cx, cy + 2.0, radius, Color::rgba(0, 0, 0, 80));
    match cell {
        CellState::Black => {
            circle(ctx, cx, cy, radius, Color::rgb(8, 8, 10));
            circle(
                ctx,
                cx - 1.2,
                cy - 1.8,
                radius * 0.91,
                Color::rgb(31, 31, 34),
            );
            circle(
                ctx,
                cx - radius * 0.25,
                cy - radius * 0.3,
                radius * 0.24,
                Color::rgba(255, 255, 255, 24),
            );
        }
        CellState::White => {
            circle(ctx, cx, cy, radius, Color::rgb(174, 174, 178));
            circle(
                ctx,
                cx - 1.0,
                cy - 1.8,
                radius * 0.91,
                Color::rgb(246, 246, 248),
            );
            circle(
                ctx,
                cx - radius * 0.25,
                cy - radius * 0.3,
                radius * 0.25,
                Color::rgba(255, 255, 255, 180),
            );
        }
        CellState::Empty => {}
    }
}

fn circle(ctx: &mut CanvasRenderingContext2d<'_>, x: f32, y: f32, radius: f32, color: Color) {
    ctx.begin_path();
    ctx.arc(x, y, radius, 0.0, TAU, false);
    ctx.set_fill_style(color);
    ctx.fill();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_lines_preserve_all_cell_and_gutter_bounds() {
        assert_eq!(board_grid_rects().count(), 18);
        for y in 0..BOARD_BACKGROUND_SIZE as usize {
            for x in 0..BOARD_BACKGROUND_SIZE as usize {
                let px = BOARD_BACKGROUND_X + x as f32 + 0.5;
                let py = BOARD_BACKGROUND_Y + y as f32 + 0.5;
                let grid = board_grid_rects().any(|(gx, gy, width, height)| {
                    px >= gx && px < gx + width && py >= gy && py < gy + height
                });
                let cell = (0..8).any(|row| {
                    (0..8).any(|col| {
                        let cx = BOARD_X + col as f32 * CELL_STEP;
                        let cy = BOARD_Y + row as f32 * CELL_STEP;
                        px >= cx && px < cx + CELL && py >= cy && py < cy + CELL
                    })
                });
                assert_eq!(grid, !cell, "board pixel ({x}, {y})");
            }
        }
    }

    #[test]
    fn hover_redraws_only_when_the_visible_highlight_changes() {
        let mut game = Game::default();
        assert!(!game.set_hover(Some((0, 0))));
        assert!(!game.set_hover(Some((3, 3))));
        assert!(game.set_hover(Some((2, 3))));
        assert!(!game.set_hover(Some((2, 3))));
        assert!(game.set_hover(Some((3, 2))));
        assert!(game.set_hover(None));
        assert!(!game.set_hover(None));
        assert!(game.play(2, 3));
        game.ai_job().expect("AI turn");
        assert!(!game.set_hover(Some((2, 2))));
        assert!(!game.set_hover(None));
    }

    #[test]
    fn cursor_only_marks_legal_human_moves_as_clickable() {
        let mut game = Game::default();
        assert_eq!(game.cursor(), CursorIcon::Default);
        for hover in [Some((0, 0)), Some((3, 3))] {
            game.hover = hover;
            assert_eq!(game.cursor(), CursorIcon::Default);
            assert_eq!(game.interactive_hover(), None);
        }
        game.hover = Some((2, 3));
        assert_eq!(game.cursor(), CursorIcon::Pointer);
        assert_eq!(game.interactive_hover(), game.hover);
        game.turn = Player::White;
        assert_eq!(game.cursor(), CursorIcon::Default);
        game.turn = Player::Black;
        game.game_over = true;
        assert_eq!(game.cursor(), CursorIcon::Default);
    }

    #[test]
    fn thinking_cursor_clears_after_ai_completion_or_reset() {
        let mut game = Game::default();
        assert!(game.play(2, 3));
        let job = game.ai_job().expect("AI job");
        assert_eq!(game.cursor(), CursorIcon::Progress);
        assert_eq!(game.interactive_hover(), None);
        assert!(game.apply_ai(AiResult {
            job,
            movement: Some(job.board.valid_moves(Player::White)[0]),
        }));
        assert_eq!(game.cursor(), CursorIcon::Default);
        game.reset();
        assert!(game.play(2, 3));
        game.ai_job().expect("AI job");
        game.reset();
        assert_eq!(game.cursor(), CursorIcon::Default);
        assert!(!game.thinking);
    }

    #[test]
    fn hit_testing_excludes_gutters_and_scales_with_the_board() {
        let layout = GameLayout::new(LogicalSize::new(900.0, 900.0), 0.0);
        let point = LogicalPoint::new(
            (BOARD_X + 3.0 * CELL_STEP + CELL / 2.0) * 1.5,
            (BOARD_Y + 2.0 * CELL_STEP + CELL / 2.0) * 1.5,
        );
        assert_eq!(board_cell(&point, layout), Some((2, 3)));
        assert_eq!(board_cell(&LogicalPoint::new(10.0, 10.0), layout), None);
        let gap = LogicalPoint::new((BOARD_X + CELL + 1.0) * 1.5, (BOARD_Y + 10.0) * 1.5);
        assert_eq!(board_cell(&gap, layout), None);
    }

    #[test]
    fn stale_and_duplicate_ai_results_do_not_change_the_board() {
        let mut game = Game::default();
        assert!(game.play(2, 3));
        let job = game.ai_job().expect("job");
        assert!(game.ai_job().is_none());
        let movement = job.board.valid_moves(Player::White)[0];
        game.reset();
        let board = game.board;
        assert!(!game.apply_ai(AiResult {
            job,
            movement: Some(movement)
        }));
        assert_eq!(game.board, board);
        assert!(game.play(2, 3));
        let job = game.ai_job().expect("new job");
        assert!(game.apply_ai(AiResult {
            job,
            movement: Some(movement)
        }));
        let board = game.board;
        assert!(!game.apply_ai(AiResult {
            job,
            movement: Some(movement)
        }));
        assert_eq!(game.board, board);
    }

    #[test]
    fn game_applies_a_human_move() {
        let mut game = Game::default();
        assert!(game.play(2, 3));
        assert_eq!(game.board.score(Player::Black), 4);
        assert_eq!(game.board.score(Player::White), 1);
    }

    #[test]
    fn rejects_occupied_and_illegal() {
        let mut game = Game::default();
        assert!(!game.play(3, 3));
        assert!(!game.play(0, 0));
    }

    #[test]
    fn ends_when_neither_can_move() {
        let mut game = Game {
            board: "x".repeat(64).parse().expect("valid full board"),
            turn: Player::White,
            hover: None,
            game_over: false,
            window_size: LogicalSize::new(WINDOW_WIDTH, WINDOW_HEIGHT),
            top_inset: 0.0,
            theme: Theme::Light,
            revision: 0,
            thinking: false,
        };
        game.advance_passes();
        assert!(game.game_over);
    }

    #[test]
    fn game_layout_centers_with_a_fixed_aspect_ratio() {
        let wide = GameLayout::new(LogicalSize::new(1_000.0, 800.0), 0.0);
        assert_eq!(wide.scale, 4.0 / 3.0);
        assert_eq!((wide.offset_x, wide.offset_y), (100.0, 0.0));

        let tall = GameLayout::new(LogicalSize::new(720.0, 1_000.0), 0.0);
        assert_eq!(tall.scale, 1.2);
        assert_eq!((tall.offset_x, tall.offset_y), (0.0, 140.0));
    }

    #[test]
    fn game_layout_reserves_the_hidden_titlebar() {
        let layout = GameLayout::new(LogicalSize::new(600.0, 628.0), 28.0);
        assert_eq!(layout.scale, 1.0);
        assert_eq!((layout.offset_x, layout.offset_y), (0.0, 28.0));

        let titlebar = LogicalPoint::new(300.0, 14.0);
        assert!(layout.game_point(&titlebar).is_none());
    }

    #[test]
    fn light_and_dark_palettes_have_distinct_contrast() {
        let light = Palette::for_theme(Theme::Light);
        let dark = Palette::for_theme(Theme::Dark);
        assert_ne!(light.background, dark.background);
        assert!(u16::from(light.text.red) < u16::from(dark.text.red));
        assert_ne!(light.board, dark.board);
        assert_ne!(light.move_hint, dark.move_hint);
    }

    #[test]
    fn new_game_preserves_theme_and_window_size() {
        let mut game = Game {
            window_size: LogicalSize::new(800.0, 700.0),
            top_inset: 28.0,
            theme: Theme::Dark,
            ..Game::default()
        };
        game.reset();
        assert!(game.theme == Theme::Dark);
        assert_eq!(game.window_size.width, 800.0);
        assert_eq!(game.window_size.height, 700.0);
        assert_eq!(game.top_inset, 28.0);
    }
}
