/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The Wii Remote with Nunchuk controls, after the console editions of Minecraft: the
//! Nunchuk stick walks and turns, D-pad up and down look up and down and D-pad left and
//! right select a hotbar slot. A places and B breaks blocks, C jumps and double tapping it
//! flies, Z sneaks, 2 toggles the night and + or Home open the menu, where the pointer or
//! the D-pad with A choose a button and B goes back.

use core::mem::MaybeUninit;

use libogc_sys::*;
use plaatcraft_engine::camera::Input;

const STICK_DEADZONE: f32 = 0.12;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Up,
    Down,
    Left,
    Right,
    A,
    B,
    Plus,
    Two,
    Home,
    Z,
    C,
}

impl Button {
    const fn mask(self) -> u32 {
        match self {
            Self::Up => WPAD_BUTTON_UP,
            Self::Down => WPAD_BUTTON_DOWN,
            Self::Left => WPAD_BUTTON_LEFT,
            Self::Right => WPAD_BUTTON_RIGHT,
            Self::A => WPAD_BUTTON_A,
            Self::B => WPAD_BUTTON_B,
            Self::Plus => WPAD_BUTTON_PLUS,
            Self::Two => WPAD_BUTTON_2,
            Self::Home => WPAD_BUTTON_HOME,
            Self::Z => WPAD_NUNCHUK_BUTTON_Z,
            Self::C => WPAD_NUNCHUK_BUTTON_C,
        }
    }
}

#[derive(Default)]
pub struct Controller {
    held: u32,
    pressed: u32,
    /// The Nunchuk stick, x to the right and y forward, `None` without a Nunchuk.
    pub stick: Option<[f32; 2]>,
    /// Where the Wii Remote points on the screen.
    pub pointer: Option<[f32; 2]>,
}

impl Controller {
    pub fn read() -> Self {
        unsafe {
            WPAD_ScanPads();
            let mut expansion = MaybeUninit::<Expansion>::zeroed();
            WPAD_Expansion(0, expansion.as_mut_ptr());
            let expansion = expansion.assume_init();
            let mut ir = MaybeUninit::<IrData>::zeroed();
            WPAD_IR(0, ir.as_mut_ptr());
            let ir = ir.assume_init();
            Self {
                held: WPAD_ButtonsHeld(0),
                pressed: WPAD_ButtonsDown(0),
                stick: (expansion.kind == WPAD_EXP_NUNCHUK).then(|| stick(&expansion.joystick)),
                pointer: (ir.valid != 0).then_some([ir.x, ir.y]),
            }
        }
    }

    pub const fn held(&self, button: Button) -> bool {
        self.held & button.mask() != 0
    }

    pub const fn pressed(&self, button: Button) -> bool {
        self.pressed & button.mask() != 0
    }

    /// The movement for the player, the stick walks forward and back and turns sideways.
    pub fn input(&self) -> Input {
        Input {
            direction: [0.0; 2],
            stick: [0.0, self.stick.map_or(0.0, |[_, y]| y)],
            jump: self.held(Button::C),
            sneak: self.held(Button::Z),
            fast: false,
        }
    }
}

// wiiuse gives the stick as an angle from up in degrees and a magnitude from 0 to 1
fn stick(joystick: &Joystick) -> [f32; 2] {
    let magnitude = if joystick.mag.is_finite() {
        joystick.mag.clamp(0.0, 1.0)
    } else {
        0.0
    };
    if magnitude < STICK_DEADZONE {
        return [0.0; 2];
    }
    let magnitude = (magnitude - STICK_DEADZONE) / (1.0 - STICK_DEADZONE);
    let angle = joystick.ang.to_radians();
    [magnitude * sinf(angle), magnitude * cosf(angle)]
}
