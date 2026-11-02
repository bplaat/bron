/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The Wii U GamePad and a Wii Remote with Nunchuk, read with VPAD and KPAD for the engine's
//! controls, both play at the same time.

use core::mem::MaybeUninit;

use plaatcraft_engine::client::Controller;
use plaatcraft_engine::client::input::{Context, Gamepad, Pad, Wiimote};
use wut_sys::*;

const STICK_DEADZONE: f32 = 0.12;
// The calibrated touch points are in the pixels of a 1280x720 screen
const TOUCH_SIZE: [f32; 2] = [1280.0, 720.0];

const GAMEPAD_BUTTONS: [(u32, u32); 15] = [
    (VPAD_BUTTON_A, Gamepad::A),
    (VPAD_BUTTON_B, Gamepad::B),
    (VPAD_BUTTON_X, Gamepad::X),
    (VPAD_BUTTON_Y, Gamepad::Y),
    (VPAD_BUTTON_L, Gamepad::L),
    (VPAD_BUTTON_R, Gamepad::R),
    (VPAD_BUTTON_ZL, Gamepad::ZL),
    (VPAD_BUTTON_ZR, Gamepad::ZR),
    (VPAD_BUTTON_PLUS, Gamepad::PLUS),
    (VPAD_BUTTON_MINUS, Gamepad::MINUS),
    (VPAD_BUTTON_UP, Gamepad::UP),
    (VPAD_BUTTON_DOWN, Gamepad::DOWN),
    (VPAD_BUTTON_LEFT, Gamepad::LEFT),
    (VPAD_BUTTON_RIGHT, Gamepad::RIGHT),
    (VPAD_BUTTON_STICK_L, Gamepad::STICK_L),
];

const WIIMOTE_BUTTONS: [(u32, u32); 13] = [
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
    (WPAD_BUTTON_C, Wiimote::C),
    (WPAD_BUTTON_Z, Wiimote::Z),
];

fn buttons(table: &[(u32, u32)], raw: u32) -> u32 {
    table
        .iter()
        .filter(|&&(mask, _)| raw & mask != 0)
        .fold(0, |buttons, &(_, button)| buttons | button)
}

fn stick(x: f32, y: f32) -> [f32; 2] {
    let magnitude = sqrtf(x * x + y * y);
    if !magnitude.is_finite() || magnitude < STICK_DEADZONE {
        return [0.0; 2];
    }
    let scale = ((magnitude.min(1.0) - STICK_DEADZONE) / (1.0 - STICK_DEADZONE)) / magnitude;
    [x * scale, y * scale]
}

pub struct Controllers {
    size: [f32; 2],
    gamepad: Gamepad,
    wiimote: Wiimote,
    connected: bool,
}

impl Controllers {
    /// Starts KPAD with the pointer and touches in the 2D screen space.
    pub fn new(size: [f32; 2]) -> Self {
        unsafe {
            VPADInit();
            KPADInit();
        }
        Self {
            size,
            gamepad: Gamepad::default(),
            wiimote: Wiimote::default(),
            connected: false,
        }
    }

    fn read_gamepad(&mut self) {
        let mut status = VPADStatus::default();
        let mut error = 0;
        let read = unsafe { VPADRead(VPAD_CHAN_0, &mut status, 1, &mut error) };
        let gamepad = &mut self.gamepad;
        gamepad.pressed = 0;
        gamepad.touched = false;
        if read < 1 || error != VPAD_READ_SUCCESS {
            return;
        }
        gamepad.held = buttons(&GAMEPAD_BUTTONS, status.hold);
        gamepad.pressed = buttons(&GAMEPAD_BUTTONS, status.trigger);
        gamepad.left = stick(status.left_stick.x, status.left_stick.y);
        gamepad.right = stick(status.right_stick.x, status.right_stick.y);
        let mut touch = VPADTouchData::default();
        unsafe { VPADGetTPCalibratedPoint(VPAD_CHAN_0, &mut touch, &status.tp_normal) };
        let touching = touch.touched != 0 && touch.validity == VPAD_VALID;
        gamepad.touched = touching && gamepad.touch.is_none();
        gamepad.touch = touching.then(|| {
            [
                f32::from(touch.x) * self.size[0] / TOUCH_SIZE[0],
                f32::from(touch.y) * self.size[1] / TOUCH_SIZE[1],
            ]
        });
    }

    fn read_wiimote(&mut self) {
        let mut status = MaybeUninit::<KPADStatus>::zeroed();
        let mut error = 0;
        let read = unsafe { KPADReadEx(WPAD_CHAN_0, status.as_mut_ptr(), 1, &mut error) };
        self.wiimote.pressed = 0;
        if read == 0 {
            // Without samples the Wii Remote is disconnected or nothing changed
            if error != KPAD_ERROR_NO_SAMPLES {
                self.wiimote = Wiimote::default();
                self.connected = false;
            }
            return;
        }
        let status = unsafe { status.assume_init() };
        let nunchuk = matches!(
            status.extension_type,
            WPAD_EXT_NUNCHUK | WPAD_EXT_MPLUS_NUNCHUK
        )
        .then(|| unsafe { status.extension.nunchuk });
        let (held, pressed) = nunchuk.map_or((0, 0), |n| (n.hold, n.trigger));
        self.connected = true;
        self.wiimote = Wiimote {
            held: buttons(&WIIMOTE_BUTTONS, status.hold | held),
            pressed: buttons(&WIIMOTE_BUTTONS, status.trigger | pressed),
            stick: nunchuk.map(|n| stick(n.stick.x, n.stick.y)),
            // KPAD points from -1 to 1 with y down, from the center of the screen
            pointer: (status.pos_valid > 0).then(|| {
                [
                    (status.pos.x + 1.0) * 0.5 * self.size[0],
                    (status.pos.y + 1.0) * 0.5 * self.size[1],
                    0.0,
                ]
            }),
        };
    }
}

impl Controller for Controllers {
    fn read(&mut self, context: Context) -> Pad {
        self.read_gamepad();
        self.read_wiimote();
        let pad = self.gamepad.pad(context);
        if self.connected {
            pad.merge(self.wiimote.pad(context))
        } else {
            pad
        }
    }
}
