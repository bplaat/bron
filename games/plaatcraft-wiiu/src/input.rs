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

const WIIMOTE_BUTTONS: [(u32, u32); 12] = [
    (WPAD_BUTTON_A, Wiimote::A),
    (WPAD_BUTTON_B, Wiimote::B),
    (WPAD_BUTTON_1, Wiimote::ONE),
    (WPAD_BUTTON_2, Wiimote::TWO),
    (WPAD_BUTTON_PLUS, Wiimote::PLUS),
    (WPAD_BUTTON_MINUS, Wiimote::MINUS),
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

fn wiimote_buttons(remote: u32, nunchuk: u32) -> u32 {
    // Nunchuk stick emulation shares the D-pad bits; only merge its C and Z buttons.
    buttons(
        &WIIMOTE_BUTTONS,
        remote | (nunchuk & (WPAD_BUTTON_C | WPAD_BUTTON_Z)),
    )
}

fn stick(x: f32, y: f32) -> [f32; 2] {
    let magnitude = sqrtf(x * x + y * y);
    if !magnitude.is_finite() || magnitude < STICK_DEADZONE {
        return [0.0; 2];
    }
    let scale = ((magnitude.min(1.0) - STICK_DEADZONE) / (1.0 - STICK_DEADZONE)) / magnitude;
    [x * scale, y * scale]
}

// KPAD's horizon vector uses screen coordinates, with positive y pointing down.
fn pointer_angle(angle: KPADVec2D) -> f32 {
    atan2f(angle.y, angle.x).to_degrees()
}

pub struct Controllers {
    size: [f32; 2],
    gamepad_size: [f32; 2],
    gamepad: Gamepad,
    wiimotes: [Wiimote; 4],
}

impl Controllers {
    /// Starts KPAD with the pointer and touches in the 2D screen space.
    pub fn new(size: [f32; 2], gamepad_size: [f32; 2]) -> Self {
        unsafe {
            VPADInit();
            KPADInit();
        }
        Self {
            size,
            gamepad_size,
            gamepad: Gamepad::default(),
            wiimotes: [Wiimote::default(); 4],
        }
    }

    fn read_gamepad(&mut self) {
        let mut status = VPADStatus::default();
        let mut error = 0;
        let read = unsafe { VPADRead(VPAD_CHAN_0, &mut status, 1, &mut error) };
        let gamepad = &mut self.gamepad;
        if read < 1 || error != VPAD_READ_SUCCESS {
            if error != VPAD_READ_NO_SAMPLES {
                *gamepad = Gamepad::default();
            } else {
                gamepad.pressed = 0;
                gamepad.touched = false;
            }
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
                f32::from(touch.x) * self.gamepad_size[0] / TOUCH_SIZE[0],
                f32::from(touch.y) * self.gamepad_size[1] / TOUCH_SIZE[1],
            ]
        });
    }

    fn read_wiimote(&mut self, channel: usize) {
        let mut status = MaybeUninit::<KPADStatus>::zeroed();
        let mut error = 0;
        let read = unsafe { KPADReadEx(channel as u32, status.as_mut_ptr(), 1, &mut error) };
        self.wiimotes[channel].pressed = 0;
        if read == 0 {
            // Without samples the Wii Remote is disconnected or nothing changed
            if error != KPAD_ERROR_NO_SAMPLES {
                self.wiimotes[channel] = Wiimote::default();
            }
            return;
        }
        let status = unsafe { status.assume_init() };
        if error != KPAD_ERROR_OK || status.error != 0 {
            self.wiimotes[channel] = Wiimote::default();
            return;
        }
        let nunchuk = matches!(
            status.extension_type,
            WPAD_EXT_NUNCHUK | WPAD_EXT_MPLUS_NUNCHUK
        )
        .then(|| unsafe { status.extension.nunchuk });
        let (held, pressed) = nunchuk.map_or((0, 0), |n| (n.hold, n.trigger));
        self.wiimotes[channel] = Wiimote {
            held: wiimote_buttons(status.hold, held),
            pressed: wiimote_buttons(status.trigger, pressed),
            stick: nunchuk.map(|n| stick(n.stick.x, n.stick.y)),
            // KPAD points from -1 to 1 with y down, from the center of the screen
            pointer: (status.pos_valid > 0).then(|| {
                [
                    (status.pos.x + 1.0) * 0.5 * self.size[0],
                    (status.pos.y + 1.0) * 0.5 * self.size[1],
                    pointer_angle(status.angle),
                ]
            }),
        };
    }
}

impl Controller for Controllers {
    fn poll(&mut self) {
        self.read_gamepad();
        for channel in 0..4 {
            self.read_wiimote(channel);
        }
    }

    fn cursor_index(&self, player: usize) -> Option<usize> {
        // The GamePad uses touch, while Remote channel numbers select the four hand images.
        player
            .checked_sub(1)
            .filter(|&channel| channel < self.wiimotes.len())
    }

    fn read_player(&self, player: usize, context: Context) -> Pad {
        if player == 0 {
            self.gamepad.pad(context)
        } else {
            self.wiimotes[player - 1].pad(context)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_is_reserved_for_the_wii_u_system_menu() {
        use plaatcraft_engine::client::input::Button;

        let remote = Wiimote {
            held: wiimote_buttons(WPAD_BUTTON_HOME, 0),
            pressed: wiimote_buttons(WPAD_BUTTON_HOME, 0),
            ..Wiimote::default()
        };
        for context in [Context::Play, Context::Menu, Context::Inventory] {
            assert!(!remote.pad(context).pressed(Button::Pause));
        }
    }

    #[test]
    fn remote_minus_plus_one_and_two_reach_the_game_actions() {
        use plaatcraft_engine::client::input::Button;

        for (raw, action) in [
            (0x1000, Button::Tools),
            (0x0010, Button::Pause),
            (0x0200, Button::Pick),
            (0x0100, Button::Inventory),
        ] {
            let remote = Wiimote {
                held: wiimote_buttons(raw, 0),
                pressed: wiimote_buttons(raw, 0),
                ..Wiimote::default()
            };
            let pad = remote.pad(Context::Play);
            assert!(pad.held(action) && pad.pressed(action));
        }
    }

    #[test]
    fn nunchuk_stick_emulation_does_not_press_the_remote_dpad() {
        for direction in [
            WPAD_BUTTON_LEFT,
            WPAD_BUTTON_RIGHT,
            WPAD_BUTTON_UP,
            WPAD_BUTTON_DOWN,
        ] {
            assert_eq!(wiimote_buttons(0, direction), 0);
            assert_eq!(
                wiimote_buttons(direction, 0),
                buttons(&WIIMOTE_BUTTONS, direction)
            );
        }
        assert_eq!(
            wiimote_buttons(
                WPAD_BUTTON_A,
                WPAD_BUTTON_C | WPAD_BUTTON_Z | WPAD_BUTTON_LEFT
            ),
            Wiimote::A | Wiimote::C | Wiimote::Z
        );
    }

    #[test]
    fn gamepad_is_player_one_and_the_four_remotes_are_players_two_to_five() {
        let mut controllers = Controllers {
            size: [640.0, 480.0],
            gamepad_size: [854.0, 480.0],
            gamepad: Gamepad {
                left: [0.2, 0.3],
                ..Gamepad::default()
            },
            wiimotes: core::array::from_fn(|channel| Wiimote {
                stick: Some([0.0, (channel + 1) as f32 / 5.0]),
                pressed: Wiimote::A,
                ..Wiimote::default()
            }),
        };
        assert_eq!(controllers.read_player(0, Context::Play).walk, [0.2, 0.3]);
        assert!(
            !controllers
                .read_player(0, Context::Menu)
                .pressed(plaatcraft_engine::client::input::Button::Select)
        );
        assert_eq!(controllers.cursor_index(0), None);
        for player in 1..=4 {
            assert_eq!(
                controllers.read_player(player, Context::Play).look,
                [0.0, player as f32 / 5.0]
            );
            assert_eq!(controllers.cursor_index(player), Some(player - 1));
        }
        controllers.local_players(1);
        assert_eq!(controllers.read_player(0, Context::Play).walk, [0.2, 0.3]);
        assert!(
            !controllers
                .read_player(0, Context::Menu)
                .pressed(plaatcraft_engine::client::input::Button::Select)
        );
    }

    #[test]
    fn gamepad_touch_clicks_without_a_hand_cursor() {
        let controllers = Controllers {
            size: [640.0, 480.0],
            gamepad_size: [854.0, 480.0],
            gamepad: Gamepad {
                touch: Some([320.0, 240.0]),
                touched: true,
                ..Gamepad::default()
            },
            wiimotes: [Wiimote::default(); 4],
        };
        for context in [Context::Menu, Context::Inventory] {
            let pad = controllers.read_player(0, context);
            assert!(pad.pressed(plaatcraft_engine::client::input::Button::Select));
            assert_eq!(pad.pointer, Some([320.0, 240.0, 0.0]));
            assert_eq!(controllers.cursor_index(0), None);
        }
    }

    #[test]
    fn gamepad_touch_uses_the_entire_primary_screen_in_every_context() {
        let mut controllers = Controllers {
            size: [640.0, 480.0],
            gamepad_size: [854.0, 480.0],
            gamepad: Gamepad {
                touch: Some([800.0, 450.0]),
                touched: true,
                ..Gamepad::default()
            },
            wiimotes: [Wiimote::default(); 4],
        };
        for count in 1..=5 {
            controllers.local_players(count);
            for context in [Context::Play, Context::Menu, Context::Inventory] {
                let pad = controllers.read_player(0, context);
                assert_eq!(pad.pointer, Some([800.0, 450.0, 0.0]));
                let button = if context == Context::Play {
                    plaatcraft_engine::client::input::Button::Tap
                } else {
                    plaatcraft_engine::client::input::Button::Select
                };
                assert!(pad.pressed(button));
            }
        }
    }

    #[test]
    fn horizon_vectors_produce_cursor_roll_in_degrees() {
        for ([x, y], expected) in [
            ([1.0, 0.0], 0.0),
            ([1.0, 1.0], 45.0),
            ([0.0, 1.0], 90.0),
            ([1.0, -1.0], -45.0),
            ([0.0, -1.0], -90.0),
            ([-1.0, 0.0], 180.0),
        ] {
            assert!((pointer_angle(KPADVec2D { x, y }) - expected).abs() < 0.001);
        }
    }
}
