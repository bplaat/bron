/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The engine's menus on the Wii, with the Wii Remote and an on-screen keyboard, and the
//! inventory's block grid.

use plaatcraft_engine::menu::{Keyboard, Platform};

use crate::input::{Button, Controller};

/// The Wii for the engine's menus: the render distance is fixed.
pub const PLATFORM: Platform = Platform {
    keyboard: Keyboard::OnScreen,
    quits: true,
    distance: false,
    confirm: "+",
    open: "Press A on a world",
    controls: &[
        "+ pause - Stick walk - D-pad look and slot - A place - B break",
        "C jump - Z sneak - Minus blocks - Z+Minus pick - 1 tools",
    ],
};

// The inventory grid.
pub const CELL: f32 = 44.0;
pub const COLUMNS: usize = 9;
pub const INVENTORY_TOP: f32 = 84.0;
const ROWS: usize = 6;
pub const INVENTORY_PAGE: usize = COLUMNS * ROWS;

/// The first block of the inventory page with the focused block.
pub const fn page_start(focus: usize) -> usize {
    focus / INVENTORY_PAGE * INVENTORY_PAGE
}

/// Moves through the inventory grid with the D-pad and the pointer.
pub fn grid(controller: &Controller, focus: &mut usize, count: usize, width: f32) {
    let page = page_start(*focus);
    if controller.pressed(Button::Left) {
        *focus = (*focus + count - 1) % count;
    }
    if controller.pressed(Button::Right) {
        *focus = (*focus + 1) % count;
    }
    if controller.pressed(Button::Up) {
        *focus = (*focus + count - COLUMNS) % count;
    }
    if controller.pressed(Button::Down) {
        *focus = (*focus + COLUMNS) % count;
    }
    if let Some([x, y, _]) = controller.pointer {
        let left = (width - CELL * COLUMNS as f32) / 2.0;
        let bottom = INVENTORY_TOP + CELL * ROWS as f32;
        if x >= left && x < left + CELL * COLUMNS as f32 && (INVENTORY_TOP..bottom).contains(&y) {
            let index = ((y - INVENTORY_TOP) / CELL) as usize * COLUMNS
                + ((x - left) / CELL) as usize
                + page;
            if index < count {
                *focus = index;
            }
        }
    }
}
