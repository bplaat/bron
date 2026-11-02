/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The controls of the consoles after their editions of Minecraft. The ports read their
//! controllers into a [`Wiimote`] or [`Gamepad`], which map the buttons to what the player
//! does in a [`Pad`].

/// What the player is doing, which changes what the buttons do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Context {
    /// A menu page is open.
    Menu,
    /// The inventory's block grid is open.
    Inventory,
    /// The player plays the world.
    Play,
}

/// What the player does with the controllers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    /// Moves up in a menu or the inventory.
    Up,
    /// Moves down in a menu or the inventory.
    Down,
    /// Moves left in a menu or the inventory.
    Left,
    /// Moves right in a menu or the inventory.
    Right,
    /// Presses the focused button, or assigns the focused block in the inventory.
    Select,
    /// Goes back from a menu page, erases while typing.
    Back,
    /// Finishes typing.
    Confirm,
    /// Opens or closes the pause menu.
    Pause,
    /// Places a block, holding repeats it.
    Place,
    /// Breaks a block, holding repeats it.
    Break,
    /// Jumps, swims and flies up, a double tap flies.
    Jump,
    /// Sneaks and flies down.
    Sneak,
    /// Runs and flies faster.
    Fast,
    /// Opens the inventory.
    Inventory,
    /// Picks the targeted block into the hotbar.
    Pick,
    /// Opens the build tools.
    Tools,
    /// Selects the hotbar slot to the left.
    Previous,
    /// Selects the hotbar slot to the right.
    Next,
    /// Touches the screen at the pointer, like a hotbar slot.
    Tap,
}

impl Button {
    const fn mask(self) -> u32 {
        1 << self as u32
    }
}

/// The controllers mapped for what the player does.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pad {
    held: u32,
    pressed: u32,
    /// Walking, x to the right and y forward, from -1.0 to 1.0.
    pub walk: [f32; 2],
    /// Turning, x to the right and y up, from -1.0 to 1.0.
    pub look: [f32; 2],
    /// Where the pointer is on the 2D screen space, and its roll in degrees.
    pub pointer: Option<[f32; 3]>,
}

impl Pad {
    /// Whether a button is held.
    pub const fn held(&self, button: Button) -> bool {
        self.held & button.mask() != 0
    }

    /// Whether a button was pressed since the last read.
    pub const fn pressed(&self, button: Button) -> bool {
        self.pressed & button.mask() != 0
    }

    const fn set(&mut self, button: Button, (held, pressed): (bool, bool)) {
        if held {
            self.held |= button.mask();
        }
        if pressed {
            self.pressed |= button.mask();
        }
    }

    /// Both controllers together, like a Wii U GamePad and a Wii Remote.
    pub fn merge(self, other: Self) -> Self {
        let add = |a: [f32; 2], b: [f32; 2]| [0, 1].map(|i| (a[i] + b[i]).clamp(-1.0, 1.0));
        Self {
            held: self.held | other.held,
            pressed: self.pressed | other.pressed,
            walk: add(self.walk, other.walk),
            look: add(self.look, other.look),
            pointer: self.pointer.or(other.pointer),
        }
    }
}

// The held and pressed state of the buttons of a mask.
const fn state(held: u32, pressed: u32, mask: u32) -> (bool, bool) {
    (held & mask != 0, pressed & mask != 0)
}

const fn or((a, b): (bool, bool), (c, d): (bool, bool)) -> (bool, bool) {
    (a || c, b || d)
}

/// A Wii Remote with a Nunchuk: the Nunchuk stick walks and turns, D-pad up and down look up
/// and down and D-pad left and right select a hotbar slot. A places and B breaks blocks, C
/// jumps and double tapping it flies, Z sneaks, 2 runs or flies faster, Minus opens the
/// inventory, Z and Minus picks a block, 1 opens the build tools and + or Home the menu,
/// where the pointer or the D-pad with A choose a button and B goes back.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Wiimote {
    /// The held buttons, of the masks of this type.
    pub held: u32,
    /// The buttons pressed since the last read.
    pub pressed: u32,
    /// The Nunchuk stick, x to the right and y forward, `None` without a Nunchuk.
    pub stick: Option<[f32; 2]>,
    /// Where the remote points on the 2D screen space, and its roll in degrees.
    pub pointer: Option<[f32; 3]>,
}

impl Wiimote {
    /// The A button.
    pub const A: u32 = 1 << 0;
    /// The B trigger.
    pub const B: u32 = 1 << 1;
    /// The 1 button.
    pub const ONE: u32 = 1 << 2;
    /// The 2 button.
    pub const TWO: u32 = 1 << 3;
    /// The + button.
    pub const PLUS: u32 = 1 << 4;
    /// The - button.
    pub const MINUS: u32 = 1 << 5;
    /// The Home button.
    pub const HOME: u32 = 1 << 6;
    /// D-pad up.
    pub const UP: u32 = 1 << 7;
    /// D-pad down.
    pub const DOWN: u32 = 1 << 8;
    /// D-pad left.
    pub const LEFT: u32 = 1 << 9;
    /// D-pad right.
    pub const RIGHT: u32 = 1 << 10;
    /// The Nunchuk's C button.
    pub const C: u32 = 1 << 11;
    /// The Nunchuk's Z trigger.
    pub const Z: u32 = 1 << 12;

    /// The pad for what the player does.
    pub fn pad(&self, context: Context) -> Pad {
        let button = |mask| state(self.held, self.pressed, mask);
        let mut pad = Pad {
            pointer: self.pointer,
            ..Pad::default()
        };
        pad.set(Button::Pause, or(button(Self::PLUS), button(Self::HOME)));
        match context {
            Context::Menu | Context::Inventory => {
                pad.set(Button::Up, button(Self::UP));
                pad.set(Button::Down, button(Self::DOWN));
                pad.set(Button::Left, button(Self::LEFT));
                pad.set(Button::Right, button(Self::RIGHT));
                pad.set(Button::Select, button(Self::A));
                pad.set(Button::Back, button(Self::B));
                pad.set(Button::Confirm, button(Self::PLUS));
                if context == Context::Inventory {
                    pad.set(Button::Back, button(Self::MINUS));
                    pad.set(Button::Previous, button(Self::Z));
                    pad.set(Button::Next, button(Self::C));
                }
            }
            Context::Play => {
                let picking = self.held & Self::Z != 0;
                let [x, y] = self.stick.unwrap_or_default();
                pad.walk = [0.0, y];
                let pitch = f32::from(u8::from(self.held & Self::UP != 0))
                    - f32::from(u8::from(self.held & Self::DOWN != 0));
                pad.look = [x, pitch];
                pad.set(Button::Place, button(Self::A));
                pad.set(Button::Break, button(Self::B));
                pad.set(Button::Jump, button(Self::C));
                pad.set(Button::Sneak, button(Self::Z));
                pad.set(Button::Fast, button(Self::TWO));
                pad.set(Button::Tools, button(Self::ONE));
                pad.set(Button::Previous, button(Self::LEFT));
                pad.set(Button::Next, button(Self::RIGHT));
                let minus = button(Self::MINUS);
                pad.set(
                    if picking {
                        Button::Pick
                    } else {
                        Button::Inventory
                    },
                    minus,
                );
            }
        }
        pad
    }
}

/// The Wii U GamePad, like Minecraft: Wii U Edition: the left stick walks and the right
/// stick looks around, ZL breaks and ZR places blocks, A jumps and double tapping it flies, B
/// sneaks, clicking the left stick runs, L and R or the D-pad select a hotbar slot, and so does
/// touching it, X opens the inventory, Y picks a block, Minus opens the build tools and + the
/// menu. The menus and the inventory are touched or moved through with the D-pad, A presses
/// and B goes back.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Gamepad {
    /// The held buttons, of the masks of this type.
    pub held: u32,
    /// The buttons pressed since the last read.
    pub pressed: u32,
    /// The left stick, x to the right and y up.
    pub left: [f32; 2],
    /// The right stick, x to the right and y up.
    pub right: [f32; 2],
    /// Where the touch screen is touched on the 2D screen space, the GamePad shows the TV.
    pub touch: Option<[f32; 2]>,
    /// Whether the touch started since the last read.
    pub touched: bool,
}

impl Gamepad {
    /// The A button.
    pub const A: u32 = 1 << 0;
    /// The B button.
    pub const B: u32 = 1 << 1;
    /// The X button.
    pub const X: u32 = 1 << 2;
    /// The Y button.
    pub const Y: u32 = 1 << 3;
    /// The L button.
    pub const L: u32 = 1 << 4;
    /// The R button.
    pub const R: u32 = 1 << 5;
    /// The ZL trigger.
    pub const ZL: u32 = 1 << 6;
    /// The ZR trigger.
    pub const ZR: u32 = 1 << 7;
    /// The + button.
    pub const PLUS: u32 = 1 << 8;
    /// The - button.
    pub const MINUS: u32 = 1 << 9;
    /// D-pad up.
    pub const UP: u32 = 1 << 10;
    /// D-pad down.
    pub const DOWN: u32 = 1 << 11;
    /// D-pad left.
    pub const LEFT: u32 = 1 << 12;
    /// D-pad right.
    pub const RIGHT: u32 = 1 << 13;
    /// Clicking the left stick.
    pub const STICK_L: u32 = 1 << 14;

    /// The pad for what the player does.
    pub fn pad(&self, context: Context) -> Pad {
        let button = |mask| state(self.held, self.pressed, mask);
        let mut pad = Pad {
            pointer: self.touch.map(|[x, y]| [x, y, 0.0]),
            ..Pad::default()
        };
        pad.set(Button::Pause, button(Self::PLUS));
        pad.set(Button::Tap, (self.touch.is_some(), self.touched));
        match context {
            Context::Menu | Context::Inventory => {
                pad.set(Button::Up, button(Self::UP));
                pad.set(Button::Down, button(Self::DOWN));
                pad.set(Button::Left, button(Self::LEFT));
                pad.set(Button::Right, button(Self::RIGHT));
                pad.set(Button::Select, or(button(Self::A), (false, self.touched)));
                pad.set(Button::Back, button(Self::B));
                pad.set(Button::Confirm, button(Self::PLUS));
                if context == Context::Inventory {
                    pad.set(Button::Back, button(Self::X));
                    pad.set(Button::Previous, button(Self::L));
                    pad.set(Button::Next, button(Self::R));
                }
            }
            Context::Play => {
                pad.walk = self.left;
                pad.look = self.right;
                pad.set(Button::Break, button(Self::ZL));
                pad.set(Button::Place, button(Self::ZR));
                pad.set(Button::Jump, button(Self::A));
                pad.set(Button::Sneak, button(Self::B));
                pad.set(Button::Fast, button(Self::STICK_L));
                pad.set(Button::Inventory, button(Self::X));
                pad.set(Button::Pick, button(Self::Y));
                pad.set(Button::Tools, button(Self::MINUS));
                pad.set(Button::Previous, or(button(Self::L), button(Self::LEFT)));
                pad.set(Button::Next, or(button(Self::R), button(Self::RIGHT)));
            }
        }
        pad
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wiimote_minus_opens_inventory_or_picks_with_z() {
        let minus = Wiimote {
            held: Wiimote::MINUS,
            pressed: Wiimote::MINUS,
            ..Wiimote::default()
        };
        assert!(minus.pad(Context::Play).pressed(Button::Inventory));
        let pick = Wiimote {
            held: Wiimote::MINUS | Wiimote::Z,
            ..minus
        };
        let pad = pick.pad(Context::Play);
        assert!(pad.pressed(Button::Pick) && !pad.pressed(Button::Inventory));
    }

    #[test]
    fn gamepad_touch_selects_in_menus_and_taps_in_game() {
        let touch = Gamepad {
            touch: Some([10.0, 20.0]),
            touched: true,
            ..Gamepad::default()
        };
        let menu = touch.pad(Context::Menu);
        assert!(menu.pressed(Button::Select));
        assert_eq!(menu.pointer, Some([10.0, 20.0, 0.0]));
        let play = touch.pad(Context::Play);
        assert!(play.pressed(Button::Tap) && !play.pressed(Button::Select));
    }

    #[test]
    fn merged_pads_add_sticks_and_buttons() {
        let gamepad = Gamepad {
            held: Gamepad::A,
            left: [0.75, 0.5],
            ..Gamepad::default()
        };
        let wiimote = Wiimote {
            held: Wiimote::B,
            stick: Some([0.0, 0.75]),
            ..Wiimote::default()
        };
        let pad = gamepad.pad(Context::Play).merge(wiimote.pad(Context::Play));
        assert!(pad.held(Button::Jump) && pad.held(Button::Break));
        assert_eq!(pad.walk, [0.75, 1.0]);
    }
}
