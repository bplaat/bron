/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The Wii Remote with Nunchuk, read with WPAD for the engine's Wii Remote controls.

use core::mem::MaybeUninit;

use libogc_sys::*;
use plaatcraft_engine::client::Controller;
use plaatcraft_engine::client::input::{Context, Pad, Wiimote};

const STICK_DEADZONE: f32 = 0.12;

const BUTTONS: [(u32, u32); 13] = [
    (WPAD_BUTTON_A, Wiimote::A),
    (WPAD_BUTTON_B, Wiimote::B),
    (WPAD_BUTTON_1, Wiimote::ONE),
    (WPAD_BUTTON_2, Wiimote::TWO),
    (WPAD_BUTTON_PLUS, Wiimote::PLUS),
    (WPAD_BUTTON_MINUS, Wiimote::MINUS),
    (WPAD_BUTTON_HOME, Wiimote::HOME),
    (WPAD_BUTTON_UP, Wiimote::UP),
    (WPAD_BUTTON_DOWN, Wiimote::DOWN),
    (WPAD_BUTTON_LEFT, Wiimote::LEFT),
    (WPAD_BUTTON_RIGHT, Wiimote::RIGHT),
    (WPAD_NUNCHUK_BUTTON_C, Wiimote::C),
    (WPAD_NUNCHUK_BUTTON_Z, Wiimote::Z),
];

fn buttons(wpad: u32) -> u32 {
    BUTTONS
        .iter()
        .filter(|&&(mask, _)| wpad & mask != 0)
        .fold(0, |buttons, &(_, button)| buttons | button)
}

pub struct Remote;

impl Remote {
    /// Starts WPAD with the pointer in the 2D screen space.
    pub fn new([width, height]: [f32; 2]) -> Self {
        unsafe {
            WPAD_Init();
            WPAD_SetDataFormat(WPAD_CHAN_ALL, WPAD_FMT_BTNS_ACC_IR);
            WPAD_SetVRes(0, width as u32, height as u32);
        }
        Self
    }
}

impl Controller for Remote {
    fn read(&mut self, context: Context) -> Pad {
        let wiimote = unsafe {
            WPAD_ScanPads();
            let mut expansion = MaybeUninit::<Expansion>::zeroed();
            WPAD_Expansion(0, expansion.as_mut_ptr());
            let expansion = expansion.assume_init();
            let mut ir = MaybeUninit::<IrData>::zeroed();
            WPAD_IR(0, ir.as_mut_ptr());
            let ir = ir.assume_init();
            Wiimote {
                held: buttons(WPAD_ButtonsHeld(0)),
                pressed: buttons(WPAD_ButtonsDown(0)),
                stick: (expansion.kind == WPAD_EXP_NUNCHUK).then(|| stick(&expansion.joystick)),
                pointer: (ir.valid != 0).then_some([ir.x, ir.y, ir.angle]),
            }
        };
        wiimote.pad(context)
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
