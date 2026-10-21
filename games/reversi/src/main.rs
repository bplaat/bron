/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::cmp::Ordering;
use std::f32::consts::TAU;

use bcanvas::{
    CanvasBuilder, CanvasRenderingContext2d, Color, CursorIcon, FontWeight, TextAlign, TextBaseline,
};
use bwindow::{
    Accelerator, Event, EventLoopBuilder, Key, KeyCode, LogicalPoint, LogicalSize, MenuBarBuilder,
    MenuBuilder, MenuItem, Modifiers, MouseButton, MouseEvent, Theme, WindowBuilder, WindowEvent,
};
use reversi_engine::{CellState, Move, Othello, Player};

use crate::smoke::SmokeTest;
use crate::worker::{AiJob, AiResult, AiWorker};

mod smoke;
mod worker;

const WINDOW_WIDTH: f32 = 600.0;
const WINDOW_HEIGHT: f32 = 600.0;
const MIN_WINDOW_WIDTH: f32 = 360.0;
const MIN_WINDOW_HEIGHT: f32 = 360.0;
const BOARD_SIZE: usize = 8;
const COLUMN_LABELS: [&str; BOARD_SIZE] = ["A", "B", "C", "D", "E", "F", "G", "H"];
const ROW_LABELS: [&str; BOARD_SIZE] = ["1", "2", "3", "4", "5", "6", "7", "8"];
const GRID_OFFSET: f32 = 31.0;
const CELL: f32 = 52.0;
const CELL_GAP: f32 = 2.0;
const CELL_STEP: f32 = CELL + CELL_GAP;
const BOARD_X: f32 = GRID_OFFSET + CELL_STEP;
const BOARD_Y: f32 = GRID_OFFSET + CELL_STEP;
const BOARD_BACKGROUND_X: f32 = GRID_OFFSET + CELL;
const BOARD_BACKGROUND_Y: f32 = GRID_OFFSET + CELL;
const BOARD_BACKGROUND_SIZE: f32 = CELL * BOARD_SIZE as f32 + CELL_GAP * (BOARD_SIZE + 1) as f32;
const STATUS_Y: f32 = BOARD_BACKGROUND_Y + BOARD_BACKGROUND_SIZE + CELL / 2.0;
const RESTART_WIDTH: f32 = 176.0;
const RESTART_HEIGHT: f32 = 52.0;
const RESTART_X: f32 = BOARD_BACKGROUND_X + (BOARD_BACKGROUND_SIZE - RESTART_WIDTH) / 2.0;
const RESTART_Y: f32 = BOARD_BACKGROUND_Y + (BOARD_BACKGROUND_SIZE - RESTART_HEIGHT) / 2.0;
// On mobile the visible area is the board with its labels and status line plus a small margin,
// so it fills more of the screen; desktop windows show the whole design area.
const MOBILE: bool = cfg!(any(target_os = "ios", target_os = "android"));
const STATUS_HEIGHT: f32 = 20.0;
const VIEW_MARGIN: f32 = 8.0;
const VIEW_X: f32 = if MOBILE {
    GRID_OFFSET - VIEW_MARGIN
} else {
    0.0
};
const VIEW_Y: f32 = VIEW_X;
const VIEW_WIDTH: f32 = WINDOW_WIDTH - VIEW_X * 2.0;
const VIEW_HEIGHT: f32 = if MOBILE {
    STATUS_Y + STATUS_HEIGHT / 2.0 + VIEW_MARGIN - VIEW_Y
} else {
    WINDOW_HEIGHT
};

// MARK: Palette
#[derive(Clone, Copy)]
struct Palette {
    background: u32,
    text: Color,
    grid: Color,
    board: Color,
    board_hover: Color,
    move_hint: Color,
    overlay: Color,
    button: Color,
    button_hover: Color,
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
                overlay: Color::rgba(0, 0, 0, 90),
                button: Color::rgb(245, 245, 245),
                button_hover: Color::rgb(222, 222, 222),
            },
            Theme::Dark => Self {
                background: 0x1b201d,
                text: Color::rgb(235, 240, 237),
                grid: Color::rgb(72, 79, 75),
                board: Color::rgb(24, 126, 80),
                board_hover: Color::rgb(34, 151, 97),
                move_hint: Color::rgba(210, 239, 226, 155),
                overlay: Color::rgba(0, 0, 0, 120),
                button: Color::rgb(40, 46, 43),
                button_hover: Color::rgb(58, 66, 62),
            },
        }
    }
}

// MARK: Game
/// The interactive element under the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hover {
    None,
    Cell(Move),
    Restart,
}

#[derive(Clone)]
struct Game {
    board: Othello,
    turn: Player,
    game_over: bool,
    thinking: bool,
    revision: u64,
    hover: Hover,
}

impl Default for Game {
    fn default() -> Self {
        Self {
            board: Othello::default(),
            turn: Player::Black,
            game_over: false,
            thinking: false,
            revision: 0,
            hover: Hover::None,
        }
    }
}

impl Game {
    fn reset(&mut self) {
        *self = Self {
            revision: self.next_revision(),
            ..Self::default()
        };
    }

    const fn next_revision(&self) -> u64 {
        self.revision
            .checked_add(1)
            .expect("game revision exhausted")
    }

    fn play(&mut self, movement: Move) -> bool {
        if !self.board.make_move(self.turn, movement) {
            return false;
        }
        self.turn = self.turn.other();
        self.revision = self.next_revision();
        self.advance_passes();
        true
    }

    fn advance_passes(&mut self) {
        if !self.board.has_valid_move(self.turn) {
            self.turn = self.turn.other();
            self.game_over = !self.board.has_valid_move(self.turn);
        }
    }

    const fn is_human_turn(&self) -> bool {
        matches!(self.turn, Player::Black) && !self.game_over && !self.thinking
    }

    /// The hovered element, if it currently reacts to clicks.
    fn active_hover(&self) -> Hover {
        match self.hover {
            Hover::Cell(movement)
                if self.is_human_turn() && self.board.is_valid_move(Player::Black, movement) =>
            {
                self.hover
            }
            Hover::Restart if self.game_over => self.hover,
            _ => Hover::None,
        }
    }

    /// Update the hover, returns whether the visible highlight changed.
    fn set_hover(&mut self, hover: Hover) -> bool {
        let previous = self.active_hover();
        self.hover = hover;
        previous != self.active_hover()
    }

    /// Handle a click, returns whether the game changed.
    fn click(&mut self, target: Hover) -> bool {
        match target {
            Hover::Cell(movement) if self.is_human_turn() => self.play(movement),
            Hover::Restart if self.game_over => {
                self.reset();
                true
            }
            _ => false,
        }
    }

    fn cursor(&self) -> CursorIcon {
        if self.thinking {
            CursorIcon::Progress
        } else if self.active_hover() != Hover::None {
            CursorIcon::Pointer
        } else {
            CursorIcon::Default
        }
    }

    /// Start the AI search when it is the computer's turn.
    fn take_ai_job(&mut self) -> Option<AiJob> {
        if self.turn != Player::White || self.game_over || self.thinking {
            return None;
        }
        self.thinking = true;
        Some(AiJob {
            revision: self.revision,
            board: self.board,
        })
    }

    /// Apply a finished AI search, returns false for stale results.
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
        match result.movement {
            Some(movement) => self.play(movement),
            None => {
                self.advance_passes();
                true
            }
        }
    }

    fn status(&self) -> String {
        if !self.game_over {
            return match self.turn {
                Player::Black => "Human's move.",
                Player::White => "Computer's move...",
            }
            .into();
        }
        let human = self.board.score(Player::Black);
        let computer = self.board.score(Player::White);
        match human.cmp(&computer) {
            Ordering::Greater => format!("Human wins {human}-{computer}!"),
            Ordering::Less => format!("Computer wins {computer}-{human}!"),
            Ordering::Equal => "Draw!".into(),
        }
    }
}

// MARK: Layout
/// Maps the fixed size game coordinate space centered into the window.
#[derive(Clone, Copy)]
struct GameLayout {
    scale: f32,
    offset_x: f32,
    offset_y: f32,
    size: LogicalSize,
}

impl GameLayout {
    fn new(size: LogicalSize) -> Self {
        let scale = (size.width / VIEW_WIDTH)
            .min(size.height / VIEW_HEIGHT)
            .max(0.01);
        Self {
            scale,
            offset_x: (size.width - VIEW_WIDTH * scale) / 2.0 - VIEW_X * scale,
            offset_y: (size.height - VIEW_HEIGHT * scale) / 2.0 - VIEW_Y * scale,
            size,
        }
    }

    fn game_point(self, point: LogicalPoint) -> Option<(f32, f32)> {
        let x = (point.x - self.offset_x) / self.scale;
        let y = (point.y - self.offset_y) / self.scale;
        ((VIEW_X..VIEW_X + VIEW_WIDTH).contains(&x) && (VIEW_Y..VIEW_Y + VIEW_HEIGHT).contains(&y))
            .then_some((x, y))
    }

    fn hit_test(self, point: LogicalPoint, game_over: bool) -> Hover {
        let Some((x, y)) = self.game_point(point) else {
            return Hover::None;
        };
        if game_over {
            let restart = (RESTART_X..RESTART_X + RESTART_WIDTH).contains(&x)
                && (RESTART_Y..RESTART_Y + RESTART_HEIGHT).contains(&y);
            return if restart { Hover::Restart } else { Hover::None };
        }
        board_cell(x, y).map_or(Hover::None, Hover::Cell)
    }
}

fn board_cell(x: f32, y: f32) -> Option<Move> {
    let (x, y) = (x - BOARD_X, y - BOARD_Y);
    if x < 0.0 || y < 0.0 || x % CELL_STEP >= CELL || y % CELL_STEP >= CELL {
        return None;
    }
    let (row, col) = ((y / CELL_STEP) as usize, (x / CELL_STEP) as usize);
    (row < BOARD_SIZE && col < BOARD_SIZE).then_some(Move { row, col })
}

// MARK: Main
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
    let mut theme = event_loop.theme();

    let mut window_builder = WindowBuilder::new()
        .title("Reversi")
        .size(LogicalSize::new(WINDOW_WIDTH, WINDOW_HEIGHT))
        .min_size(LogicalSize::new(MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT))
        .background_color(Palette::for_theme(theme).background)
        .center();
    if !smoke {
        window_builder = window_builder.remember_window_state();
    }
    let mut window = window_builder.build();
    let window_id = window.id();
    #[cfg(target_os = "macos")]
    if smoke {
        window.set_theme(Theme::Light);
    }
    let mut canvas = CanvasBuilder::new(&window).build();

    let mut game = Game::default();
    let mut layout = GameLayout::new(LogicalSize::new(WINDOW_WIDTH, WINDOW_HEIGHT));
    let mut smoke_test = smoke.then(|| SmokeTest::new(&event_loop));
    event_loop.run(move |event| {
        let mut changed = false;
        match event {
            Event::Window(id, WindowEvent::RedrawRequested) if id == window_id => {
                assert!(canvas.draw(|context| {
                    layout = GameLayout::new(LogicalSize::new(context.width(), context.height()));
                    draw(context, &game, Palette::for_theme(theme), layout);
                }));
                if let Some(smoke_test) = &mut smoke_test {
                    smoke_test.after_draw(&mut game, &worker, &mut window);
                }
            }
            Event::Window(_, WindowEvent::Resize(size)) => {
                layout = GameLayout::new(size);
                game.hover = Hover::None;
                window.request_redraw();
            }
            Event::Window(_, WindowEvent::ThemeChanged(new_theme)) => {
                theme = new_theme;
                window.set_background_color(Palette::for_theme(theme).background);
                window.request_redraw();
                #[cfg(target_os = "macos")]
                if let Some(smoke_test) = &mut smoke_test {
                    smoke_test.theme_changed(theme, &mut window);
                }
            }
            Event::Window(_, WindowEvent::MouseMove { position, .. }) => {
                if game.set_hover(layout.hit_test(position, game.game_over)) {
                    window.request_redraw();
                }
            }
            Event::Window(_, WindowEvent::MouseLeave) => {
                if game.set_hover(Hover::None) {
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
            ) => changed = game.click(layout.hit_test(position, game.game_over)),
            Event::Window(_, WindowEvent::KeyDown(event))
                if !event.repeat
                    && matches!(&event.key, Key::Character(key) if key.eq_ignore_ascii_case("r")) =>
            {
                game.reset();
                changed = true;
            }
            Event::Action(action) if action == "game.new" => {
                game.reset();
                changed = true;
            }
            Event::UserEvent(AppEvent::Ai(result)) => {
                changed = game.apply_ai(result);
                if changed && let Some(smoke_test) = &mut smoke_test {
                    smoke_test.ai_applied();
                }
            }
            Event::LoopExiting => {
                if let Some(smoke_test) = &smoke_test {
                    assert!(
                        smoke_test.completed().get(),
                        "Reversi smoke test did not finish"
                    );
                }
            }
            _ => {}
        }
        if changed {
            // Drop any search that was started for an older position
            worker.cancel(game.revision);
            window.request_redraw();
        }
        if !window.is_closed()
            && let Some(job) = game.take_ai_job()
        {
            worker.submit(job);
            window.request_redraw();
        }
        let cursor = game.cursor();
        canvas.set_cursor(cursor);
        if let Some(smoke_test) = &mut smoke_test {
            smoke_test.cursor_set(cursor);
        }
    });
}

// MARK: Drawing
fn draw(ctx: &mut CanvasRenderingContext2d<'_>, game: &Game, palette: Palette, layout: GameLayout) {
    ctx.set_fill_style(Color::from_rgb(palette.background));
    ctx.fill_rect(0.0, 0.0, layout.size.width, layout.size.height);
    ctx.save();
    ctx.translate(layout.offset_x, layout.offset_y);
    ctx.scale(layout.scale, layout.scale);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_font("sans-serif", 19.0);
    ctx.set_font_weight(FontWeight::Bold);

    ctx.set_fill_style(palette.text);
    for (index, (column, row)) in COLUMN_LABELS.into_iter().zip(ROW_LABELS).enumerate() {
        let offset = index as f32 * CELL_STEP + CELL / 2.0;
        ctx.fill_text(column, BOARD_X + offset, GRID_OFFSET + CELL / 2.0);
        ctx.fill_text(row, GRID_OFFSET + CELL / 2.0, BOARD_Y + offset);
    }
    draw_board(ctx, game, palette);
    if game.game_over {
        draw_restart_button(ctx, game, palette);
    }
    ctx.set_fill_style(palette.text);
    ctx.fill_text(game.status(), WINDOW_WIDTH / 2.0, STATUS_Y);
    ctx.restore();
}

fn draw_board(ctx: &mut CanvasRenderingContext2d<'_>, game: &Game, palette: Palette) {
    // One board fill and grid lines replace 64 individual cell backgrounds.
    fill_board(ctx, palette.board);
    ctx.set_fill_style(palette.grid);
    for (x, y, width, height) in board_grid_rects() {
        ctx.fill_rect(x, y, width, height);
    }

    let hovered = match game.active_hover() {
        Hover::Cell(movement) => Some(movement),
        _ => None,
    };
    if let Some(Move { row, col }) = hovered {
        ctx.set_fill_style(palette.board_hover);
        ctx.fill_rect(
            BOARD_X + col as f32 * CELL_STEP,
            BOARD_Y + row as f32 * CELL_STEP,
            CELL,
            CELL,
        );
    }

    let legal_moves = if game.turn == Player::Black {
        game.board.valid_move_bits(Player::Black)
    } else {
        0
    };
    for row in 0..BOARD_SIZE {
        for col in 0..BOARD_SIZE {
            let cx = BOARD_X + col as f32 * CELL_STEP + CELL / 2.0;
            let cy = BOARD_Y + row as f32 * CELL_STEP + CELL / 2.0;
            let cell = game.board.cell_state(row, col);
            if cell != CellState::Empty {
                draw_disc(ctx, cx, cy, cell);
            } else if legal_moves & (1 << (row * BOARD_SIZE + col)) != 0 {
                let radius = if hovered == Some(Move { row, col }) {
                    CELL * 0.12
                } else {
                    CELL * 0.075
                };
                circle(ctx, cx, cy, radius, palette.move_hint);
            }
        }
    }
}

fn draw_restart_button(ctx: &mut CanvasRenderingContext2d<'_>, game: &Game, palette: Palette) {
    fill_board(ctx, palette.overlay);
    ctx.begin_path();
    ctx.round_rect(
        RESTART_X,
        RESTART_Y,
        RESTART_WIDTH,
        RESTART_HEIGHT,
        RESTART_HEIGHT / 2.0,
    );
    ctx.set_fill_style(if game.active_hover() == Hover::Restart {
        palette.button_hover
    } else {
        palette.button
    });
    ctx.fill();
    ctx.set_fill_style(palette.text);
    ctx.fill_text(
        "New Game",
        RESTART_X + RESTART_WIDTH / 2.0,
        RESTART_Y + RESTART_HEIGHT / 2.0,
    );
}

fn fill_board(ctx: &mut CanvasRenderingContext2d<'_>, color: Color) {
    ctx.set_fill_style(color);
    ctx.fill_rect(
        BOARD_BACKGROUND_X,
        BOARD_BACKGROUND_Y,
        BOARD_BACKGROUND_SIZE,
        BOARD_BACKGROUND_SIZE,
    );
}

fn board_grid_rects() -> impl Iterator<Item = (f32, f32, f32, f32)> {
    (0..=BOARD_SIZE).flat_map(|line| {
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

fn draw_disc(ctx: &mut CanvasRenderingContext2d<'_>, cx: f32, cy: f32, cell: CellState) {
    let radius = CELL * 0.46;
    let (edge, face, shine, offset_x, shine_radius) = match cell {
        CellState::Black => (
            Color::rgb(8, 8, 10),
            Color::rgb(31, 31, 34),
            Color::rgba(255, 255, 255, 24),
            1.2,
            0.24,
        ),
        CellState::White => (
            Color::rgb(174, 174, 178),
            Color::rgb(246, 246, 248),
            Color::rgba(255, 255, 255, 180),
            1.0,
            0.25,
        ),
        CellState::Empty => return,
    };

    // The game builds its small contact shadow from the same path primitives
    // as every other shape; Canvas itself only owns drawing fundamentals.
    circle(ctx, cx, cy + 2.0, radius, Color::rgba(0, 0, 0, 80));
    circle(ctx, cx, cy, radius, edge);
    circle(ctx, cx - offset_x, cy - 1.8, radius * 0.91, face);
    circle(
        ctx,
        cx - radius * 0.25,
        cy - radius * 0.3,
        radius * shine_radius,
        shine,
    );
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

    const OPENING: Move = Move { row: 2, col: 3 };

    const fn cell(row: usize, col: usize) -> Hover {
        Hover::Cell(Move { row, col })
    }

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
                let cell = (0..BOARD_SIZE).any(|row| {
                    (0..BOARD_SIZE).any(|col| {
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
        assert!(!game.set_hover(cell(0, 0)));
        assert!(!game.set_hover(cell(3, 3)));
        assert!(game.set_hover(cell(2, 3)));
        assert!(!game.set_hover(cell(2, 3)));
        assert!(game.set_hover(cell(3, 2)));
        assert!(game.set_hover(Hover::None));
        assert!(!game.set_hover(Hover::None));
        assert!(game.play(OPENING));
        game.take_ai_job().expect("AI turn");
        assert!(!game.set_hover(cell(2, 2)));
        assert!(!game.set_hover(Hover::None));
    }

    #[test]
    fn cursor_only_marks_legal_human_moves_as_clickable() {
        let mut game = Game::default();
        assert_eq!(game.cursor(), CursorIcon::Default);
        for hover in [cell(0, 0), cell(3, 3)] {
            game.hover = hover;
            assert_eq!(game.cursor(), CursorIcon::Default);
            assert_eq!(game.active_hover(), Hover::None);
        }
        game.hover = cell(2, 3);
        assert_eq!(game.cursor(), CursorIcon::Pointer);
        assert_eq!(game.active_hover(), game.hover);
        game.turn = Player::White;
        assert_eq!(game.cursor(), CursorIcon::Default);
        game.turn = Player::Black;
        game.game_over = true;
        assert_eq!(game.cursor(), CursorIcon::Default);
    }

    #[test]
    fn thinking_cursor_clears_after_ai_completion_or_reset() {
        let mut game = Game::default();
        assert!(game.play(OPENING));
        let job = game.take_ai_job().expect("AI job");
        assert_eq!(game.cursor(), CursorIcon::Progress);
        assert_eq!(game.active_hover(), Hover::None);
        assert!(game.apply_ai(AiResult {
            job,
            movement: Some(job.board.valid_moves(Player::White)[0]),
        }));
        assert_eq!(game.cursor(), CursorIcon::Default);
        game.reset();
        assert!(game.play(OPENING));
        game.take_ai_job().expect("AI job");
        game.reset();
        assert_eq!(game.cursor(), CursorIcon::Default);
        assert!(!game.thinking);
    }

    #[test]
    fn restart_button_only_activates_when_the_game_is_over() {
        let layout = GameLayout::new(LogicalSize::new(900.0, 900.0));
        let center = LogicalPoint::new(
            (RESTART_X + RESTART_WIDTH / 2.0) * 1.5,
            (RESTART_Y + RESTART_HEIGHT / 2.0) * 1.5,
        );
        assert_eq!(layout.hit_test(center, true), Hover::Restart);
        assert_ne!(layout.hit_test(center, false), Hover::Restart);
        assert_eq!(
            layout.hit_test(LogicalPoint::new(10.0, 10.0), true),
            Hover::None
        );

        let mut game = Game::default();
        assert!(!game.set_hover(Hover::Restart));
        assert!(!game.click(Hover::Restart));
        game.game_over = true;
        assert_eq!(game.cursor(), CursorIcon::Pointer);
        let revision = game.revision;
        assert!(game.click(Hover::Restart));
        assert!(!game.game_over);
        assert!(game.revision > revision);
        assert_eq!(game.active_hover(), Hover::None);
    }

    #[test]
    fn hit_testing_excludes_gutters_and_scales_with_the_board() {
        let layout = GameLayout::new(LogicalSize::new(900.0, 900.0));
        let point = LogicalPoint::new(
            (BOARD_X + 3.0 * CELL_STEP + CELL / 2.0) * 1.5,
            (BOARD_Y + 2.0 * CELL_STEP + CELL / 2.0) * 1.5,
        );
        assert_eq!(layout.hit_test(point, false), cell(2, 3));
        assert_eq!(
            layout.hit_test(LogicalPoint::new(10.0, 10.0), false),
            Hover::None
        );
        let gap = LogicalPoint::new((BOARD_X + CELL + 1.0) * 1.5, (BOARD_Y + 10.0) * 1.5);
        assert_eq!(layout.hit_test(gap, false), Hover::None);
    }

    #[test]
    fn stale_and_duplicate_ai_results_do_not_change_the_board() {
        let mut game = Game::default();
        assert!(game.play(OPENING));
        let job = game.take_ai_job().expect("job");
        assert!(game.take_ai_job().is_none());
        let movement = job.board.valid_moves(Player::White)[0];
        game.reset();
        let board = game.board;
        assert!(!game.apply_ai(AiResult {
            job,
            movement: Some(movement)
        }));
        assert_eq!(game.board, board);
        assert!(game.play(OPENING));
        let job = game.take_ai_job().expect("new job");
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
        assert!(game.play(OPENING));
        assert_eq!(game.board.score(Player::Black), 4);
        assert_eq!(game.board.score(Player::White), 1);
    }

    #[test]
    fn rejects_occupied_and_illegal() {
        let mut game = Game::default();
        assert!(!game.play(Move { row: 3, col: 3 }));
        assert!(!game.play(Move { row: 0, col: 0 }));
    }

    #[test]
    fn ends_when_neither_can_move() {
        let mut game = Game {
            board: "x".repeat(64).parse().expect("valid full board"),
            turn: Player::White,
            ..Game::default()
        };
        game.advance_passes();
        assert!(game.game_over);
    }

    #[test]
    fn reset_starts_a_new_game_with_a_newer_revision() {
        let mut game = Game::default();
        assert!(game.play(OPENING));
        let revision = game.revision;
        game.reset();
        assert_eq!(game.board, Othello::default());
        assert_eq!(game.turn, Player::Black);
        assert!(game.revision > revision);
    }

    #[test]
    fn game_layout_centers_with_a_fixed_aspect_ratio() {
        let wide = GameLayout::new(LogicalSize::new(1_000.0, 800.0));
        assert_eq!(wide.scale, 4.0 / 3.0);
        assert_eq!((wide.offset_x, wide.offset_y), (100.0, 0.0));

        let tall = GameLayout::new(LogicalSize::new(720.0, 1_000.0));
        assert_eq!(tall.scale, 1.2);
        assert_eq!((tall.offset_x, tall.offset_y), (0.0, 140.0));
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
}
