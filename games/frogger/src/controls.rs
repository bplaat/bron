// Copyright (c) 2026 Bastiaan van der Plaat
// SPDX-License-Identifier: MIT

use std::time::{Duration, Instant};

use bwindow::{LogicalPoint, ScrollDelta};

fn direction(x: f64, y: f64) -> Option<(i32, i32)> {
    if !x.is_finite() || !y.is_finite() || x.abs().max(y.abs()) < 24.0 {
        return None;
    }
    Some(if x.abs() > y.abs() {
        (if x > 0.0 { 1 } else { -1 }, 0)
    } else {
        (0, if y < 0.0 { 1 } else { -1 })
    })
}

pub(super) fn swipe(start: LogicalPoint, end: LogicalPoint) -> Option<(i32, i32)> {
    direction(f64::from(end.x - start.x), f64::from(end.y - start.y))
}

#[derive(Default)]
pub(super) struct Scroll {
    x: f64,
    y: f64,
    last: Option<Instant>,
    consumed: bool,
    pixels: bool,
}

impl Scroll {
    pub(super) fn update(&mut self, delta: ScrollDelta, now: Instant) -> Option<(i32, i32)> {
        let (x, y, pixels) = match delta {
            ScrollDelta::Pixels(x, y) => (x, y, true),
            ScrollDelta::Lines(x, y) => (x * 24.0, y * 24.0, false),
        };
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        if pixels != self.pixels
            || self
                .last
                .is_some_and(|last| now.duration_since(last) > Duration::from_millis(180))
        {
            *self = Self::default();
        }
        self.pixels = pixels;
        self.last = Some(now);
        // One precise-scroll gesture makes one hop, including its momentum tail.
        if self.consumed {
            return None;
        }
        self.x += x;
        self.y += y;
        let movement = direction(self.x, self.y)?;
        self.x = 0.0;
        self.y = 0.0;
        self.consumed = pixels;
        Some(movement)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taps_and_swipes_work_in_all_directions() {
        let origin = LogicalPoint::new(100.0, 100.0);
        assert_eq!(swipe(origin, LogicalPoint::new(105.0, 110.0)), None);
        for (x, y, expected) in [
            (140.0, 105.0, (1, 0)),
            (60.0, 105.0, (-1, 0)),
            (105.0, 60.0, (0, 1)),
            (105.0, 140.0, (0, -1)),
        ] {
            assert_eq!(swipe(origin, LogicalPoint::new(x, y)), Some(expected));
        }
    }

    #[test]
    fn trackpad_motion_accumulates_and_momentum_does_not_repeat() {
        let mut scroll = Scroll::default();
        let now = Instant::now();
        assert_eq!(scroll.update(ScrollDelta::Pixels(8.0, 0.0), now), None);
        assert_eq!(scroll.update(ScrollDelta::Pixels(8.0, 0.0), now), None);
        assert_eq!(
            scroll.update(ScrollDelta::Pixels(8.0, 0.0), now),
            Some((1, 0))
        );
        assert_eq!(
            scroll.update(
                ScrollDelta::Pixels(100.0, 0.0),
                now + Duration::from_millis(20)
            ),
            None
        );
        assert_eq!(
            scroll.update(
                ScrollDelta::Pixels(0.0, -24.0),
                now + Duration::from_millis(250)
            ),
            Some((0, 1))
        );
    }

    #[test]
    fn wheels_are_repeatable_and_invalid_motion_is_ignored() {
        let mut scroll = Scroll::default();
        let now = Instant::now();
        assert_eq!(scroll.update(ScrollDelta::Pixels(f64::NAN, 0.0), now), None);
        for _ in 0..2 {
            assert_eq!(
                scroll.update(ScrollDelta::Lines(0.0, 1.0), now),
                Some((0, -1))
            );
        }
    }
}
