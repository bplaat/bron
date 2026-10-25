/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::time::{Duration, Instant};

use bwindow::ScrollDelta;

#[derive(Default)]
pub(crate) struct Scroll {
    remainder: f64,
    direction: f64,
    pixels: bool,
    last_input: Option<Instant>,
    last_step: Option<Instant>,
}

impl Scroll {
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn steps(&mut self, delta: ScrollDelta, now: Instant) -> i64 {
        let (value, pixels) = match delta {
            ScrollDelta::Lines(_, y) => (-y, false),
            ScrollDelta::Pixels(_, y) => (-y / 96.0, true),
        };
        if !value.is_finite() {
            return 0;
        }
        if pixels != self.pixels
            || self
                .last_input
                .is_some_and(|last| now.duration_since(last) > Duration::from_millis(300))
        {
            self.reset();
        }
        if value != 0.0 && value.signum() != self.direction {
            self.remainder = 0.0;
            self.last_step = None;
            self.direction = value.signum();
        }
        self.pixels = pixels;
        self.last_input = Some(now);
        self.remainder += value;
        let whole = self.remainder.trunc();
        self.remainder -= whole;
        // Limit precise-scroll bursts so smoothing does not rapidly skip blocks.
        let ready = !pixels
            || self
                .last_step
                .is_none_or(|last| now.duration_since(last) >= Duration::from_millis(120));
        let step = if whole != 0.0 && ready {
            whole.signum() as i64
        } else {
            0
        };
        if step != 0 {
            self.last_step = Some(now);
        }
        step
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheels_and_fractional_wheels_do_not_skip_blocks() {
        let mut scroll = Scroll::default();
        let now = Instant::now();
        assert_eq!(scroll.steps(ScrollDelta::Lines(0.0, -3.0), now), 1);
        for _ in 0..3 {
            assert_eq!(scroll.steps(ScrollDelta::Lines(0.0, -0.25), now), 0);
        }
        assert_eq!(scroll.steps(ScrollDelta::Lines(0.0, -0.25), now), 1);
    }

    #[test]
    fn precise_scrolling_is_rate_limited() {
        let now = Instant::now();
        let mut scroll = Scroll::default();
        for _ in 0..3 {
            assert_eq!(scroll.steps(ScrollDelta::Pixels(0.0, -24.0), now), 0);
        }
        assert_eq!(scroll.steps(ScrollDelta::Pixels(0.0, -24.0), now), 1);
        assert_eq!(
            scroll.steps(
                ScrollDelta::Pixels(0.0, -960.0),
                now + Duration::from_millis(10),
            ),
            0
        );
        assert_eq!(
            scroll.steps(
                ScrollDelta::Pixels(0.0, 0.0),
                now + Duration::from_millis(130),
            ),
            0
        );
        assert_eq!(
            scroll.steps(
                ScrollDelta::Pixels(0.0, -96.0),
                now + Duration::from_millis(150),
            ),
            1
        );
    }

    #[test]
    fn reversal_and_idle_clear_old_fractional_input() {
        let now = Instant::now();
        let mut scroll = Scroll::default();
        assert_eq!(scroll.steps(ScrollDelta::Pixels(0.0, -90.0), now), 0);
        assert_eq!(scroll.steps(ScrollDelta::Pixels(0.0, 96.0), now), -1);
        assert_eq!(
            scroll.steps(
                ScrollDelta::Pixels(0.0, -90.0),
                now + Duration::from_secs(1),
            ),
            0
        );
    }
}
