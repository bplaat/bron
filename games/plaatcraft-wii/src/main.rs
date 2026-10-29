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

use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::f32::consts::TAU;
use core::ffi::c_int;
use core::sync::atomic::{AtomicBool, Ordering};

use libogc_sys::*;
use plaatcraft_engine::camera::{Camera, Input};
use plaatcraft_engine::database;
use plaatcraft_engine::hotbar::{Hotbar, name};
use plaatcraft_engine::menu::{self, Action, Layout, Menu, Page, Row};
use plaatcraft_engine::particles::Particles;
use plaatcraft_engine::prediction::Prediction;
use plaatcraft_engine::protocol::{
    Announcement, Controls, GAME_PORT, MAX_CHAT_LENGTH, MAX_NAME_LENGTH, PlayerInfo, Request,
    Response, ServerFrame, Welcome, validate_name,
};
use plaatcraft_engine::raycast::{Hit, raycast};
use plaatcraft_engine::server::{HOST, Outgoing, Server, TICK};
use plaatcraft_engine::sound::{Movement, ground};
use plaatcraft_engine::world::{self, Coord};

use crate::hud::Hud;
use crate::input::{Button, Controller};
use crate::local::{LIMITS, Local, Saved};
use crate::render::{Display, Renderer};
use crate::settings::Settings;
use crate::sound::Sounds;
use crate::storage::{Opened, Storage, WorldFile};
use crate::stream::{RENDER_DISTANCE, Stream};
use crate::textures::Textures;

#[cfg(feature = "bench")]
mod bench;
mod font;
mod hud;
mod input;
mod local;
mod memory;
mod mesh;
mod network;
mod render;
mod settings;
mod sound;
mod storage;
mod stream;
mod sync;
mod textures;
mod ui;

// The Wii time base runs at a quarter of the 243 MHz bus clock
const TICKS_PER_SECOND: f64 = 60_750_000.0;
const LOOK_SPEED: f32 = 2.6;
const REPEAT_DELAY: f32 = 0.3;
const REPEAT_INTERVAL: f32 = 0.2;
const NAME_DURATION: f32 = 2.0;
// How long the held block swings when the player breaks or places a block, like the desktop.
const SWING_DURATION: f32 = 0.3;

// Double tapping jump within this time toggles flying, like in Minecraft
const DOUBLE_TAP: f32 = 0.3;
const NIGHT_DAYLIGHT: f32 = 0.035;
// The title screen background turns around the default world, like the desktop game.
const BACKGROUND_TURN_SPEED: f32 = 0.05;
// An IPv4 address with a port, like 192.168.100.200:47820.
const MAX_ADDRESS_LENGTH: usize = 21;
// Saves write to the slow SD card in the background, only the changed chunks with synced
// journal writes. The game autosaves every two minutes, when the pause menu opens and when
// the world closes, so a power loss loses little and the card isn't written all the time.
const AUTOSAVE_INTERVAL: f32 = 120.0;

// The power button of the Wii and of the Wii Remotes, libogc calls it from an interrupt
static POWER_PRESSED: AtomicBool = AtomicBool::new(false);

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
    menu: Option<Menu>,
    // The inventory's block grid is open
    inventory: bool,
    exit: bool,
    focus: usize,
    controller: Controller,
    sounds: Sounds,
    breaking: Repeat,
    placing: Repeat,
    last_jump: Option<f32>,
    name_shown: Option<(u8, f32)>,
    // The time base counts from 2000, which an f32 can't hold precisely, so time starts here
    started: u64,
    last_frame: f32,
    frames: u32,
    fps: u32,
    last_fps: f32,
    settings: Settings,
    storage: Storage,
    worlds: Vec<WorldFile>,
    // Whether a world of this Wii is played, the next field is its name
    active: bool,
    world_name: String,
    last_save: f32,
    message: Option<(String, f32)>,
    swing: f32,
    particles: Particles,
    network: network::Network,
    // The integrated server of a world played on this Wii, and what it saves
    server: Option<Server>,
    saved: Saved,
    // The terrain the guests asked for, which the stream's worker builds in turn
    jobs: VecDeque<(u32, Coord, Option<u64>)>,
    prediction: Prediction,
    // The id of the player in the played world
    id: u32,
    hosting: bool,
    joining: Option<f32>,
    players: Vec<PlayerInfo>,
    found: Vec<(SocketAddress, Announcement, f32)>,
    state_elapsed: f32,
    remote_revision: u64,
    others: plaatcraft_engine::avatars::Others,
}

impl Game {
    fn new() -> Option<Self> {
        let display = Display::new()?;
        unsafe {
            WPAD_Init();
            WPAD_SetDataFormat(WPAD_CHAN_ALL, WPAD_FMT_BTNS_ACC_IR);
            WPAD_SetVRes(0, display.width as u32, display.height as u32);
            SYS_SetPowerCallback(Some(power));
        }
        let renderer = Renderer::new(&display);
        let storage = Storage::new();
        let sqlite = storage.sqlite;
        let worlds = storage.worlds();
        let settings = storage.load_settings().unwrap_or_default();
        let mut sounds = Sounds::new();
        sounds.set_volume(settings.game.volume);
        Some(Self {
            display,
            renderer,
            storage,
            worlds,
            textures: Textures::load(),
            stream: Stream::start(sqlite),
            camera: background_camera(),
            hotbar: Hotbar::default(),
            target: None,
            night: false,
            menu: Some(Menu::default()),
            inventory: false,
            exit: false,
            focus: 0,
            controller: Controller::default(),
            sounds,
            breaking: Repeat::default(),
            placing: Repeat::default(),
            last_jump: None,
            name_shown: None,
            started: unsafe { gettime() },
            last_frame: 0.0,
            frames: 0,
            fps: 0,
            last_fps: 0.0,
            settings,
            active: false,
            world_name: String::new(),
            last_save: 0.0,
            message: None,
            swing: 0.0,
            particles: Particles::default(),
            network: network::Network::new(),
            server: None,
            saved: Saved::default(),
            jobs: VecDeque::new(),
            prediction: Prediction::default(),
            id: HOST,
            hosting: false,
            joining: None,
            players: Vec::new(),
            found: Vec::new(),
            state_elapsed: 0.0,
            remote_revision: 0,
            others: plaatcraft_engine::avatars::Others::default(),
        })
    }

    fn seconds(&self) -> f32 {
        (unsafe { gettime() }.wrapping_sub(self.started) as f64 / TICKS_PER_SECOND) as f32
    }
    fn show(&mut self, page: Option<Page>) {
        self.show_text(page, String::new());
    }
    // Shows a page, a text page with the text in its field, the shown list keeps its selection.
    fn show_text(&mut self, page: Option<Page>, text: String) {
        self.inventory = false;
        self.focus = 0;
        self.breaking.next = None;
        self.placing.next = None;
        let Some(page) = page else {
            self.menu = None;
            return;
        };
        let mut menu = self.menu.take().unwrap_or_default();
        menu.session = self.session();
        menu.username.clone_from(&self.settings.username);
        match page {
            Page::Worlds => {
                let selected = menu.selected;
                menu.show_list(page, self.world_rows(), selected);
                if !self.storage.ready {
                    menu.error = Some("Insert a FAT32 SD card to save worlds".into());
                }
            }
            Page::Multiplayer => menu.show_list(page, self.server_rows(), None),
            _ => menu.show(page, text),
        }
        self.menu = Some(menu);
    }
    fn world_rows(&self) -> Vec<Row> {
        let now = storage::unix_time();
        self.worlds
            .iter()
            .map(|world| Row {
                title: world.name.clone(),
                details: menu::played(now.saturating_sub(world.modified).max(0) as u64),
                enabled: true,
            })
            .collect()
    }
    fn server_rows(&self) -> Vec<Row> {
        self.found
            .iter()
            .map(|(address, world, _)| {
                let plural = if world.players == 1 { "" } else { "s" };
                let [a, b, c, d] = address.address.to_ne_bytes();
                Row {
                    title: world.world.clone(),
                    details: format!(
                        "{} - {} player{plural} - {a}.{b}.{c}.{d}",
                        world.host, world.players
                    ),
                    enabled: true,
                }
            })
            .collect()
    }
    fn page(&self) -> Option<Page> {
        self.menu.as_ref().map(|menu| menu.page)
    }
    fn selected_world(&self) -> Option<String> {
        let selected = self.menu.as_ref()?.selected?;
        Some(self.worlds.get(selected)?.name.clone())
    }
    fn tell(&mut self, text: impl Into<String>) {
        self.message = Some((text.into(), self.seconds()));
    }

    fn playing(&self) -> bool {
        self.active || self.stream.remote
    }
    fn session(&self) -> menu::Session {
        if self.stream.remote {
            menu::Session::Guest
        } else if self.active {
            menu::Session::Host {
                lan: self.hosting.then_some(GAME_PORT),
            }
        } else {
            menu::Session::None
        }
    }
    // Closes the world and shows the turning default world again.
    fn leave_world(&mut self) {
        self.serve(|server, world| server.stop(world));
        if self.active {
            self.stream.save(self.saved.clone());
            if let Err(error) = self.stream.flush() {
                self.tell(error);
            }
            self.storage.close();
        }
        self.server = None;
        self.close_network();
        self.active = false;
        self.stream.reset(world::WORLD_SEED);
        self.particles.particles.clear();
        self.target = None;
        self.camera = background_camera();
        self.night = false;
    }
    fn close_network(&mut self) {
        self.network.stop();
        self.hosting = false;
        self.joining = None;
        self.jobs.clear();
        self.players.clear();
    }
    // Saves the world of this Wii in the background, the server of a LAN world saves itself.
    fn save(&mut self) {
        if self.active {
            self.serve(|server, world| server.save_all(world));
            self.stream.save(self.saved.clone());
            self.last_save = self.seconds();
        }
    }

    // Saves the world of this Wii and waits until it's on the card.
    fn flush(&mut self) -> bool {
        self.save();
        match self.stream.flush() {
            Ok(()) => true,
            Err(error) => {
                self.tell(error);
                false
            }
        }
    }

    fn open(&mut self, name: &str) {
        match self.storage.open(name) {
            Ok(opened) => {
                self.world_name = name.into();
                self.start(opened);
            }
            Err(error) => self.tell(error),
        }
    }

    fn create(&mut self, name: &str) -> Result<(), String> {
        let seed = unsafe { gettime() }.wrapping_mul(6_364_136_223_846_793_005);
        let name = self.storage.create(name, seed)?;
        self.open(&name);
        Ok(())
    }

    // Plays an opened world of this Wii in its integrated server and joins it as the host.
    fn start(&mut self, opened: Opened) {
        self.close_network();
        self.stream.reset(opened.seed);
        self.saved = opened.saved;
        let mut world = Local {
            stream: &mut self.stream,
            saved: &mut self.saved,
            storage: &self.storage,
        };
        let fluids = world.saved.fluids.clone();
        let mut server = Server::new(&mut world, LIMITS, &fluids);
        let joined = server.join(&mut world, Some(HOST), &self.settings.username);
        match joined {
            Ok(welcome) => {
                self.server = Some(server);
                self.active = true;
                self.last_save = self.seconds();
                self.enter(&welcome);
            }
            Err(error) => {
                self.storage.close();
                self.tell(error);
            }
        }
    }

    // Starts playing as the welcomed player, of this Wii's world or of a LAN world.
    fn enter(&mut self, welcome: &Welcome) {
        self.camera = Camera::default();
        welcome.player.restore(&mut self.camera);
        self.camera.auto_jump = self.settings.game.auto_jump;
        self.hotbar = welcome.player.hotbar;
        self.night = welcome.player.night;
        self.prediction = Prediction::new(&self.camera);
        self.id = welcome.id;
        self.remote_revision = welcome.revision;
        self.particles.particles.clear();
        self.target = None;
        self.last_jump = None;
        self.players.clear();
        self.show(None);
    }

    // Runs the integrated server with the world and delivers what it sends.
    fn serve(&mut self, run: impl FnOnce(&mut Server, &mut Local<'_>)) {
        let Some(server) = &mut self.server else {
            return;
        };
        let mut world = Local {
            stream: &mut self.stream,
            saved: &mut self.saved,
            storage: &self.storage,
        };
        run(server, &mut world);
        for outgoing in server.outgoing() {
            self.deliver(outgoing);
        }
    }

    fn deliver(&mut self, outgoing: Outgoing) {
        match outgoing {
            Outgoing::To(HOST, response) => self.response(response),
            Outgoing::To(id, response) => self.network.respond(id, ServerFrame::Response(response)),
            Outgoing::All(response) => {
                if self.hosting {
                    self.network.broadcast(response.clone());
                }
                self.response(response);
            }
            Outgoing::Terrain(id, coord) => self.queue_job(id, coord, None),
            Outgoing::Blocks(id, coord, tag) => self.queue_job(id, coord, Some(tag)),
            Outgoing::CancelTerrain(id) => {
                self.jobs
                    .retain(|&(peer, _, tag)| peer != id || tag.is_some());
                self.stream.cancel_jobs(id);
            }
            Outgoing::Log(message) => self.tell(message),
        }
    }

    // Queues the terrain of a guest, a full queue answers without the chunk.
    fn queue_job(&mut self, id: u32, coord: Coord, tag: Option<u64>) {
        if self.jobs.len() < 256 {
            self.jobs.push_back((id, coord, tag));
            return;
        }
        let response = match tag {
            Some(tag) => Response::Blocks {
                coord,
                tag,
                chunk: None,
            },
            None => Response::Terrain {
                coord,
                revision: 0,
                chunk: None,
            },
        };
        self.network.respond(id, ServerFrame::Response(response));
    }

    // Sends a request to the server of the played world, here or on the LAN.
    fn send(&mut self, request: Request) {
        if self.stream.remote {
            self.network.request(request);
        } else {
            self.serve(|server, world| server.request(world, HOST, request));
        }
    }

    fn run(&mut self) {
        while !self.exit && !POWER_PRESSED.load(Ordering::Relaxed) {
            let now = self.seconds();
            let dt = (now - self.last_frame).min(0.1);
            self.last_frame = now;
            self.controller = Controller::read();
            self.network_events(now);
            if self.playing()
                && !self.page().is_some_and(Page::typing)
                && (self.controller.pressed(Button::Home) || self.controller.pressed(Button::Plus))
            {
                if self.page() == Some(Page::Settings) {
                    self.save_settings();
                }
                if self.menu.is_none() {
                    self.show(Some(Page::Pause));
                    self.save();
                } else {
                    self.show(None);
                }
            }
            if self.inventory {
                self.inventory();
            } else if self.menu.is_some() {
                self.menu();
            } else if self.playing() {
                self.play(dt, now);
            } else {
                self.show(Some(Page::Title));
            }
            self.stream.update(self.camera.chunk());
            if !self.playing() {
                self.camera.yaw = (self.camera.yaw + dt * BACKGROUND_TURN_SPEED) % TAU;
            } else {
                self.tick_player(dt);
                self.particles.update(dt, |p| self.stream.block(p));
                self.network_tick();
                if now - self.last_save >= AUTOSAVE_INTERVAL {
                    self.save();
                }
                if let Some(error) = self.stream.error() {
                    self.tell(error);
                }
            }
            self.others.update(self.players.clone());
            self.others.animate(dt);
            self.swing = (self.swing - dt).max(0.0);
            self.render(now);
            self.display.present();
            self.frames += 1;
            if now - self.last_fps >= 1.0 {
                self.fps = (self.frames as f32 / (now - self.last_fps) + 0.5) as u32;
                self.frames = 0;
                self.last_fps = now;
            }
        }
        self.leave_world();
    }

    // The engine's menus with the pointer and the D-pad, A presses and B goes back.
    fn menu(&mut self) {
        let size = [self.display.width, self.display.height];
        let (controller, session) = (self.controller, self.session());
        let Some(menu) = &mut self.menu else {
            return;
        };
        menu.session = session;
        let page = menu.page;
        if let Some([x, y, _]) = controller.pointer {
            menu.point(&Layout::new(menu, ui::PLATFORM, size, [0.0; 4]), [x, y]);
        }
        for (button, direction) in [
            (Button::Up, [0.0, -1.0]),
            (Button::Down, [0.0, 1.0]),
            (Button::Left, [-1.0, 0.0]),
            (Button::Right, [1.0, 0.0]),
        ] {
            if controller.pressed(button) {
                menu.step(&Layout::new(menu, ui::PLATFORM, size, [0.0; 4]), direction);
            }
        }
        let layout = Layout::new(menu, ui::PLATFORM, size, [0.0; 4]);
        let focused = menu.focused(&layout);
        if focused == Some(Action::Volume) {
            let volume = &mut self.settings.game.volume;
            let step = i16::from(controller.pressed(Button::Right))
                - i16::from(controller.pressed(Button::Left));
            *volume = (i16::from(*volume) + step * 10).clamp(0, menu::MAX_VOLUME.into()) as u8;
            if controller.pressed(Button::A)
                && let Some([x, y, _]) = controller.pointer
            {
                *volume = (layout.slider_fraction([x, y]) * 10.0 + 0.5) as u8 * 10;
            }
            self.sounds.set_volume(*volume);
        }
        // The keyboard erases with B and finishes with +
        if page.typing() {
            if controller.pressed(Button::B) && !menu.text.is_empty() {
                menu.type_key(Action::Erase, 0);
                return;
            }
            if controller.pressed(Button::Plus) {
                self.confirm(page);
                return;
            }
        }
        if controller.pressed(Button::B) {
            self.back(page);
        } else if controller.pressed(Button::A)
            && let Some(action) = focused
        {
            self.sounds.click();
            self.menu_action(page, action);
        }
    }

    fn menu_action(&mut self, page: Page, action: Action) {
        match action {
            Action::SinglePlayer => {
                self.storage.refresh();
                self.worlds = self.storage.worlds();
                self.show(Some(Page::Worlds));
            }
            Action::MultiPlayer => {
                self.network.discover();
                self.show(Some(Page::Multiplayer));
            }
            Action::Resume => self.show(None),
            Action::Settings => self.show(Some(Page::Settings)),
            Action::Quit => self.exit = true,
            Action::OpenToLan => self.network.host(&self.world_name, &self.settings.username),
            Action::QuitToTitle => {
                let guest = self.stream.remote;
                if guest || self.flush() {
                    self.leave_world();
                    self.show(Some(if guest {
                        Page::Multiplayer
                    } else {
                        Page::Title
                    }));
                }
            }
            Action::Row(index) => {
                if let Some(menu) = &mut self.menu {
                    if menu.selected == Some(index) {
                        let action = if page == Page::Worlds {
                            Action::PlayWorld
                        } else {
                            Action::JoinServer
                        };
                        self.menu_action(page, action);
                    } else {
                        menu.selected = Some(index);
                    }
                }
            }
            Action::PlayWorld => {
                if let Some(name) = self.selected_world() {
                    self.open(&name);
                }
            }
            Action::CreateWorld => {
                let name = (1..)
                    .map(|index| format!("World {index}"))
                    .find(|name| {
                        !self
                            .worlds
                            .iter()
                            .any(|world| world.name.eq_ignore_ascii_case(name))
                    })
                    .expect("Free world name");
                self.show_text(Some(Page::Create), name);
            }
            // The world pages act on the selected world
            Action::RenameWorld | Action::DeleteWorld => {
                if let Some(name) = self.selected_world() {
                    let page = if action == Action::RenameWorld {
                        Page::Rename
                    } else {
                        Page::Delete
                    };
                    self.show_text(Some(page), name);
                }
            }
            Action::JoinServer => {
                let selected = self.menu.as_ref().and_then(|menu| menu.selected);
                if let Some(&(address, _, _)) = selected.and_then(|index| self.found.get(index)) {
                    self.join(address);
                }
            }
            Action::DirectConnect => self.show(Some(Page::DirectConnect)),
            Action::Confirm | Action::Enter => self.confirm(page),
            Action::Distance | Action::Volume => {}
            Action::Stats => self.settings.game.show_stats = !self.settings.game.show_stats,
            Action::AutoJump => {
                self.settings.game.auto_jump = !self.settings.game.auto_jump;
                self.camera.auto_jump = self.settings.game.auto_jump;
            }
            Action::Username => {
                self.show_text(Some(Page::Username), self.settings.username.clone());
            }
            Action::Reset => {
                self.settings.game = Settings::default().game;
                self.camera.auto_jump = self.settings.game.auto_jump;
                self.sounds.set_volume(self.settings.game.volume);
            }
            Action::Tool(index) if menu::TOOLS[index].1.is_empty() => {
                self.show(Some(Page::Chat));
            }
            Action::Tool(index) => {
                self.command(menu::TOOLS[index].1);
                self.show(None);
            }
            Action::Key(_)
            | Action::Space
            | Action::Erase
            | Action::Tab
            | Action::Shift(_)
            | Action::CapsLock => {
                let max = match page {
                    Page::Chat => MAX_CHAT_LENGTH,
                    Page::Username => MAX_NAME_LENGTH,
                    Page::DirectConnect => MAX_ADDRESS_LENGTH,
                    _ => database::MAX_NAME_LENGTH,
                };
                if let Some(menu) = &mut self.menu {
                    menu.type_key(action, max);
                }
            }
            Action::Back => self.back(page),
        }
    }

    fn back(&mut self, page: Page) {
        match page {
            Page::Title => {}
            Page::Create | Page::Rename | Page::Delete => self.show(Some(Page::Worlds)),
            Page::DirectConnect => self.show(Some(Page::Multiplayer)),
            Page::Username => self.show(Some(Page::Settings)),
            Page::Settings => {
                self.save_settings();
                self.show(Some(if self.playing() {
                    Page::Pause
                } else {
                    Page::Title
                }));
            }
            Page::Multiplayer => {
                self.joining = None;
                self.show(Some(Page::Title));
            }
            Page::Worlds => self.show(Some(Page::Title)),
            Page::Pause | Page::Tools | Page::Chat => self.show(None),
        }
    }

    // The confirm button or + of the text and delete pages.
    fn confirm(&mut self, page: Page) {
        let Some(menu) = &mut self.menu else {
            return;
        };
        let text = String::from(menu.text.trim());
        let error = match page {
            Page::DirectConnect => match network::parse_address(&text) {
                Some(address) => {
                    self.join(address);
                    None
                }
                None => Some("Type an IPv4 address, optionally with :port".into()),
            },
            Page::Username => match validate_name(&text) {
                Ok(name) => {
                    self.settings.username = name.into();
                    self.back(page);
                    None
                }
                Err(error) => Some(error),
            },
            Page::Create => self.create(&text).err(),
            Page::Rename => {
                let renamed = self
                    .selected_world()
                    .map(|name| self.storage.rename(&name, &text));
                match renamed {
                    Some(Err(error)) => Some(error),
                    _ => {
                        self.worlds = self.storage.worlds();
                        self.back(page);
                        None
                    }
                }
            }
            Page::Delete => {
                if let Some(name) = self.selected_world() {
                    if !self.storage.delete(&name) {
                        self.tell("Could not delete the world");
                    }
                    self.worlds = self.storage.worlds();
                }
                self.show(Some(Page::Worlds));
                None
            }
            Page::Chat => {
                self.command(&text);
                self.show(None);
                None
            }
            _ => None,
        };
        if let Some(menu) = &mut self.menu
            && error.is_some()
        {
            menu.error = error;
        }
    }

    fn save_settings(&mut self) {
        if let Err(error) = self.storage.save_settings(&self.settings) {
            self.tell(error);
        }
    }

    fn join(&mut self, address: SocketAddress) {
        self.close_network();
        self.joining = Some(self.seconds());
        self.network.connect(address, &self.settings.username);
        self.tell("Connecting...");
        if self.page() != Some(Page::Multiplayer) {
            self.show(Some(Page::Multiplayer));
        }
    }

    fn network_events(&mut self, now: f32) {
        for event in self.network.events() {
            match event {
                network::Event::Status(message) => self.tell(message),
                // The world can be left before it opened
                network::Event::Hosted if self.server.is_some() => {
                    self.hosting = true;
                    self.tell(format!(
                        "World opened to LAN (up to {} guests)",
                        network::MAX_CONNECTIONS
                    ));
                }
                network::Event::Found(address, announcement) => {
                    if let Some(found) = self
                        .found
                        .iter_mut()
                        .find(|(a, _, _)| a.address == address.address && a.port == address.port)
                    {
                        *found = (address, announcement, now);
                    } else if self.found.len() < 16 {
                        self.found.push((address, announcement, now));
                    }
                }
                // Guests play with the id of their connection, their datagrams use it
                network::Event::Joined(id, name) if self.hosting => {
                    let mut joined = Err(String::new());
                    self.serve(|server, world| joined = server.join(world, Some(id), &name));
                    let frame = match joined {
                        Ok(welcome) => ServerFrame::Accepted {
                            welcome,
                            token: unsafe { gettime() }.wrapping_mul(6_364_136_223_846_793_005)
                                ^ u64::from(id),
                            udp_port: GAME_PORT,
                        },
                        Err(error) => ServerFrame::Rejected(error),
                    };
                    self.network.respond(id, frame);
                }
                network::Event::Request(id, request) if self.hosting => {
                    self.serve(|server, world| server.request(world, id, request));
                }
                // The connection to a LAN world has id 0, which is the host's id for the server
                network::Event::Left(0) => {
                    if self.stream.remote || self.joining.is_some() {
                        self.leave_world();
                        self.network.discover();
                        self.show(Some(Page::Multiplayer));
                        self.tell("Disconnected from the server");
                    }
                }
                network::Event::Left(id) => self.serve(|server, world| server.leave(world, id)),
                network::Event::Server(frame) => self.server_frame(*frame),
                _ => {}
            }
        }
        self.found.retain(|(_, _, seen)| now - *seen < 5.0);
        let rows = self.server_rows();
        if let Some(menu) = &mut self.menu
            && menu.page == Page::Multiplayer
        {
            let selected = menu.selected;
            menu.set_rows(rows, selected);
        }
        if self.joining.is_some_and(|started| now - started > 20.0) {
            self.joining = None;
            self.network.stop();
            self.tell("Connection timed out");
        }
    }

    fn server_frame(&mut self, frame: ServerFrame) {
        match frame {
            ServerFrame::Accepted { welcome, .. } if self.joining.is_some() => {
                self.joining = None;
                self.active = false;
                self.server = None;
                self.stream.reset(world::WORLD_SEED);
                self.stream.remote = true;
                self.enter(&welcome);
                self.tell("Connected");
            }
            ServerFrame::Rejected(message) => {
                self.joining = None;
                self.tell(message);
                self.network.stop();
            }
            ServerFrame::Response(response) if self.stream.remote => self.response(response),
            _ => {}
        }
    }

    // Handles a response of the server, of this Wii's world or of a LAN world.
    fn response(&mut self, response: Response) {
        match response {
            Response::Terrain {
                coord,
                revision,
                chunk,
            } => self.stream.receive(coord, revision, chunk),
            // The integrated server changed the stream itself already
            Response::Edited {
                edits,
                revision: Some(revision),
                by,
            } if self.stream.remote && revision > self.remote_revision => {
                for edit in &edits {
                    // The player's own breaks burst when it breaks them
                    if by != self.id
                        && edit.material == 0
                        && let Some(material) = self.stream.block(edit.position)
                    {
                        self.particles.burst(edit.position, material);
                    }
                }
                self.remote_revision = revision;
                self.stream.invalidate(&edits, revision);
            }
            Response::State {
                sequence,
                body,
                others,
            } => {
                let flying = self.camera.flying;
                let stream = &mut self.stream;
                self.prediction
                    .reconcile(sequence, body, &mut self.camera, |p| stream.block(p));
                if self.camera.flying != flying {
                    self.sounds.fly(self.camera.flying);
                }
                self.players = others;
            }
            Response::Time { night } => self.night = night,
            Response::Chat { name, text } => self.tell(format!("{name}: {text}")),
            Response::Message(message) => self.tell(message),
            Response::Disconnected(message) => {
                self.leave_world();
                self.network.discover();
                self.show(Some(Page::Multiplayer));
                self.tell(message);
            }
            _ => {}
        }
    }

    // Runs the player's physics ticks, ahead of the server, and the integrated server.
    fn tick_player(&mut self, dt: f32) {
        let controls = Controls {
            // The player stands still while a menu is open, like the paused desktop game
            input: if self.menu.is_none() {
                self.controller.input()
            } else {
                Input::default()
            },
            yaw: self.camera.yaw,
            pitch: self.camera.pitch,
            auto_jump: self.settings.game.auto_jump,
        };
        let (stream, sounds) = (&mut self.stream, &mut self.sounds);
        let requests = self.prediction.tick(
            dt,
            &mut self.camera,
            controls,
            |p| stream.block(p),
            |before, after, block| {
                let ground = block(ground(after));
                sounds.movement(Movement::of(before), after, ground);
            },
        );
        for request in requests {
            self.send(request);
        }
        if self.server.is_some() {
            self.state_elapsed += dt;
            while self.state_elapsed >= TICK {
                self.state_elapsed -= TICK;
                self.serve(|server, world| server.tick(world));
            }
        }
    }

    fn network_tick(&mut self) {
        if self.stream.remote {
            for coord in self.stream.requests() {
                self.network.request(Request::Terrain(coord));
            }
            return;
        }
        // The stream keeps the chunks around the guests too
        self.stream.centers = self.server.as_ref().map_or_else(Vec::new, Server::centers);
        while let Some(&(id, coord, tag)) = self.jobs.front() {
            if !self.stream.job(id, coord, tag) {
                break;
            }
            self.jobs.pop_front();
        }
        for result in self.stream.results() {
            let response = if let Some(tag) = result.tag {
                Response::Blocks {
                    coord: result.coord,
                    tag,
                    chunk: Some(result.chunk),
                }
            } else {
                Response::Terrain {
                    coord: result.coord,
                    revision: result.revision,
                    chunk: Some(result.chunk),
                }
            };
            self.network
                .respond(result.id, ServerFrame::Response(response));
        }
    }

    // Sends a chat message or a command to the server.
    fn command(&mut self, command: &str) {
        self.send(Request::Chat(command.into()));
    }

    // Selects or fills hotbar slots, the server keeps the hotbar for the save.
    fn set_hotbar(&mut self, hotbar: Hotbar) {
        if hotbar == self.hotbar {
            return;
        }
        if hotbar.material() != self.hotbar.material() {
            self.name_shown = Some((hotbar.material(), self.seconds()));
        }
        self.sounds.click();
        self.hotbar = hotbar;
        self.send(Request::Hotbar(hotbar));
    }

    fn inventory(&mut self) {
        ui::grid(
            &self.controller,
            &mut self.focus,
            world::MATERIALS.len(),
            self.display.width,
        );
        let mut hotbar = self.hotbar;
        if self.controller.pressed(Button::C) {
            hotbar.cycle(-1);
        }
        if self.controller.pressed(Button::Z) {
            hotbar.cycle(1);
        }
        if self.controller.pressed(Button::A) {
            hotbar.assign(hotbar.selected, world::MATERIALS[self.focus].0);
        }
        self.set_hotbar(hotbar);
        if self.controller.pressed(Button::B) || self.controller.pressed(Button::Minus) {
            self.show(None);
        }
    }

    // Looks around, selects blocks and edits, the physics runs in tick_player.
    fn play(&mut self, dt: f32, now: f32) {
        if self.controller.pressed(Button::Minus) {
            if self.controller.held(Button::Z) {
                if let Some(hit) = self.target
                    && let Some(material) = world::item_material(hit.material)
                {
                    let mut hotbar = self.hotbar;
                    hotbar.pick(material);
                    self.set_hotbar(hotbar);
                }
            } else {
                self.show(None);
                self.inventory = true;
            }
            return;
        }
        if self.controller.pressed(Button::One) {
            self.show(Some(Page::Tools));
            return;
        }
        let controller = self.controller;
        let turn = |value: f32| value * value.abs() * LOOK_SPEED * dt / 0.002;
        let yaw = controller.stick.map_or(0.0, |[x, _]| turn(x));
        let pitch = f32::from(u8::from(controller.held(Button::Up)))
            - f32::from(u8::from(controller.held(Button::Down)));
        self.camera.look(f64::from(yaw), f64::from(-turn(pitch)));
        if controller.pressed(Button::C) {
            if self.last_jump.is_some_and(|last| now - last < DOUBLE_TAP) {
                let flying = !self.camera.flying;
                self.send(Request::Chat(
                    if flying { "/fly on" } else { "/fly off" }.into(),
                ));
                // Predicted, the server's state confirms it
                self.camera.set_flying(flying);
                self.sounds.fly(flying);
                self.last_jump = None;
            } else {
                self.last_jump = Some(now);
            }
        }
        if controller.pressed(Button::Two) {
            self.command("/time");
        }
        let mut hotbar = self.hotbar;
        if controller.pressed(Button::Right) {
            hotbar.cycle(-1);
        }
        if controller.pressed(Button::Left) {
            hotbar.cycle(1);
        }
        self.set_hotbar(hotbar);
        self.aim();
        if self.breaking.update(&self.controller, Button::B, now) {
            self.edit(false);
        }
        if self.placing.update(&self.controller, Button::A, now) {
            self.edit(true);
        }
    }
    fn aim(&mut self) {
        let direction = self.camera.direction();
        let stream = &mut self.stream;
        self.target = raycast(self.camera.position, direction, |position| {
            stream.block(position)
        });
    }
    fn edit(&mut self, place: bool) {
        self.swing = SWING_DURATION;
        let Some(hit) = self.target else {
            return;
        };
        let edit = if place {
            let stream = &mut self.stream;
            hit.place_edit(self.hotbar.material(), self.camera.position, |p| {
                stream.block(p)
            })
        } else {
            hit.break_edit()
        };
        let Some(edit) = edit else {
            return;
        };
        let material = if place { edit.material } else { hit.material };
        self.send(Request::Edit(edit));
        self.sounds
            .block(&self.camera, edit.position, material, !place);
        if !place {
            self.particles.burst(hit.position, material);
        }
        self.aim();
    }

    // The block grid of the inventory, the Wii's version of the desktop inventory.
    fn draw_inventory(&self, hud: &mut Hud<'_>) {
        hud.rect(0.0, 0.0, hud.width, hud.height, 0x0000_00aa);
        hud.label(
            "Inventory",
            (hud.width - Hud::text_width("Inventory", 30.0)) / 2.0,
            36.0,
            30.0,
        );
        let page_start = ui::page_start(self.focus);
        let left = (hud.width - ui::CELL * ui::COLUMNS as f32) / 2.0;
        for i in page_start..(page_start + ui::INVENTORY_PAGE).min(world::MATERIALS.len()) {
            let cell = i - page_start;
            let x = left + (cell % ui::COLUMNS) as f32 * ui::CELL;
            let y = ui::INVENTORY_TOP + (cell / ui::COLUMNS) as f32 * ui::CELL;
            let color = if i == self.focus {
                0x7e88_bfff
            } else {
                0x3030_30ff
            };
            hud.rect(x, y, ui::CELL - 2.0, ui::CELL - 2.0, color);
            hud.block(world::MATERIALS[i].0, x + 6.0, y + 6.0, 30.0);
        }
        hud.label(name(world::MATERIALS[self.focus].0), 24.0, 358.0, 20.0);
        let help = "A assign - C/Z slot - B close";
        hud.label(
            help,
            hud.width - 24.0 - Hud::text_width(help, 16.0),
            360.0,
            16.0,
        );
        hud.hotbar(&self.hotbar);
    }

    fn render(&mut self, now: f32) {
        self.renderer
            .begin(if self.night { NIGHT_DAYLIGHT } else { 1.0 });
        // Between the last two physics ticks, so the player moves smoothly
        let camera = if self.playing() {
            self.prediction.view(&self.camera)
        } else {
            self.camera
        };
        let view = self.renderer.view(&camera, &self.display);
        let center = camera.chunk();
        let in_game = self.menu.is_none() && !self.inventory;
        // The title pages show the turning default world behind them
        let eye = camera.position.map(|v| floor(v) as i64);
        let eye_material = self.stream.block(eye);
        self.renderer
            .draw_sky(&view, &self.textures, eye_material, now);
        self.renderer.draw_world(
            &view,
            center,
            &self.stream.chunks,
            &self.textures,
            eye_material,
        );
        if self.playing() {
            if in_game && let Some(hit) = self.target {
                self.renderer
                    .draw_selection(&view, center, hit.position, hit.material);
            }
            self.renderer
                .draw_players(&view, center, &self.others, &self.textures);
            self.renderer
                .draw_particles(&view, center, &self.particles, &self.textures);
            if in_game {
                self.renderer.draw_held(
                    self.hotbar.material(),
                    1.0 - self.swing / SWING_DURATION,
                    &self.textures,
                );
            }
        }
        let mut hud = Hud::begin(&self.display, &self.textures);
        if in_game {
            for (name, [x, y]) in self.others.name_tags(&view, center) {
                hud.label(name, x - Hud::text_width(name, 16.0) / 2.0, y, 16.0);
            }
        }
        if self.settings.game.show_stats && self.playing() {
            let [x, y, z] = self.camera.position;
            hud.label(
                &format!(
                    "{} FPS - {x:.0}, {y:.0}, {z:.0} - {} chunks - distance {RENDER_DISTANCE}",
                    self.fps,
                    self.stream.chunks.len()
                ),
                12.0,
                10.0,
                16.0,
            );
        }
        match &self.menu {
            _ if self.inventory => self.draw_inventory(&mut hud),
            Some(menu) => {
                let size = [hud.width, hud.height];
                let layout = Layout::new(menu, ui::PLATFORM, size, [0.0; 4]);
                let focused = menu.focused(&layout);
                menu.draw(&mut hud, &layout, size, focused, self.settings.game);
            }
            None => {
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
        if !in_game && let Some(pointer) = self.controller.pointer {
            hud.cursor(pointer);
        }
        if let Some((message, shown)) = &self.message
            && now - shown < 8.0
        {
            hud.rect(8.0, hud.height - 74.0, hud.width - 16.0, 24.0, 0x0000_00c0);
            hud.label(message, 14.0, hud.height - 70.0, 16.0);
        }
    }
}
// Looks over the default world from above its spawn, like the desktop title screen.
fn background_camera() -> Camera {
    Camera {
        pitch: -0.2,
        ..Camera::default()
    }
}

extern "C" fn power() {
    POWER_PRESSED.store(true, Ordering::Relaxed);
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
    if POWER_PRESSED.load(Ordering::Relaxed) {
        unsafe { SYS_ResetSystem(SYS_POWEROFF, 0, 0) };
    }
    0
}
