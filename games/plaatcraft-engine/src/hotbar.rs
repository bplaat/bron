/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A Minecraft style hotbar with nine quick select slots.

use crate::world;

/// The number of hotbar slots.
pub const SLOTS: usize = 9;
const DEFAULT_SLOTS: [u8; SLOTS] = [1, 2, 3, 28, 27, 62, 6, 9, 4];

/// The name of a material.
pub fn name(material: u8) -> &'static str {
    world::MATERIALS
        .iter()
        .find(|&&(id, _)| id == material)
        .map_or("Unknown", |&(_, name)| name)
}

/// The nine quick select slots of placeable blocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hotbar {
    /// The material in each slot.
    pub slots: [u8; SLOTS],
    /// The selected slot.
    pub selected: usize,
}

impl Default for Hotbar {
    fn default() -> Self {
        Self {
            slots: DEFAULT_SLOTS,
            selected: 0,
        }
    }
}

impl Hotbar {
    /// The material of the selected slot.
    pub const fn material(&self) -> u8 {
        self.slots[self.selected]
    }

    /// Scrolling down selects the next slot, like the mouse wheel in Minecraft.
    pub const fn cycle(&mut self, steps: i64) {
        self.selected = (self.selected as i64 - steps).rem_euclid(SLOTS as i64) as usize;
    }

    /// Selects the slot holding a material, or puts it in the selected slot.
    pub fn pick(&mut self, material: u8) {
        match self.slots.iter().position(|&slot| slot == material) {
            Some(slot) => self.selected = slot,
            None => self.slots[self.selected] = material,
        }
    }

    /// Puts a material in a slot, a slot that already held it gets the replaced material.
    pub fn assign(&mut self, slot: usize, material: u8) {
        if let Some(other) = self.slots.iter().position(|&held| held == material) {
            self.slots[other] = self.slots[slot];
        }
        self.slots[slot] = material;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotbar_cycles_picks_and_assigns_without_duplicates() {
        let mut hotbar = Hotbar::default();
        hotbar.cycle(-1);
        assert_eq!(hotbar.selected, 1);
        hotbar.cycle(2);
        assert_eq!(hotbar.selected, SLOTS - 1);

        hotbar.pick(DEFAULT_SLOTS[2]);
        assert_eq!(hotbar.selected, 2);
        hotbar.pick(world::WATER);
        assert_eq!(hotbar.slots[2], world::WATER);

        // Assigning a material that is already in another slot swaps the two slots.
        hotbar.assign(0, world::WATER);
        assert_eq!(hotbar.slots[0], world::WATER);
        assert_eq!(hotbar.slots[2], DEFAULT_SLOTS[0]);
        assert_eq!(
            hotbar
                .slots
                .iter()
                .filter(|&&slot| slot == world::WATER)
                .count(),
            1
        );
    }
}
