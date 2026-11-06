/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The Wii Remote with Nunchuk, read with WPAD for the engine's Wii Remote controls.

use alloc::format;
use alloc::vec::Vec;
use core::mem::MaybeUninit;

use libogc_sys::*;
use plaatcraft_engine::client::Controller;
use plaatcraft_engine::client::desktop::Desktop;
use plaatcraft_engine::client::input::{Context, Pad, Wiimote};
use plaatcraft_engine::menu::Row;

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

pub struct Remote {
    remotes: [Wiimote; 4],
    desktop: Desktop,
}

impl Remote {
    /// Starts WPAD with the pointer in the 2D screen space.
    pub fn new([width, height]: [f32; 2]) -> Self {
        crate::usb::start();
        unsafe {
            WPAD_Init();
            WPAD_SetDataFormat(WPAD_CHAN_ALL, WPAD_FMT_BTNS_ACC_IR);
            for channel in 0..4 {
                WPAD_SetVRes(channel, width as u32, height as u32);
            }
        }
        Self {
            remotes: [Wiimote::default(); 4],
            desktop: Desktop::new([width, height]),
        }
    }
}

impl Controller for Remote {
    fn poll(&mut self) {
        crate::usb::poll();
        self.desktop
            .poll(&crate::usb::EVENTS, crate::system::seconds());
        unsafe {
            WPAD_ScanPads();
        }
        for channel in 0..4 {
            self.remotes[channel as usize] = unsafe {
                let mut kind = 0;
                if WPAD_Probe(channel, &mut kind) != 0 {
                    self.remotes[channel as usize] = Wiimote::default();
                    continue;
                }
                let mut expansion = MaybeUninit::<Expansion>::zeroed();
                WPAD_Expansion(channel, expansion.as_mut_ptr());
                let expansion = expansion.assume_init();
                let mut ir = MaybeUninit::<IrData>::zeroed();
                WPAD_IR(channel, ir.as_mut_ptr());
                let ir = ir.assume_init();
                Wiimote {
                    held: buttons(WPAD_ButtonsHeld(channel)),
                    pressed: buttons(WPAD_ButtonsDown(channel)),
                    stick: (expansion.kind == WPAD_EXP_NUNCHUK).then(|| stick(&expansion.joystick)),
                    pointer: Wiimote::smooth_pointer(
                        self.remotes[channel as usize].pointer,
                        (ir.valid != 0).then_some([ir.x, ir.y, ir.angle]),
                    ),
                }
            };
        }
    }

    fn read_player(&self, player: usize, context: Context) -> Pad {
        self.remotes[player].pad(context)
    }

    fn desktop(&self, context: Context) -> Pad {
        self.desktop.pad(context)
    }
    fn text(&self) -> &str {
        self.desktop.text()
    }
    fn mouse_pointer(&self) -> Option<[f32; 2]> {
        self.desktop.pointer()
    }

    fn status(&self) -> Vec<Row> {
        (0..4)
            .map(|channel| {
                let mut kind = 0;
                let connected = unsafe { WPAD_Probe(channel, &mut kind) == 0 };
                Row {
                    title: format!("Player {} - Wii Remote {}", channel + 1, channel + 1),
                    details: if connected {
                        let battery = unsafe { WPAD_BatteryLevel(channel) };
                        let extension = if kind == WPAD_EXP_NUNCHUK as u32 {
                            "Nunchuk ready"
                        } else {
                            "Attach a Nunchuk to play"
                        };
                        format!("{extension} - Battery {battery}%")
                    } else {
                        "Not connected - press 1 + 2".into()
                    },
                    enabled: false,
                }
            })
            .collect()
    }

    fn reconnect(&mut self) {
        for channel in 0..4 {
            unsafe {
                WPAD_Disconnect(channel);
            }
        }
        self.remotes = [Wiimote::default(); 4];
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
