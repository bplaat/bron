/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Aligned buffer suballocation. Images use separate native allocations.

use std::ops::Range;

pub(crate) struct Ranges {
    free: Vec<Range<u64>>,
}

impl Ranges {
    pub(crate) fn new(size: u64) -> Self {
        Self {
            free: std::iter::once(0..size).collect(),
        }
    }

    pub(crate) fn allocate(&mut self, size: u64, alignment: u64) -> Option<u64> {
        assert!(alignment.is_power_of_two());
        if size == 0 {
            return None;
        }
        let (index, start) = self.free.iter().enumerate().find_map(|(index, range)| {
            let start = range.start.checked_add(alignment - 1)? & !(alignment - 1);
            (start.checked_add(size)? <= range.end).then_some((index, start))
        })?;
        let range = self.free.remove(index);
        if range.start < start {
            self.free.push(range.start..start);
        }
        if start + size < range.end {
            self.free.push(start + size..range.end);
        }
        Some(start)
    }

    pub(crate) fn release(&mut self, range: Range<u64>) {
        self.free.push(range);
        self.free.sort_by_key(|range| range.start);
        let mut index = 1;
        while index < self.free.len() {
            if self.free[index - 1].end == self.free[index].start {
                self.free[index - 1].end = self.free.remove(index).end;
            } else {
                index += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alignment_gaps_and_released_blocks_are_reused() {
        let mut ranges = Ranges::new(32);
        assert_eq!(ranges.allocate(5, 8), Some(0));
        assert_eq!(ranges.allocate(5, 8), Some(8));
        assert_eq!(ranges.allocate(3, 1), Some(5));
        assert_eq!(ranges.allocate(20, 4), None);
        ranges.release(8..13);
        ranges.release(0..5);
        ranges.release(5..8);
        assert_eq!(ranges.allocate(32, 32), Some(0));
        assert_eq!(ranges.allocate(1, 1), None);
    }

    #[test]
    fn size_and_alignment_overflow_do_not_wrap() {
        let mut ranges = Ranges::new(u64::MAX);
        assert_eq!(ranges.allocate(u64::MAX - 1, 1), Some(0));
        assert_eq!(ranges.allocate(1, 4), None);
        assert_eq!(ranges.allocate(3, 1), None);
        assert_eq!(ranges.allocate(1, 1), Some(u64::MAX - 1));
    }
}
