/*
 * Copyright (c) 2017 Hans Wennborg
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Bitboard Othello engine derived from Hans Wennborg's C implementation:
//! https://www.hanshq.net/othello.html
//!
//! Adapted to safe Rust. Search is bounded and cancellable.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};
use core::str::FromStr;

const BOARD_CELLS: usize = 64;
const DIRECTIONS: usize = 8;
const WIN_BONUS: i32 = 1 << 20;
const CORNER_MASK: u64 = 0x8100_0000_0000_0081;

const SHIFT_MASKS: [u64; DIRECTIONS] = [
    0x7f7f_7f7f_7f7f_7f7f,
    0x007f_7f7f_7f7f_7f7f,
    u64::MAX,
    0x00fe_fefe_fefe_fefe,
    0xfefe_fefe_fefe_fefe,
    0xfefe_fefe_fefe_fe00,
    u64::MAX,
    0x7f7f_7f7f_7f7f_7f00,
];
const LEFT_SHIFTS: [u32; DIRECTIONS] = [0, 0, 0, 0, 1, 9, 8, 7];
const RIGHT_SHIFTS: [u32; DIRECTIONS] = [1, 9, 8, 7, 0, 0, 0, 0];

/// A player and disk color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Player {
    /// The player using black disks.
    Black,
    /// The player using white disks.
    White,
}

impl Player {
    /// Return the opposing player.
    pub const fn other(self) -> Self {
        match self {
            Self::Black => Self::White,
            Self::White => Self::Black,
        }
    }

    const fn index(self) -> usize {
        self as usize
    }
}

/// Contents of one board cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellState {
    /// A black disk.
    Black,
    /// A white disk.
    White,
    /// An empty cell.
    Empty,
}

/// A zero-indexed board move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Move {
    /// The zero-indexed board row.
    pub row: usize,
    /// The zero-indexed board column.
    pub col: usize,
}

impl Move {
    const fn from_index(index: usize) -> Self {
        Self {
            row: index / 8,
            col: index % 8,
        }
    }

    const fn index(self) -> usize {
        self.row * 8 + self.col
    }
}

/// Compact Othello state represented by one bitboard per player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Othello {
    disks: [u64; 2],
}

impl Default for Othello {
    fn default() -> Self {
        let mut game = Self { disks: [0; 2] };
        game.set_cell_state(3, 4, CellState::Black);
        game.set_cell_state(4, 3, CellState::Black);
        game.set_cell_state(3, 3, CellState::White);
        game.set_cell_state(4, 4, CellState::White);
        game
    }
}

impl Othello {
    /// Return the contents of a board cell.
    pub fn cell_state(self, row: usize, col: usize) -> CellState {
        let mask = cell_mask(row, col);
        if self.disks[Player::Black.index()] & mask != 0 {
            CellState::Black
        } else if self.disks[Player::White.index()] & mask != 0 {
            CellState::White
        } else {
            CellState::Empty
        }
    }

    /// Replace the contents of a board cell.
    pub fn set_cell_state(&mut self, row: usize, col: usize, state: CellState) {
        let mask = cell_mask(row, col);
        self.disks[0] &= !mask;
        self.disks[1] &= !mask;
        match state {
            CellState::Black => self.disks[Player::Black.index()] |= mask,
            CellState::White => self.disks[Player::White.index()] |= mask,
            CellState::Empty => {}
        }
    }

    /// Return the number of disks owned by a player.
    pub const fn score(self, player: Player) -> u32 {
        self.disks[player.index()].count_ones()
    }

    /// Return all legal moves for a player in board order.
    pub fn valid_moves(self, player: Player) -> Vec<Move> {
        let mut bits = self.valid_move_bits(player);
        let mut moves = Vec::with_capacity(bits.count_ones() as usize);
        while bits != 0 {
            let index = bits.trailing_zeros() as usize;
            moves.push(Move::from_index(index));
            bits &= bits - 1;
        }
        moves
    }

    /// Return whether a player has at least one legal move.
    pub fn has_valid_move(self, player: Player) -> bool {
        self.valid_move_bits(player) != 0
    }

    /// Return whether a move is legal for a player.
    pub fn is_valid_move(self, player: Player, movement: Move) -> bool {
        movement.row < 8
            && movement.col < 8
            && self.valid_move_bits(player) & (1_u64 << movement.index()) != 0
    }

    /// Apply a legal move and return whether it was accepted.
    pub fn make_move(&mut self, player: Player, movement: Move) -> bool {
        if !self.is_valid_move(player, movement) {
            return false;
        }
        let (my_disks, opponent_disks) = player_disks_mut(&mut self.disks, player);
        resolve_move(my_disks, opponent_disks, movement.index());
        true
    }

    /// Find a move within a bounded number of search nodes.
    #[cfg(test)]
    fn compute_move_with_budget(self, player: Player, depth: u32, nodes: u32) -> Option<Move> {
        self.compute_move_cancellable(player, depth, nodes, &|| false)
    }

    /// Search for a move within depth and node limits.
    ///
    /// The search returns `None` when the player has no legal move or when
    /// `cancelled` returns `true`.
    pub fn compute_move_cancellable(
        self,
        player: Player,
        depth: u32,
        nodes: u32,
        cancelled: &dyn Fn() -> bool,
    ) -> Option<Move> {
        if !self.has_valid_move(player) || cancelled() {
            return None;
        }
        iterative_negamax(
            self.disks[player.index()],
            self.disks[player.other().index()],
            depth.max(1),
            nodes.max(1),
            cancelled,
        )
        .map(Move::from_index)
    }

    /// Return a bitboard containing every legal move for a player.
    pub fn valid_move_bits(self, player: Player) -> u64 {
        generate_moves(
            self.disks[player.index()],
            self.disks[player.other().index()],
        )
    }
}

fn cell_mask(row: usize, col: usize) -> u64 {
    assert!(row < 8 && col < 8, "cell must be on the board");
    1_u64 << (row * 8 + col)
}

#[inline(always)]
const fn shift(disks: u64, direction: usize) -> u64 {
    if direction < DIRECTIONS / 2 {
        (disks >> RIGHT_SHIFTS[direction]) & SHIFT_MASKS[direction]
    } else {
        (disks << LEFT_SHIFTS[direction]) & SHIFT_MASKS[direction]
    }
}

#[inline]
fn generate_moves(my_disks: u64, opponent_disks: u64) -> u64 {
    debug_assert_eq!(my_disks & opponent_disks, 0);
    let empty = !(my_disks | opponent_disks);
    let mut legal = 0;
    for direction in 0..DIRECTIONS {
        let mut candidates = shift(my_disks, direction) & opponent_disks;
        for _ in 0..5 {
            candidates |= shift(candidates, direction) & opponent_disks;
        }
        legal |= shift(candidates, direction) & empty;
    }
    legal
}

#[inline]
fn resolve_move(my_disks: &mut u64, opponent_disks: &mut u64, index: usize) {
    let new_disk = 1_u64 << index;
    *my_disks |= new_disk;
    let mut captured = 0;
    for direction in 0..DIRECTIONS {
        let mut candidates = shift(new_disk, direction) & *opponent_disks;
        if candidates == 0 {
            continue;
        }
        for _ in 0..5 {
            candidates |= shift(candidates, direction) & *opponent_disks;
        }
        if shift(candidates, direction) & *my_disks != 0 {
            captured |= candidates;
        }
    }
    debug_assert_ne!(captured, 0);
    *my_disks ^= captured;
    *opponent_disks ^= captured;
}

const fn player_disks_mut(disks: &mut [u64; 2], player: Player) -> (&mut u64, &mut u64) {
    let [black, white] = disks;
    match player {
        Player::Black => (black, white),
        Player::White => (white, black),
    }
}

#[inline]
fn frontier_disks(my_disks: u64, opponent_disks: u64) -> (u64, u64) {
    let empty = !(my_disks | opponent_disks);
    let mut my_frontier = 0;
    let mut opponent_frontier = 0;
    for direction in 0..DIRECTIONS {
        let adjacent = shift(empty, direction);
        my_frontier |= adjacent & my_disks;
        opponent_frontier |= adjacent & opponent_disks;
    }
    (my_frontier, opponent_frontier)
}

#[inline]
fn evaluate(my_disks: u64, opponent_disks: u64, my_moves: u64, opponent_moves: u64) -> i32 {
    if my_moves == 0 && opponent_moves == 0 {
        return (my_disks.count_ones() as i32 - opponent_disks.count_ones() as i32) * WIN_BONUS;
    }
    let (my_frontier, opponent_frontier) = frontier_disks(my_disks, opponent_disks);
    let corners = (my_disks & CORNER_MASK).count_ones() as i32
        - (opponent_disks & CORNER_MASK).count_ones() as i32;
    let mobility = my_moves.count_ones() as i32 - opponent_moves.count_ones() as i32;
    let frontier = my_frontier.count_ones() as i32 - opponent_frontier.count_ones() as i32;
    corners * 16 + mobility * 2 - frontier
}

struct SearchBudget<'a> {
    remaining: u32,
    cancelled: &'a dyn Fn() -> bool,
}

impl SearchBudget<'_> {
    fn visit(&mut self) -> Option<()> {
        if self.remaining == 0 || (self.cancelled)() {
            return None;
        }
        self.remaining -= 1;
        Some(())
    }
}

#[allow(clippy::too_many_arguments)]
fn negamax(
    my_disks: u64,
    opponent_disks: u64,
    depth: u32,
    mut alpha: i32,
    beta: i32,
    mut best_move: Option<&mut usize>,
    budget: &mut SearchBudget<'_>,
) -> Option<i32> {
    budget.visit()?;
    let my_moves = generate_moves(my_disks, opponent_disks);
    // Internal nodes with legal moves do not need opponent mobility.
    if my_moves == 0 || depth == 0 {
        let opponent_moves = generate_moves(opponent_disks, my_disks);
        if my_moves == 0 && opponent_moves != 0 {
            return negamax(opponent_disks, my_disks, depth, -beta, -alpha, None, budget)
                .map(|score| -score);
        }
        return Some(evaluate(my_disks, opponent_disks, my_moves, opponent_moves));
    }
    let mut best = -i32::MAX;
    let mut remaining_moves = my_moves;
    while remaining_moves != 0 {
        let index = remaining_moves.trailing_zeros() as usize;
        remaining_moves &= remaining_moves - 1;
        let (mut next_mine, mut next_opponent) = (my_disks, opponent_disks);
        resolve_move(&mut next_mine, &mut next_opponent, index);
        let score = -negamax(
            next_opponent,
            next_mine,
            depth - 1,
            -beta,
            -alpha,
            None,
            budget,
        )?;
        if score > best {
            best = score;
            if let Some(output) = best_move.as_deref_mut() {
                *output = index;
            }
            alpha = alpha.max(score);
            if alpha >= beta {
                break;
            }
        }
    }
    Some(best)
}

fn iterative_negamax(
    my_disks: u64,
    opponent_disks: u64,
    max_depth: u32,
    nodes: u32,
    cancelled: &dyn Fn() -> bool,
) -> Option<usize> {
    let moves = generate_moves(my_disks, opponent_disks);
    if moves == 0 {
        return None;
    }
    let mut best = moves.trailing_zeros() as usize;
    let mut budget = SearchBudget {
        remaining: nodes,
        cancelled,
    };
    // Keep the last completed iteration. Even a one-node budget has a legal fallback.
    for depth in 1..=max_depth.min(64) {
        let mut candidate = best;
        let Some(score) = negamax(
            my_disks,
            opponent_disks,
            depth,
            -i32::MAX,
            i32::MAX,
            Some(&mut candidate),
            &mut budget,
        ) else {
            break;
        };
        best = candidate;
        if score.abs() >= WIN_BONUS {
            break;
        }
    }
    (!cancelled()).then_some(best)
}

impl Display for Othello {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        writeln!(formatter, " abcdefgh ")?;
        for row in 0..8 {
            write!(formatter, "{}", row + 1)?;
            for col in 0..8 {
                formatter.write_str(match self.cell_state(row, col) {
                    CellState::Black => "x",
                    CellState::White => "o",
                    CellState::Empty => ".",
                })?;
            }
            writeln!(formatter, "{}", row + 1)?;
        }
        writeln!(formatter, " abcdefgh ")
    }
}

impl FromStr for Othello {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let cells = value.chars().filter(|c| matches!(c, '.' | 'x' | 'o'));
        let mut game = Self { disks: [0; 2] };
        let mut count = 0;
        for (index, cell) in cells.enumerate() {
            if index >= BOARD_CELLS {
                return Err("board contains more than 64 cells");
            }
            let state = match cell {
                'x' => CellState::Black,
                'o' => CellState::White,
                _ => CellState::Empty,
            };
            game.set_cell_state(index / 8, index % 8, state);
            count += 1;
        }
        if count != BOARD_CELLS {
            return Err("board must contain exactly 64 cells");
        }
        Ok(game)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_matches_reference_across_a_complete_game() {
        fn reference(mine: u64, theirs: u64, depth: u32) -> i32 {
            let moves = generate_moves(mine, theirs);
            let other_moves = generate_moves(theirs, mine);
            if moves == 0 && other_moves != 0 {
                return -reference(theirs, mine, depth);
            }
            if depth == 0 || moves == 0 {
                return evaluate(mine, theirs, moves, other_moves);
            }
            let mut remaining = moves;
            let mut best = -i32::MAX;
            while remaining != 0 {
                let index = remaining.trailing_zeros() as usize;
                remaining &= remaining - 1;
                let (mut next_mine, mut next_theirs) = (mine, theirs);
                resolve_move(&mut next_mine, &mut next_theirs, index);
                best = best.max(-reference(next_theirs, next_mine, depth - 1));
            }
            best
        }

        let mut board = Othello::default();
        let mut player = Player::Black;
        loop {
            let mine = board.disks[player.index()];
            let theirs = board.disks[player.other().index()];
            let mut budget = SearchBudget {
                remaining: u32::MAX,
                cancelled: &|| false,
            };
            assert_eq!(
                negamax(mine, theirs, 2, -i32::MAX, i32::MAX, None, &mut budget),
                Some(reference(mine, theirs, 2))
            );
            if let Some(movement) = board.valid_moves(player).first() {
                assert!(board.make_move(player, *movement));
            } else if !board.has_valid_move(player.other()) {
                break;
            }
            player = player.other();
        }
    }

    #[test]
    fn cancellation_and_tiny_budgets_are_bounded() {
        let board = Othello::default();
        assert_eq!(
            board.compute_move_cancellable(Player::Black, 64, 500_000, &|| true),
            None
        );
        let movement = board
            .compute_move_with_budget(Player::Black, 64, 1)
            .expect("fallback");
        assert!(board.is_valid_move(Player::Black, movement));
        let visits = core::cell::Cell::new(0);
        let result = board.compute_move_cancellable(Player::Black, 64, 500_000, &|| {
            visits.set(visits.get() + 1);
            visits.get() > 20
        });
        assert_eq!(result, None);
        assert!(visits.get() <= 22);
    }

    #[test]
    fn initial_moves_match_reference() {
        assert_eq!(
            Othello::default().valid_moves(Player::Black),
            [
                Move { row: 2, col: 3 },
                Move { row: 3, col: 2 },
                Move { row: 4, col: 5 },
                Move { row: 5, col: 4 }
            ]
        );
    }

    #[test]
    fn board_string_round_trip() {
        let game = Othello::default();
        assert_eq!(game.to_string().parse(), Ok(game));
    }

    #[test]
    fn invalid_move_does_not_mutate_board() {
        let mut game = Othello::default();
        let original = game;
        assert!(!game.make_move(Player::Black, Move { row: 0, col: 0 }));
        assert_eq!(game, original);
    }

    #[test]
    fn valid_move_flips_disks() {
        let mut game = Othello::default();
        assert!(game.make_move(Player::Black, Move { row: 2, col: 3 }));
        assert_eq!(game.score(Player::Black), 4);
        assert_eq!(game.score(Player::White), 1);
    }

    #[test]
    fn moves_match_stepwise_flips_across_a_game() {
        fn state(player: Player) -> CellState {
            match player {
                Player::Black => CellState::Black,
                Player::White => CellState::White,
            }
        }

        let mut game = Othello::default();
        let mut player = Player::Black;
        loop {
            let moves = game.valid_moves(player);
            for movement in &moves {
                let mut expected = game;
                expected.set_cell_state(movement.row, movement.col, state(player));
                for (row_step, col_step) in [
                    (-1, -1),
                    (-1, 0),
                    (-1, 1),
                    (0, -1),
                    (0, 1),
                    (1, -1),
                    (1, 0),
                    (1, 1),
                ] {
                    let mut row = movement.row as i32 + row_step;
                    let mut col = movement.col as i32 + col_step;
                    let mut captured = Vec::new();
                    while (0..8).contains(&row)
                        && (0..8).contains(&col)
                        && game.cell_state(row as usize, col as usize) == state(player.other())
                    {
                        captured.push((row as usize, col as usize));
                        row += row_step;
                        col += col_step;
                    }
                    if (0..8).contains(&row)
                        && (0..8).contains(&col)
                        && game.cell_state(row as usize, col as usize) == state(player)
                    {
                        for (row, col) in captured {
                            expected.set_cell_state(row, col, state(player));
                        }
                    }
                }
                let mut actual = game;
                assert!(actual.make_move(player, *movement));
                assert_eq!(actual, expected);
            }
            if let Some(movement) = moves.first() {
                assert!(game.make_move(player, *movement));
            } else if !game.has_valid_move(player.other()) {
                break;
            }
            player = player.other();
        }
    }

    #[test]
    fn shallow_ai_returns_a_legal_move() {
        let game = Othello::default();
        let movement = game
            .compute_move_with_budget(Player::Black, 2, 32)
            .expect("legal AI move");
        assert!(game.is_valid_move(Player::Black, movement));
    }

    #[test]
    fn full_ai_returns_a_legal_move() {
        let mut game = Othello::default();
        assert!(game.make_move(Player::Black, Move { row: 2, col: 3 }));
        let movement = game
            .compute_move_with_budget(Player::White, 64, 500_000)
            .unwrap();
        assert!(game.is_valid_move(Player::White, movement));
    }
}
