/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::time::Duration;

use bwindow::{LogicalPoint as Point, LogicalSize as Size};

pub(crate) const SIZE: Size = Size::new(640.0, 480.0);
pub(crate) const TICK: Duration = Duration::from_millis(15);
pub(crate) const ARENA: Rect = Rect::new(35.0, 35.0, 570.0, 410.0);

#[derive(Clone, Copy)]
pub(crate) struct Rect {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

impl Rect {
    pub(crate) const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub(crate) const fn right(self) -> f32 {
        self.x + self.width
    }

    pub(crate) const fn bottom(self) -> f32 {
        self.y + self.height
    }

    pub(crate) const fn contains(self, point: Point) -> bool {
        point.x >= self.x && point.x < self.right() && point.y >= self.y && point.y < self.bottom()
    }

    const fn overlaps(self, other: Self) -> bool {
        self.x < other.right()
            && self.right() > other.x
            && self.y < other.bottom()
            && self.bottom() > other.y
    }
}

struct Block {
    rect: Rect,
    dx: f32,
    dy: f32,
}

pub(crate) struct Game {
    pub(crate) player: Rect,
    blocks: [Block; 4],
    grab: Option<Point>,
    pub(crate) elapsed: Duration,
    pub(crate) score: u32,
    pub(crate) lost: bool,
}

impl Game {
    pub(crate) fn new() -> Self {
        let blocks = [
            (1.0, 1.0, 120.0, 64.0, 1.0, 1.0),
            (519.0, 1.0, 120.0, 32.0, -1.0, 1.0),
            (1.0, 359.0, 64.0, 120.0, 1.0, -1.0),
            (575.0, 415.0, 64.0, 64.0, -1.0, -1.0),
        ]
        .map(|(x, y, width, height, dx, dy)| Block {
            rect: Rect::new(x, y, width, height),
            dx,
            dy,
        });
        Self {
            player: Rect::new(288.0, 208.0, 64.0, 64.0),
            blocks,
            grab: None,
            elapsed: Duration::ZERO,
            score: 0,
            lost: false,
        }
    }

    pub(crate) fn demo() -> Self {
        let mut game = Self::new();
        for block in &mut game.blocks {
            block.rect.x += 110.0 * block.dx;
            block.rect.y += 110.0 * block.dy;
        }
        game
    }

    pub(crate) fn blues(&self) -> impl Iterator<Item = Rect> + '_ {
        self.blocks.iter().map(|block| block.rect)
    }

    pub(crate) const fn dragging(&self) -> bool {
        self.grab.is_some()
    }

    pub(crate) fn grab(&mut self, point: Point) {
        if !self.lost && self.player.contains(point) {
            self.grab = Some(Point::new(point.x - self.player.x, point.y - self.player.y));
        }
    }

    pub(crate) const fn drag(&mut self, point: Point) {
        if let Some(offset) = self.grab {
            self.player.x = point.x - offset.x;
            self.player.y = point.y - offset.y;
        }
    }

    pub(crate) const fn release(&mut self) {
        self.grab = None;
    }

    pub(crate) const fn lose(&mut self) {
        self.lost = true;
        self.release();
    }

    pub(crate) const fn level(&self) -> u64 {
        1 + self.elapsed.as_secs() / 10
    }

    pub(crate) fn step(&mut self) -> bool {
        if self.lost {
            return false;
        }
        let speed = self.level() as f32;
        self.score = self.score.saturating_add(1);
        self.elapsed += TICK;

        let player = self.player;
        if player.x < ARENA.x
            || player.y < ARENA.y
            || player.right() > ARENA.right()
            || player.bottom() > ARENA.bottom()
        {
            self.lose();
            return false;
        }

        let bounced = self.move_blocks(speed);
        if self.blues().any(|rect| rect.overlaps(player)) {
            self.lose();
        }
        bounced
    }

    pub(crate) fn move_blocks(&mut self, speed: f32) -> bool {
        let mut bounced = false;
        for block in &mut self.blocks {
            let rect = &mut block.rect;
            bounced |= advance_axis(&mut rect.x, &mut block.dx, SIZE.width - rect.width, speed);
            bounced |= advance_axis(&mut rect.y, &mut block.dy, SIZE.height - rect.height, speed);
        }
        bounced
    }
}

fn advance_axis(position: &mut f32, direction: &mut f32, limit: f32, speed: f32) -> bool {
    let next = *position + *direction * speed;
    let bounced = next <= 0.0 || next >= limit;
    if bounced {
        *direction = -*direction;
    }
    *position = next.clamp(0.0, limit);
    bounced
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_increases_every_ten_seconds_and_each_tick_scores_one_point() {
        let mut game = Game::new();
        game.elapsed = Duration::from_millis(29985);
        game.step();
        assert_eq!(game.level(), 4);
        assert_eq!(game.blocks[0].rect.x, 4.0);
        game.step();
        assert_eq!(game.blocks[0].rect.x, 8.0);
        assert_eq!(game.score, 2);
    }

    #[test]
    fn dragging_keeps_the_grab_offset_and_stops_on_loss() {
        let mut game = Game::new();
        game.grab(Point::new(300.0, 220.0));
        game.drag(Point::new(12.0, 200.0));
        assert_eq!(game.player.x, 0.0);
        game.step();
        assert!(game.lost);
        assert!(!game.dragging());
        game.step();
        assert_eq!(game.score, 1);
    }

    #[test]
    fn contact_with_blue_edges_is_safe_until_there_is_overlap() {
        let mut game = Game::new();
        game.blocks[0].rect = Rect::new(167.0, 208.0, 120.0, 64.0);
        game.step();
        assert!(!game.lost);
        game.step();
        assert!(game.lost);
    }
}
