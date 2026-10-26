/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! PlaatCraft for the Wii, the plaatcraft-engine world rendered with GX and played with a
//! Wii Remote and Nunchuk.

#![no_std]
#![no_main]
#![cfg_attr(feature = "bench", allow(dead_code))]

use alloc::format;
use core::ffi::c_int;
use core::sync::atomic::{AtomicBool, Ordering};

use libogc_sys::*;
use plaatcraft_engine::camera::Camera;
use plaatcraft_engine::hotbar::{Hotbar, name};
use plaatcraft_engine::raycast::{Hit, raycast};
use plaatcraft_engine::world::{self, LAVA, WATER};

use crate::hud::{BUTTON_HEIGHT, BUTTON_WIDTH, Hud};
use crate::input::{Button, Controller};
use crate::render::{Display, Renderer};
use crate::stream::Stream;
use crate::textures::Textures;

#[cfg(feature = "bench")]
mod bench;
mod font;
mod hud;
mod input;
mod memory;
mod mesh;
mod render;
mod stream;
mod sync;
mod textures;

// The Wii time base runs at a quarter of the 243 MHz bus clock
const TICKS_PER_SECOND: f64 = 60_750_000.0;
const LOOK_SPEED: f32 = 2.6;
const REPEAT_DELAY: f32 = 0.3;
const REPEAT_INTERVAL: f32 = 0.2;
const NAME_DURATION: f32 = 2.0;
const NIGHT_DAYLIGHT: f32 = 0.035;
const MENU: [&str; 3] = ["Play", "Night", "Exit"];

static RUNNING: AtomicBool = AtomicBool::new(true);

// A held button that repeats its action, like holding a mouse button
#[derive(Default)]
struct Repeat {
    next: Option<f32>,
}

impl Repeat {
    fn update(&mut self, controller: &Controller, button: Button, now: f32) -> bool {
        if controller.pressed(button) {
            self.next = Some(now + REPEAT_DELAY);
            return true;
        }
        match self.next {
            Some(next) if controller.held(button) && now >= next => {
                self.next = Some(now + REPEAT_INTERVAL);
                true
            }
            Some(_) if !controller.held(button) => {
                self.next = None;
                false
            }
            _ => false,
        }
    }
}

struct Game {
    display: Display,
    renderer: Renderer,
    textures: Textures,
    stream: Stream,
    camera: Camera,
    hotbar: Hotbar,
    target: Option<Hit>,
    night: bool,
    paused: bool,
    focus: usize,
    controller: Controller,
    breaking: Repeat,
    placing: Repeat,
    name_shown: Option<(u8, f32)>,
    // The time base counts from 2000, which an f32 can't hold precisely, so time starts here
    started: u64,
    last_frame: f32,
    frames: u32,
    fps: u32,
    last_fps: f32,
}

impl Game {
    fn new() -> Option<Self> {
        let display = Display::new()?;
        unsafe {
            WPAD_Init();
            WPAD_SetDataFormat(WPAD_CHAN_ALL, WPAD_FMT_BTNS_ACC_IR);
            WPAD_SetVRes(0, display.width as u32, display.height as u32);
            SYS_SetPowerCallback(Some(poweroff));
            WPAD_SetPowerButtonCallback(Some(wpad_poweroff));
        }
        let renderer = Renderer::new(&display);
        world::set_seed(world::WORLD_SEED);
        Some(Self {
            display,
            renderer,
            textures: Textures::load(),
            stream: Stream::start(),
            camera: Camera::default(),
            hotbar: Hotbar::default(),
            target: None,
            night: false,
            paused: true,
            focus: 0,
            controller: Controller::default(),
            breaking: Repeat::default(),
            placing: Repeat::default(),
            name_shown: None,
            started: unsafe { gettime() },
            last_frame: 0.0,
            frames: 0,
            fps: 0,
            last_fps: 0.0,
        })
    }

    fn seconds(&self) -> f32 {
        (unsafe { gettime() }.wrapping_sub(self.started) as f64 / TICKS_PER_SECOND) as f32
    }

    fn run(&mut self) {
        while RUNNING.load(Ordering::Relaxed) {
            let now = self.seconds();
            let dt = now - self.last_frame;
            self.last_frame = now;
            self.controller = Controller::read();
            if self.controller.pressed(Button::Home) {
                self.paused = !self.paused;
                self.focus = 0;
            }
            if self.paused {
                self.menu();
            } else {
                self.play(dt, now);
            }
            self.stream.update(self.camera.chunk());
            self.render(now);
            self.display.present();
            self.frames += 1;
            if now - self.last_fps >= 1.0 {
                self.fps = (self.frames as f32 / (now - self.last_fps) + 0.5) as u32;
                self.frames = 0;
                self.last_fps = now;
            }
        }
        unsafe { WPAD_Disconnect(WPAD_CHAN_ALL) };
    }

    fn menu_button_y(&self, index: usize) -> f32 {
        self.display.height / 2.0 - 40.0 + index as f32 * (BUTTON_HEIGHT + 12.0)
    }

    fn menu_button_x(&self) -> f32 {
        (self.display.width - BUTTON_WIDTH) / 2.0
    }

    fn menu(&mut self) {
        let controller = &self.controller;
        if controller.pressed(Button::Up) {
            self.focus = (self.focus + MENU.len() - 1) % MENU.len();
        }
        if controller.pressed(Button::Down) {
            self.focus = (self.focus + 1) % MENU.len();
        }
        if let Some([x, y]) = controller.pointer {
            let left = self.menu_button_x();
            if let Some(index) = (0..MENU.len()).find(|&index| {
                let top = self.menu_button_y(index);
                x >= left && x < left + BUTTON_WIDTH && y >= top && y < top + BUTTON_HEIGHT
            }) {
                self.focus = index;
            }
        }
        if self.controller.pressed(Button::A) {
            match self.focus {
                0 => self.paused = false,
                1 => self.night = !self.night,
                _ => RUNNING.store(false, Ordering::Relaxed),
            }
        }
    }

    fn play(&mut self, dt: f32, now: f32) {
        let controller = &self.controller;
        if let Some([x, y]) = controller.stick {
            // A curve keeps small stick movements precise
            let turn = |value: f32| value * value.abs() * LOOK_SPEED * dt / 0.002;
            self.camera.look(f64::from(turn(x)), f64::from(-turn(y)));
        }
        if controller.pressed(Button::One) {
            self.camera.set_flying(!self.camera.flying);
        }
        if controller.pressed(Button::Two) {
            self.night = !self.night;
        }
        let mut hotbar = self.hotbar;
        if controller.pressed(Button::Plus) {
            hotbar.cycle(-1);
        }
        if controller.pressed(Button::Minus) {
            hotbar.cycle(1);
        }
        if hotbar.material() != self.hotbar.material() {
            self.name_shown = Some((hotbar.material(), now));
        }
        self.hotbar = hotbar;
        self.camera.input = controller.input();

        // The feet or waist in water or lava make the player swim
        let [x, y, z] = self.camera.position;
        self.camera.in_liquid = [0.2, 0.9].into_iter().any(|offset| {
            let position = [
                floor(x) as i64,
                floor(y - 1.7 + offset) as i64,
                floor(z) as i64,
            ];
            matches!(self.stream.block(position), Some(WATER | LAVA))
        });
        let stream = &mut self.stream;
        self.camera.update(dt, |position| {
            position[1] < 0
                || !matches!(stream.block(position), Some(material) if world::is_passable(material))
        });

        self.aim();
        if self.breaking.update(&self.controller, Button::Z, now) {
            self.edit(false);
        }
        if self.placing.update(&self.controller, Button::C, now) {
            self.edit(true);
        }
    }

    fn aim(&mut self) {
        let (sin, cos) = (sinf(self.camera.yaw), cosf(self.camera.yaw));
        let (sp, cp) = (sinf(self.camera.pitch), cosf(self.camera.pitch));
        let direction = [f64::from(sin * cp), f64::from(sp), f64::from(-cos * cp)];
        let stream = &mut self.stream;
        self.target = raycast(self.camera.position, direction, |position| {
            stream.block(position)
        });
    }

    fn edit(&mut self, place: bool) {
        let Some(hit) = self.target else {
            return;
        };
        let edit = if place {
            let stream = &mut self.stream;
            hit.place_edit(self.hotbar.material(), self.camera.position, |position| {
                stream.block(position)
            })
        } else {
            hit.break_edit()
        };
        let Some(edit) = edit else {
            return;
        };
        self.stream.edit(edit);
        self.aim();
    }

    fn render(&mut self, now: f32) {
        let daylight = if self.night { NIGHT_DAYLIGHT } else { 1.0 };
        self.renderer.begin(daylight);
        let view = self.renderer.view(&self.camera, &self.display);
        let center = self.camera.chunk();
        let eye = self.camera.position.map(|v| floor(v) as i64);
        let eye_material = self.stream.block(eye);
        self.renderer.draw_world(
            &view,
            center,
            &self.stream.chunks,
            &self.textures,
            eye_material,
        );
        if !self.paused
            && let Some(hit) = self.target
        {
            self.renderer.draw_selection(&view, center, hit.position);
        }

        let mut hud = Hud::begin(&self.display, &self.textures);
        let [x, y, z] = self.camera.position;
        hud.label(
            &format!(
                "{} FPS - {x:.0}, {y:.0}, {z:.0} - {} chunks",
                self.fps,
                self.stream.chunks.len()
            ),
            12.0,
            10.0,
            16.0,
        );
        if self.paused {
            hud.rect(0.0, 0.0, hud.width, hud.height, 0x0000_0080);
            let title = "PlaatCraft";
            let size = 56.0;
            hud.label(
                title,
                (hud.width - Hud::text_width(title, size)) / 2.0,
                self.menu_button_y(0) - 100.0,
                size,
            );
            for (index, text) in MENU.iter().enumerate() {
                let text = match (index, self.night) {
                    (1, true) => "Day",
                    _ => text,
                };
                hud.button(
                    text,
                    self.menu_button_x(),
                    self.menu_button_y(index),
                    self.focus == index,
                );
            }
            let help = if self.controller.stick.is_some() {
                "D-pad move - Stick look - A up - B down - Z break - C place"
            } else {
                "Connect a Nunchuk to look around and build"
            };
            hud.label(
                help,
                (hud.width - Hud::text_width(help, 15.0)) / 2.0,
                hud.height - 70.0,
                15.0,
            );
            let help = "+/- block - 1 fly - 2 night - Home menu";
            hud.label(
                help,
                (hud.width - Hud::text_width(help, 15.0)) / 2.0,
                hud.height - 46.0,
                15.0,
            );
            if let Some(pointer) = self.controller.pointer {
                hud.cursor(pointer);
            }
        } else {
            hud.crosshair();
            let top = hud.hotbar(&self.hotbar);
            if let Some((material, shown)) = self.name_shown
                && now - shown < NAME_DURATION
            {
                let text = name(material);
                hud.label(
                    text,
                    (hud.width - Hud::text_width(text, 18.0)) / 2.0,
                    top - 30.0,
                    18.0,
                );
            }
            if self.camera.flying {
                hud.label("Flying", 12.0, 32.0, 16.0);
            }
        }
    }
}

extern "C" fn poweroff() {
    RUNNING.store(false, Ordering::Relaxed);
}

extern "C" fn wpad_poweroff(channel: c_int) {
    if channel == WPAD_CHAN_ALL {
        RUNNING.store(false, Ordering::Relaxed);
    }
}

#[cfg(feature = "bench")]
#[unsafe(export_name = "main")]
pub extern "C" fn bench_main() -> c_int {
    bench::run()
}

#[cfg(not(feature = "bench"))]
#[unsafe(export_name = "main")]
pub extern "C" fn app_main() -> c_int {
    let Some(mut game) = Game::new() else {
        return 1;
    };
    game.run();
    0
}
