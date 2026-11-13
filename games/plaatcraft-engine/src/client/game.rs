/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The game loop of the consoles: the title screen over the turning default world, the
//! menus, playing a world of the SD card in the integrated server or a LAN world, and the
//! inventory's block grid.

use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::f32::consts::TAU;
use core::mem;

use super::files;
use super::hud::{self, Hud, text_width};
use super::input::{Button, Context, Pad};
use super::light::NIGHT_DAYLIGHT;
use super::local::{Local, Saved};
use super::network::{self, Network};
use super::platform::{Address, Controller, Gpu, Platform};
use super::render::Renderer;
use super::settings::Settings;
use super::sound::Sounds;
use super::storage::{Opened, Storage, WorldFile};
use super::stream::Stream;
use crate::avatars::Others;
use crate::camera::{Camera, Input};
use crate::database;
use crate::hotbar::{Hotbar, name};
#[cfg(not(feature = "std"))]
use crate::math::Float;
use crate::menu::{self, Action, Layout, Menu, Page, Row};
use crate::particles::Particles;
use crate::prediction::Prediction;
use crate::protocol::{
    Announcement, Controls, GAME_PORT, MAX_CHAT_LENGTH, MAX_NAME_LENGTH, PlayerInfo, Request,
    Response, ServerFrame, Welcome, validate_name,
};
use crate::raycast::{Hit, raycast};
use crate::server::{HOST, Outgoing, Server, TICK};
use crate::sound::{Footsteps, Movement, ground};
use crate::world::{self, Coord};

const LOOK_SPEED: f32 = 2.6;
const REPEAT_DELAY: f32 = 0.3;
const REPEAT_INTERVAL: f32 = 0.2;
const NAME_DURATION: f32 = 2.0;
// The chat shows the last few messages for a while, like the desktop chat.
const CHAT_RECENT: usize = 5;
const CHAT_TIME: f32 = 8.0;
// How long the held block swings when the player breaks or places a block, like the desktop.
const SWING_DURATION: f32 = 0.3;

// Double tapping jump within this time toggles flying, like in Minecraft
const DOUBLE_TAP: f32 = 0.3;
// The title screen background turns around the default world, like the desktop game.
const BACKGROUND_TURN_SPEED: f32 = 0.05;
// An IPv4 address with a port, like 192.168.100.200:47820.
const MAX_ADDRESS_LENGTH: usize = 21;
// Saves write to the slow SD card in the background, only the changed chunks with synced
// journal writes. The game autosaves every two minutes, when the pause menu opens and when
// the world closes, so a power loss loses little and the card isn't written all the time.
const AUTOSAVE_INTERVAL: f32 = 120.0;

// The inventory grid.
const CELL: f32 = 44.0;
const COLUMNS: usize = 9;
const INVENTORY_TOP: f32 = 84.0;
const ROWS: usize = 6;
const INVENTORY_PAGE: usize = COLUMNS * ROWS;

/// The first block of the inventory page with the focused block.
const fn page_start(focus: usize) -> usize {
    focus / INVENTORY_PAGE * INVENTORY_PAGE
}

// A held button that repeats its action, like holding a mouse button
#[derive(Default)]
struct Repeat {
    next: Option<f32>,
}

impl Repeat {
    fn update(&mut self, pad: &Pad, button: Button, now: f32) -> bool {
        if pad.pressed(button) {
            self.next = Some(now + REPEAT_DELAY);
            return true;
        }
        match self.next {
            Some(next) if pad.held(button) && now >= next => {
                self.next = Some(now + REPEAT_INTERVAL);
                true
            }
            Some(_) if !pad.held(button) => {
                self.next = None;
                false
            }
            _ => false,
        }
    }
}

#[derive(Clone, Copy, Default)]
struct MenuPointer {
    position: Option<[f32; 2]>,
    pointing: bool,
}

impl MenuPointer {
    fn moved(&mut self, pointer: Option<[f32; 3]>) -> Option<[f32; 2]> {
        let Some([x, y, _]) = pointer else {
            self.position = None;
            return None;
        };
        if !x.is_finite()
            || !y.is_finite()
            || self
                .position
                .is_some_and(|[px, py]| (x - px).abs() < 2.0 && (y - py).abs() < 2.0)
        {
            return None;
        }
        self.position = Some([x, y]);
        self.pointing = true;
        self.position
    }
}

// The independent controls and prediction of one local player.
struct LocalPlayer {
    menu: Option<Menu>,
    inventory: bool,
    focus: usize,
    menu_pointer: MenuPointer,
    camera: Camera,
    hotbar: Hotbar,
    target: Option<Hit>,
    pad: Pad,
    breaking: Repeat,
    placing: Repeat,
    last_jump: Option<f32>,
    name_shown: Option<(u8, f32)>,
    swing: f32,
    prediction: Prediction,
    id: u32,
    players: Vec<PlayerInfo>,
    others: Others,
    footsteps: Footsteps,
}

/// The game of a console, its port runs it with [`Game::run`].
pub struct Game<P: Platform> {
    player: LocalPlayer,
    locals: Vec<LocalPlayer>,
    player_index: usize,
    menu_pointers: Vec<MenuPointer>,
    gpu: P::Gpu,
    controller: P::Controller,
    renderer: Renderer,
    stream: Stream<P>,
    night: bool,
    exit: bool,
    sounds: Sounds<P::Audio>,
    // The system's clock can be far from zero, which an f32 can't hold precisely, so time
    // starts here
    started: f64,
    last_frame: f32,
    frames: u32,
    fps: u32,
    last_fps: f32,
    settings: Settings,
    storage: Storage<P>,
    worlds: Vec<WorldFile>,
    // Whether a world of this console is played, the next field is its name
    active: bool,
    world_name: String,
    last_save: f32,
    messages: VecDeque<(String, f32)>,
    particles: Particles,
    network: Network<P>,
    // The integrated server of a world played on this console, and what it saves
    server: Option<Server>,
    saved: Saved,
    // The terrain the guests asked for, which the stream's worker builds in turn
    jobs: VecDeque<(u32, Coord, Option<u64>)>,
    hosting: bool,
    joining: Option<f32>,
    found: Vec<(Address, Announcement, f32)>,
    state_elapsed: f32,
    remote_revision: u64,
}

impl LocalPlayer {
    fn new() -> Self {
        Self {
            menu: None,
            inventory: false,
            focus: 0,
            menu_pointer: MenuPointer::default(),
            camera: background_camera(),
            hotbar: Hotbar::default(),
            target: None,
            pad: Pad::default(),
            breaking: Repeat::default(),
            placing: Repeat::default(),
            last_jump: None,
            name_shown: None,
            swing: 0.0,
            prediction: Prediction::default(),
            id: HOST,
            players: Vec::new(),
            others: Others::default(),
            footsteps: Footsteps::default(),
        }
    }

    fn moved_menu_pointer(&mut self) -> Option<[f32; 2]> {
        self.menu_pointer.moved(self.pad.pointer)
    }
}

impl<P: Platform> Game<P> {
    /// Starts the game on the title screen, with the settings of the SD card.
    pub fn new(gpu: P::Gpu, audio: P::Audio, controller: P::Controller) -> Self {
        let renderer = Renderer::new(gpu.size(), P::RENDER_DISTANCE);
        let storage = Storage::new();
        let sqlite = storage.sqlite.clone();
        let worlds = storage.worlds();
        let settings = storage.load_settings().unwrap_or_else(Settings::new::<P>);
        let mut sounds = Sounds::new(audio);
        sounds.set_volume(settings.game.volume);
        let mut menu = Menu::default();
        menu.remotes = P::REMOTE_MANAGEMENT;
        let mut player = LocalPlayer::new();
        player.menu = Some(menu);
        Self {
            player_index: 0,
            menu_pointers: (0..=P::MAX_LOCAL_PLAYERS)
                .map(|_| MenuPointer::default())
                .collect(),
            player,
            locals: Vec::new(),
            gpu,
            controller,
            renderer,
            storage,
            worlds,
            stream: Stream::start(sqlite),
            night: false,
            exit: false,
            sounds,
            started: P::seconds(),
            last_frame: 0.0,
            frames: 0,
            fps: 0,
            last_fps: 0.0,
            settings,
            active: false,
            world_name: String::new(),
            last_save: 0.0,
            messages: VecDeque::new(),
            particles: Particles::default(),
            network: Network::new(),
            server: None,
            saved: Saved::default(),
            jobs: VecDeque::new(),
            hosting: false,
            joining: None,
            found: Vec::new(),
            state_elapsed: 0.0,
            remote_revision: 0,
        }
    }

    fn screen_players(&self) -> usize {
        if self.gpu.primary_screen_size().is_some() && !self.locals.is_empty() {
            self.locals.len()
        } else {
            self.locals.len() + 1
        }
    }

    fn view_size(&self) -> [f32; 2] {
        let [width, height] = self.gpu.size();
        let [_, _, w, h] = viewport(self.screen_players(), 0);
        [width * w / h, height]
    }

    fn control_size(&self) -> [f32; 2] {
        if self.player_index == 0
            && let Some(size) = self.gpu.primary_screen_size()
        {
            return size;
        }
        if self.global_menu() {
            return self.gpu.size();
        }
        self.view_size()
    }

    fn global_menu(&self) -> bool {
        self.player
            .menu
            .as_ref()
            .is_some_and(|menu| !matches!(menu.page, Page::Tools | Page::Chat))
    }

    fn map_pointer(&mut self, index: usize) {
        if self.global_menu() || (index == 0 && self.gpu.primary_screen_size().is_some()) {
            return;
        }
        let [_, _, w, h] = viewport(self.screen_players(), 0);
        if let Some(pointer) = &mut self.player.pad.pointer {
            // Each controller points at its own view using the whole sensor bar or touch screen.
            pointer[0] = pointer[0] * w / h;
        }
    }

    fn draw_player(&mut self, index: usize, now: f32, dt: f32) {
        self.player.others.update(self.player.players.clone());
        self.player.others.animate(dt);
        self.player.swing = (self.player.swing - dt).max(0.0);
        let count = self.screen_players();
        let offset = self.locals.len() + 1 - count;
        if let Some(index) = index.checked_sub(offset) {
            self.draw_view(viewport(count, index), self.view_size(), now);
        }
    }

    fn draw_view(&mut self, rect: [f32; 4], size: [f32; 2], now: f32) {
        self.gpu.viewport(rect);
        self.renderer = Renderer::new(size, self.stream.distance());
        self.render(now, size);
        if !self.global_menu() {
            self.draw_menu_overlay(size, false);
        }
    }

    fn set_local_players(&mut self, count: usize) {
        if !self.active || self.player.id != HOST || !(1..=P::MAX_LOCAL_PLAYERS).contains(&count) {
            return;
        }
        let Some(server) = &self.server else {
            return;
        };
        let additional = count.saturating_sub(self.locals.len() + 1);
        if server.players() + additional > P::MAX_GUESTS + 1 {
            self.tell("World is full - disconnect online players first");
            return;
        }
        while self.locals.len() + 1 > count {
            let player = self.locals.pop().expect("Local player");
            self.serve(|server, world| server.leave(world, player.id));
        }
        while self.locals.len() + 1 < count {
            let slot = self.locals.len() + 2;
            // Stable names preserve each local slot's position and hotbar across sessions.
            let name = format!("Local Player {slot}");
            let mut joined = None;
            self.serve(|server, world| {
                joined = Some(server.join(world, Some((slot - 1) as u32), &name))
            });
            match joined {
                Some(Ok(welcome)) => {
                    let mut player = LocalPlayer::new();
                    welcome.player.restore(&mut player.camera);
                    player.camera.auto_jump = self.settings.game.auto_jump;
                    player.hotbar = welcome.player.hotbar;
                    player.prediction = Prediction::new(&player.camera);
                    player.id = welcome.id;
                    self.locals.push(player);
                }
                Some(Err(error)) => {
                    self.tell(error);
                    break;
                }
                None => break,
            }
        }
        self.controller.local_players(self.locals.len() + 1);
        if self.hosting {
            self.network.local_players(self.locals.len() + 1);
        }
        self.show(Some(Page::Pause));
        self.save();
    }

    fn seconds(&self) -> f32 {
        (P::seconds() - self.started) as f32
    }
    fn show(&mut self, page: Option<Page>) {
        self.show_text(page, String::new());
    }
    // Shows a page, a text page with the text in its field, the shown list keeps its selection.
    fn show_text(&mut self, page: Option<Page>, text: String) {
        self.player.inventory = false;
        self.player.focus = 0;
        self.player.menu_pointer = MenuPointer::default();
        self.player.breaking.next = None;
        self.player.placing.next = None;
        let Some(page) = page else {
            self.player.menu = None;
            return;
        };
        let mut menu = self.player.menu.take().unwrap_or_default();
        menu.session = self.session();
        menu.splitscreen = self.active;
        menu.local_players = self.locals.len() + 1;
        menu.max_local_players = P::MAX_LOCAL_PLAYERS;
        menu.remotes = P::REMOTE_MANAGEMENT;
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
            Page::Remotes => menu.show_list(page, self.controller.status(), None),
            Page::SplitScreen => menu.show(
                page,
                if P::NAME == "Wii U" {
                    "Player 1: GamePad - Players 2-5: Wii Remotes".into()
                } else {
                    "One Wii Remote with Nunchuk per player".into()
                },
            ),
            _ => menu.show(page, text),
        }
        self.player.menu = Some(menu);
    }
    fn world_rows(&self) -> Vec<Row> {
        let now = files::unix_time();
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
                let [a, b, c, d] = address.ip;
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
        self.player.menu.as_ref().map(|menu| menu.page)
    }
    fn selected_world(&self) -> Option<String> {
        let selected = self.player.menu.as_ref()?.selected?;
        Some(self.worlds.get(selected)?.name.clone())
    }
    fn tell(&mut self, text: impl Into<String>) {
        if self.messages.len() == CHAT_RECENT {
            self.messages.pop_front();
        }
        let now = self.seconds();
        self.messages.push_back((text.into(), now));
    }

    const fn playing(&self) -> bool {
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
        self.locals.clear();
        self.player_index = 0;
        self.controller.local_players(1);
        self.close_network();
        self.active = false;
        self.stream.reset(world::WORLD_SEED);
        self.particles.particles.clear();
        self.player.target = None;
        self.player.camera = background_camera();
        self.night = false;
    }
    fn close_network(&mut self) {
        self.network.stop();
        self.hosting = false;
        self.joining = None;
        self.jobs.clear();
        self.player.players.clear();
    }
    // Saves the world of this console in the background, the server of a LAN world saves
    // itself.
    fn save(&mut self) {
        if self.active {
            self.serve(|server, world| server.save_all(world));
            self.stream.save(self.saved.clone());
            self.last_save = self.seconds();
        }
    }

    // Saves the world of this console and waits until it's on the card.
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
        let name = self.storage.create(name, P::random())?;
        self.open(&name);
        Ok(())
    }

    // Plays an opened world of this console in its integrated server and joins it as the host.
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
        let mut server = Server::new(&mut world, P::LIMITS, &fluids);
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

    // Starts playing as the welcomed player, of this console's world or of a LAN world.
    fn enter(&mut self, welcome: &Welcome) {
        self.player.camera = Camera::default();
        welcome.player.restore(&mut self.player.camera);
        self.player.camera.auto_jump = self.settings.game.auto_jump;
        self.player.hotbar = welcome.player.hotbar;
        self.night = welcome.player.night;
        self.player.prediction = Prediction::new(&self.player.camera);
        self.player.id = welcome.id;
        self.remote_revision = welcome.revision;
        self.particles.particles.clear();
        self.player.target = None;
        self.player.last_jump = None;
        self.player.footsteps = Footsteps::default();
        self.player.players.clear();
        self.show(None);
    }

    // Runs the integrated server with the world and delivers what it sends.
    fn serve(&mut self, run: impl FnOnce(&mut Server, &mut Local<'_, P>)) {
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
            Outgoing::To(id, response) => {
                if self.player.id == id {
                    self.response(response);
                } else if let Some(index) = self.locals.iter().position(|player| player.id == id) {
                    mem::swap(&mut self.player, &mut self.locals[index]);
                    self.response(response);
                    mem::swap(&mut self.player, &mut self.locals[index]);
                } else {
                    self.network.respond(id, ServerFrame::Response(response));
                }
            }
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
        let id = self.player.id;
        if self.stream.remote {
            self.network.request(request);
        } else {
            self.serve(|server, world| server.request(world, id, request));
        }
    }

    const fn context(&self) -> Context {
        if self.player.inventory {
            Context::Inventory
        } else if self.player.menu.is_some() {
            Context::Menu
        } else {
            Context::Play
        }
    }

    /// Plays until the player quits or the console stops the game, then saves the world.
    /// Returns true when the player chose Quit, false when the console requested an exit.
    pub fn run(&mut self) -> bool {
        while !self.exit && P::running() {
            let now = self.seconds();
            let dt = (now - self.last_frame).min(0.1);
            self.last_frame = now;
            self.controller.poll();
            self.player.pad = self
                .controller
                .desktop(self.context())
                .merge(self.controller.read_player(0, self.context()));
            self.map_pointer(0);
            self.network_events(now);
            self.update_controls(dt, now);
            self.stream.set_views(
                self.locals
                    .iter()
                    .map(|player| player.camera.chunk())
                    .collect(),
            );
            self.stream.update(self.player.camera.chunk());
            if !self.playing() {
                self.player.camera.yaw =
                    (self.player.camera.yaw + dt * BACKGROUND_TURN_SPEED) % TAU;
            } else {
                self.tick_player(dt);
                self.particles.update(dt, |p| self.stream.block(p));
                self.state_elapsed += dt;
                while self.state_elapsed >= TICK {
                    self.state_elapsed -= TICK;
                    self.serve(|server, world| server.tick(world));
                }
                self.network_tick();
                if now - self.last_save >= AUTOSAVE_INTERVAL {
                    self.save();
                }
                if let Some(error) = self.stream.error() {
                    self.tell(error);
                }
            }
            self.draw_frame(now, dt);
            self.frames += 1;
            if now - self.last_fps >= 1.0 {
                self.fps = (self.frames as f32 / (now - self.last_fps) + 0.5) as u32;
                self.frames = 0;
                self.last_fps = now;
            }
        }
        self.leave_world();
        self.exit
    }

    fn update_controls(&mut self, dt: f32, now: f32) {
        let shared_menu = self.global_menu();
        let pause = !shared_menu
            && self.playing()
            && (self.locals.len() + 1..P::MAX_LOCAL_PLAYERS).any(|index| {
                self.controller
                    .read_player(index, Context::Play)
                    .pressed(Button::Pause)
            });
        if pause {
            self.show(Some(Page::Pause));
            self.save();
        } else if shared_menu {
            self.update_shared_menu(dt, now);
        } else {
            self.update_player(dt, now);
        }
        self.update_local_players(dt, now, shared_menu || pause);
    }

    fn update_shared_menu(&mut self, dt: f32, now: f32) {
        let size = self.control_size();
        let mut selected: Option<(usize, Pad, bool)> = None;
        // Button presses take priority over pointer movement from another controller.
        for index in 0..=P::MAX_LOCAL_PLAYERS {
            let mut pad = if index == P::MAX_LOCAL_PLAYERS {
                self.controller.desktop(Context::Menu)
            } else {
                self.controller.read_player(index, Context::Menu)
            };
            let source = if index == 0 || index == P::MAX_LOCAL_PLAYERS {
                self.gpu.primary_screen_size().unwrap_or(self.gpu.size())
            } else {
                self.gpu.size()
            };
            if let Some(pointer) = &mut pad.pointer {
                pointer[0] *= size[0] / source[0];
                pointer[1] *= size[1] / source[1];
            }
            let moved = self.menu_pointers[index].moved(pad.pointer).is_some();
            if (pad.has_action() || moved)
                && selected
                    .is_none_or(|(_, previous, _)| pad.has_action() && !previous.has_action())
            {
                selected = Some((index, pad, moved));
            }
        }
        let primary_pad = self.player.pad;
        let primary_pointer = self.player.menu_pointer;
        self.player.pad = Pad::default();
        if let Some((index, mut pad, moved)) = selected {
            let pointer = self.menu_pointers[index];
            self.player.menu_pointer = pointer;
            if pointer.pointing && (moved || pad.pressed(Button::Select)) {
                self.player.menu_pointer.position = None;
            } else {
                pad.pointer = None;
            }
            self.player.pad = pad;
            let page = self.page();
            self.update_player(dt, now);
            self.menu_pointers[index].pointing = self.player.menu_pointer.pointing;
            if self.page() != page {
                self.menu_pointers[index].position = None;
            }
        } else {
            self.update_player(dt, now);
        }
        self.player.pad = primary_pad;
        self.player.menu_pointer = primary_pointer;
    }

    fn update_local_players(&mut self, dt: f32, now: f32, shared_menu: bool) {
        for index in 0..self.locals.len() {
            let pad = self.controller.read_player(index + 1, Context::Play);
            if !shared_menu
                && !self.global_menu()
                && !self.locals[index]
                    .menu
                    .as_ref()
                    .is_some_and(|menu| menu.page.typing())
                && pad.pressed(Button::Pause)
            {
                self.show(Some(Page::Pause));
                self.save();
            }
            let paused = shared_menu || self.global_menu();
            mem::swap(&mut self.player, &mut self.locals[index]);
            self.player_index = index + 1;
            if paused {
                self.player.pad = Pad::default();
            } else {
                self.player.pad = self.controller.read_player(index + 1, self.context());
                self.map_pointer(index + 1);
                self.update_player(dt, now);
            }
            self.tick_player(dt);
            mem::swap(&mut self.player, &mut self.locals[index]);
            self.player_index = 0;
        }
    }

    fn draw_frame(&mut self, now: f32, dt: f32) {
        let daylight = if self.night { NIGHT_DAYLIGHT } else { 1.0 };
        self.gpu.begin([0; 3], daylight, now);
        self.draw_player(0, now, dt);
        for index in 0..self.locals.len() {
            mem::swap(&mut self.player, &mut self.locals[index]);
            self.player_index = index + 1;
            self.draw_player(index + 1, now, dt);
            mem::swap(&mut self.player, &mut self.locals[index]);
            self.player_index = 0;
        }
        self.gpu.viewport([0.0, 0.0, 1.0, 1.0]);
        self.draw_overlay(now, self.gpu.size());
        if let Some(size) = self.gpu.primary_screen_size() {
            self.gpu.begin_primary_screen();
            self.draw_view([0.0, 0.0, 1.0, 1.0], size, now);
            self.draw_overlay(now, size);
        }
        self.gpu.present();
    }

    fn update_player(&mut self, dt: f32, now: f32) {
        if self.playing()
            && !self.page().is_some_and(Page::typing)
            && self.player.pad.pressed(Button::Pause)
        {
            if self.page() == Some(Page::Settings) {
                self.save_settings();
            }
            if self.global_menu() {
                self.show(None);
            } else {
                self.show(Some(Page::Pause));
                self.save();
            }
            return;
        }
        if self.player.inventory {
            self.inventory();
        } else if self.player.menu.is_some() {
            self.menu();
        } else if self.playing() {
            self.play(dt, now);
        } else {
            self.show(Some(Page::Title));
        }
    }

    // The engine's menus with the pointer and the D-pad, select presses and back goes back.
    fn menu(&mut self) {
        let size = self.control_size();
        let (pad, session) = (self.player.pad, self.session());
        let pointer = self.player.moved_menu_pointer();
        let status = (self.page() == Some(Page::Remotes)).then(|| self.controller.status());
        let Some(menu) = &mut self.player.menu else {
            return;
        };
        if let Some(rows) = status {
            menu.set_rows(rows, None);
        }
        menu.session = session;
        let page = menu.page;
        if pad.scroll != 0 {
            menu.scroll_steps(i64::from(pad.scroll));
        }
        if self.player_index == 0 && page.typing() {
            let max = match page {
                Page::Chat => MAX_CHAT_LENGTH,
                Page::Username => MAX_NAME_LENGTH,
                Page::DirectConnect => MAX_ADDRESS_LENGTH,
                _ => database::MAX_NAME_LENGTH,
            };
            for ch in self.controller.text().chars().filter(|ch| !ch.is_control()) {
                if menu.text.chars().count() < max {
                    menu.text.push(ch);
                }
            }
        }
        if let Some([x, y]) = pointer {
            menu.point(&Layout::new(menu, P::MENU, size, [0.0; 4]), [x, y]);
        }
        for (button, direction) in [
            (Button::Up, [0.0, -1.0]),
            (Button::Down, [0.0, 1.0]),
            (Button::Left, [-1.0, 0.0]),
            (Button::Right, [1.0, 0.0]),
        ] {
            if pad.pressed(button) {
                self.player.menu_pointer.pointing = false;
                menu.step(&Layout::new(menu, P::MENU, size, [0.0; 4]), direction);
            }
        }
        let layout = Layout::new(menu, P::MENU, size, [0.0; 4]);
        let focused = menu.focused(&layout);
        if focused == Some(Action::Volume) {
            let volume = &mut self.settings.game.volume;
            let step = i16::from(pad.pressed(Button::Right)) - i16::from(pad.pressed(Button::Left));
            *volume = (i16::from(*volume) + step * 10).clamp(0, menu::MAX_VOLUME.into()) as u8;
            if pad.held(Button::Select)
                && let Some([x, y, _]) = pad.pointer
            {
                *volume = (layout.slider_fraction([x, y]) * 10.0 + 0.5) as u8 * 10;
            }
            self.sounds.set_volume(*volume);
        }
        if pad.pressed(Button::Cancel) {
            self.back(page);
            return;
        }
        // The keyboard erases with back and finishes with confirm
        if page.typing() {
            if pad.pressed(Button::Back) && !menu.text.is_empty() {
                menu.type_key(Action::Erase, 0);
                return;
            }
            if pad.pressed(Button::Confirm) {
                self.confirm(page);
                return;
            }
        }
        if pad.pressed(Button::Back) {
            self.back(page);
        } else if pad.pressed(Button::Select)
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
            Action::SplitScreen => self.show(Some(Page::SplitScreen)),
            Action::LocalPlayers(count) => self.set_local_players(count),
            Action::Remotes => self.show(Some(Page::Remotes)),
            Action::ReconnectRemotes => self.controller.reconnect(),
            Action::Resume => self.show(None),
            Action::Settings => self.show(Some(Page::Settings)),
            Action::Quit => self.exit = true,
            Action::OpenToLan => self.network.host(
                &self.world_name,
                &self.settings.username,
                self.locals.len() + 1,
            ),
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
                if let Some(menu) = &mut self.player.menu {
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
                let selected = self.player.menu.as_ref().and_then(|menu| menu.selected);
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
                self.player.camera.auto_jump = self.settings.game.auto_jump;
            }
            Action::Username => {
                self.show_text(Some(Page::Username), self.settings.username.clone());
            }
            Action::Reset => {
                self.settings.game = Settings::new::<P>().game;
                self.player.camera.auto_jump = self.settings.game.auto_jump;
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
                if let Some(menu) = &mut self.player.menu {
                    menu.type_key(action, max);
                }
            }
            Action::Back => self.back(page),
        }
    }

    fn back(&mut self, page: Page) {
        match page {
            Page::Title => {}
            Page::SplitScreen | Page::Remotes => self.show(Some(if self.playing() {
                Page::Pause
            } else {
                Page::Title
            })),
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
        let Some(menu) = &mut self.player.menu else {
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
                    self.save_settings();
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
        if let Some(menu) = &mut self.player.menu
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

    fn join(&mut self, address: Address) {
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
                    // The local player count may have changed while networking started.
                    self.network.local_players(self.locals.len() + 1);
                    self.tell(format!(
                        "World opened to LAN (up to {} guests)",
                        P::MAX_GUESTS.saturating_sub(self.locals.len())
                    ));
                }
                network::Event::Found(address, announcement) => {
                    if let Some(found) = self
                        .found
                        .iter_mut()
                        .find(|(found, _, _)| *found == address)
                    {
                        *found = (address, announcement, now);
                    } else if self.found.len() < 16 {
                        self.found.push((address, announcement, now));
                    }
                }
                // Guests play with the id of their connection, their datagrams use it
                network::Event::Joined(id, name)
                    if self.hosting && id >= P::MAX_LOCAL_PLAYERS as u32 =>
                {
                    let mut joined = Err(String::new());
                    self.serve(|server, world| {
                        joined = if server.players() > P::MAX_GUESTS {
                            Err("World is full".into())
                        } else {
                            server.join(world, Some(id), &name)
                        }
                    });
                    let frame = match joined {
                        Ok(welcome) => ServerFrame::Accepted {
                            welcome,
                            token: P::random() ^ u64::from(id),
                            udp_port: GAME_PORT,
                        },
                        Err(error) => ServerFrame::Rejected(error),
                    };
                    self.network.respond(id, frame);
                }
                network::Event::Request(id, request)
                    if self.hosting && id >= P::MAX_LOCAL_PLAYERS as u32 =>
                {
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
                network::Event::Left(id) if id >= P::MAX_LOCAL_PLAYERS as u32 => {
                    self.serve(|server, world| server.leave(world, id))
                }
                network::Event::Server(frame) => self.server_frame(*frame),
                _ => {}
            }
        }
        self.found.retain(|(_, _, seen)| now - *seen < 5.0);
        let rows = self.server_rows();
        if let Some(menu) = &mut self.player.menu
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

    // Handles a response of the server, of this console's world or of a LAN world.
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
                    if by != self.player.id
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
                let flying = self.player.camera.flying;
                let stream = &mut self.stream;
                self.player
                    .prediction
                    .reconcile(sequence, body, &mut self.player.camera, |p| stream.block(p));
                if self.player.camera.flying != flying {
                    self.sounds.fly(self.player.camera.flying);
                }
                self.player.players = others;
            }
            Response::Time { night } => self.night = night,
            Response::Chat { name, text } => self.tell(format!("{name}: {text}")),
            Response::Message(message) => self.tell(message),
            Response::Disconnected(message) if self.stream.remote => {
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
            input: if self.player.menu.is_none() && !self.player.inventory {
                Input {
                    direction: self.player.pad.direction,
                    stick: self.player.pad.walk,
                    jump: self.player.pad.held(Button::Jump),
                    sneak: self.player.pad.held(Button::Sneak),
                    fast: self.player.pad.held(Button::Fast),
                }
            } else {
                Input::default()
            },
            yaw: self.player.camera.yaw,
            pitch: self.player.camera.pitch,
            auto_jump: self.settings.game.auto_jump,
        };
        let (stream, sounds) = (&mut self.stream, &mut self.sounds);
        let requests = self.player.prediction.tick(
            dt,
            &mut self.player.camera,
            controls,
            |p| stream.block(p),
            |before, after, block| {
                let ground = block(ground(after));
                sounds.movement(
                    &mut self.player.footsteps,
                    Movement::of(before),
                    after,
                    ground,
                );
            },
        );
        for request in requests {
            self.send(request);
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
        if hotbar == self.player.hotbar {
            return;
        }
        if hotbar.material() != self.player.hotbar.material() {
            self.player.name_shown = Some((hotbar.material(), self.seconds()));
        }
        self.sounds.click();
        self.player.hotbar = hotbar;
        self.send(Request::Hotbar(hotbar));
    }

    fn inventory(&mut self) {
        let (pad, count) = (self.player.pad, world::MATERIALS.len());
        let page = page_start(self.player.focus);
        if let Some([x, y]) = self.player.moved_menu_pointer() {
            let left = (self.control_size()[0] - CELL * COLUMNS as f32) / 2.0;
            let bottom = INVENTORY_TOP + CELL * ROWS as f32;
            if x >= left && x < left + CELL * COLUMNS as f32 && (INVENTORY_TOP..bottom).contains(&y)
            {
                let index = ((y - INVENTORY_TOP) / CELL) as usize * COLUMNS
                    + ((x - left) / CELL) as usize
                    + page;
                if index < count {
                    self.player.focus = index;
                }
            }
        }
        for (button, step) in [
            (Button::Left, count - 1),
            (Button::Right, 1),
            (Button::Up, count - COLUMNS),
            (Button::Down, COLUMNS),
        ] {
            if pad.pressed(button) {
                self.player.menu_pointer.pointing = false;
                self.player.focus = (self.player.focus + step) % count;
            }
        }
        let mut hotbar = self.player.hotbar;
        if let Some(slot) = pad.hotbar {
            let left = (self.control_size()[0] - CELL * COLUMNS as f32) / 2.0;
            let hovered = pad.pointer.is_some_and(|[x, y, _]| {
                x >= left
                    && x < left + CELL * COLUMNS as f32
                    && (INVENTORY_TOP..INVENTORY_TOP + CELL * ROWS as f32).contains(&y)
                    && (page
                        + ((y - INVENTORY_TOP) / CELL) as usize * COLUMNS
                        + ((x - left) / CELL) as usize)
                        < count
            });
            if hovered {
                hotbar.assign(slot, world::MATERIALS[self.player.focus].0);
            } else {
                hotbar.selected = slot;
            }
        }
        if pad.scroll != 0 {
            self.player.focus = (self.player.focus as i64 - i64::from(pad.scroll) * COLUMNS as i64)
                .clamp(0, count as i64 - 1) as usize;
        }
        if pad.pressed(Button::Previous) {
            hotbar.cycle(1);
        }
        if pad.pressed(Button::Next) {
            hotbar.cycle(-1);
        }
        // Pressing on a hotbar slot selects it, elsewhere it fills the selected slot
        if pad.pressed(Button::Select) {
            match self.hotbar_slot(
                pad.pointer.filter(|_| self.player.menu_pointer.pointing),
                true,
            ) {
                Some(slot) => hotbar.selected = slot,
                None => hotbar.assign(hotbar.selected, world::MATERIALS[self.player.focus].0),
            }
        }
        self.set_hotbar(hotbar);
        if pad.pressed(Button::Back) {
            self.show(None);
        }
    }

    // Looks around, selects blocks and edits, the physics runs in tick_player.
    fn play(&mut self, dt: f32, now: f32) {
        let pad = self.player.pad;
        if pad.pressed(Button::Chat) || pad.pressed(Button::Command) {
            let command = pad.pressed(Button::Command);
            let mut text = if command {
                String::from("/")
            } else {
                String::new()
            };
            // USB callbacks can deliver the shortcut and more text before the next frame.
            let shortcut = if command { &['/'][..] } else { &['t', 'T'][..] };
            if let Some((_, typed)) = self.controller.text().split_once(shortcut) {
                text.extend(typed.chars().take(MAX_CHAT_LENGTH - text.chars().count()));
            }
            self.show_text(Some(Page::Chat), text);
            return;
        }
        if pad.pressed(Button::Pick) {
            if let Some(hit) = self.player.target
                && let Some(material) = world::item_material(hit.material)
            {
                let mut hotbar = self.player.hotbar;
                hotbar.pick(material);
                self.set_hotbar(hotbar);
            }
            return;
        }
        if pad.pressed(Button::Inventory) {
            self.show(None);
            self.player.inventory = true;
            return;
        }
        if pad.pressed(Button::Tools) {
            self.show(Some(Page::Tools));
            return;
        }
        let turn = |value: f32| value * value.abs() * LOOK_SPEED * dt / 0.002;
        let [yaw, pitch] = pad.look;
        self.player.camera.look(
            f64::from(turn(yaw) + pad.mouse_motion[0]),
            f64::from(-turn(pitch) + pad.mouse_motion[1]),
        );
        if pad.pressed(Button::Jump) {
            if self
                .player
                .last_jump
                .is_some_and(|last| now - last < DOUBLE_TAP)
            {
                let flying = !self.player.camera.flying;
                self.send(Request::Chat(
                    if flying { "/fly on" } else { "/fly off" }.into(),
                ));
                // Predicted, the server's state confirms it
                self.player.camera.set_flying(flying);
                self.sounds.fly(flying);
                self.player.last_jump = None;
            } else {
                self.player.last_jump = Some(now);
            }
        }
        let mut hotbar = self.player.hotbar;
        if let Some(slot) = pad.hotbar {
            hotbar.selected = slot;
        }
        hotbar.cycle(i64::from(pad.scroll));
        if pad.pressed(Button::Previous) {
            hotbar.cycle(1);
        }
        if pad.pressed(Button::Next) {
            hotbar.cycle(-1);
        }
        if pad.pressed(Button::Tap)
            && let Some(slot) = self.hotbar_slot(pad.pointer, self.player_index == 0)
        {
            hotbar.selected = slot;
        }
        self.set_hotbar(hotbar);
        self.aim();
        if self.player.breaking.update(&pad, Button::Break, now) {
            self.edit(false);
        }
        if self.player.placing.update(&pad, Button::Place, now) {
            self.edit(true);
        }
    }
    // The hotbar slot under the pointer.
    fn hotbar_slot(&self, pointer: Option<[f32; 3]>, primary: bool) -> Option<usize> {
        let [x, y, _] = pointer?;
        let size = if self.player.inventory {
            self.control_size()
        } else if primary {
            self.gpu
                .primary_screen_size()
                .unwrap_or_else(|| self.view_size())
        } else {
            self.view_size()
        };
        hud::hotbar_slot(size, [x, y])
    }

    fn aim(&mut self) {
        let direction = self.player.camera.direction();
        let stream = &mut self.stream;
        self.player.target = raycast(self.player.camera.position, direction, |position| {
            stream.block(position)
        });
    }
    fn edit(&mut self, place: bool) {
        self.player.swing = SWING_DURATION;
        let Some(hit) = self.player.target else {
            return;
        };
        let edit = if place {
            let stream = &mut self.stream;
            hit.place_edit(
                self.player.hotbar.material(),
                self.player.camera.position,
                |p| stream.block(p),
            )
        } else {
            hit.break_edit()
        };
        let Some(edit) = edit else {
            return;
        };
        let material = if place { edit.material } else { hit.material };
        self.send(Request::Edit(edit));
        self.sounds
            .block(&self.player.camera, edit.position, material, !place);
        if !place {
            self.particles.burst(hit.position, material);
        }
        self.aim();
    }

    // The block grid of the inventory, the consoles' version of the desktop inventory.
    fn draw_inventory(hud: &mut Hud<'_, P::Gpu>, focus: usize, hotbar: &Hotbar, help: &str) {
        hud.rect(0.0, 0.0, hud.width, hud.height, 0x0000_00aa);
        hud.label(
            "Inventory",
            (hud.width - text_width("Inventory", 30.0)) / 2.0,
            36.0,
            30.0,
        );
        let first = page_start(focus);
        let left = (hud.width - CELL * COLUMNS as f32) / 2.0;
        for i in first..(first + INVENTORY_PAGE).min(world::MATERIALS.len()) {
            let cell = i - first;
            let x = left + (cell % COLUMNS) as f32 * CELL;
            let y = INVENTORY_TOP + (cell / COLUMNS) as f32 * CELL;
            let color = if i == focus { 0x7e88_bfff } else { 0x3030_30ff };
            hud.rect(x, y, CELL - 2.0, CELL - 2.0, color);
            hud.block(world::MATERIALS[i].0, x + 6.0, y + 6.0, 30.0);
        }
        hud.label(name(world::MATERIALS[focus].0), 24.0, 358.0, 20.0);
        hud.label(help, hud.width - 24.0 - text_width(help, 16.0), 360.0, 16.0);
        hud.hotbar(hotbar);
    }

    fn render(&mut self, now: f32, [width, height]: [f32; 2]) {
        let gpu = &mut self.gpu;
        let daylight = if self.night { NIGHT_DAYLIGHT } else { 1.0 };
        // Between the last two physics ticks, so the player moves smoothly
        let camera = if self.active || self.stream.remote {
            self.player.prediction.view(&self.player.camera)
        } else {
            self.player.camera
        };
        let view = camera.view(width as u32, height as u32, self.stream.distance());
        let center = camera.chunk();
        let in_game = self.player.menu.is_none() && !self.player.inventory;
        // The title pages show the turning default world behind them
        let eye = camera.position.map(|v| v.floor() as i64);
        let eye_material = self.stream.block(eye);
        let clear = self.renderer.begin(daylight, eye_material);
        let mut background = Hud::begin(gpu, [width, height]);
        background.rect(
            0.0,
            0.0,
            width,
            height,
            u32::from_be_bytes([clear[0], clear[1], clear[2], 255]),
        );
        self.renderer.draw_sky(gpu, &view, eye_material, now);
        self.renderer
            .draw_world(gpu, &view, center, &self.stream.chunks);
        if self.active || self.stream.remote {
            if in_game && let Some(hit) = self.player.target {
                self.renderer
                    .draw_selection(gpu, &view, center, hit.position, hit.material);
            }
            self.renderer
                .draw_players(gpu, &view, center, &self.player.others);
            self.renderer
                .draw_particles(gpu, &view, center, &self.particles);
            if in_game {
                self.renderer.draw_held(
                    gpu,
                    self.player.hotbar.material(),
                    1.0 - self.player.swing / SWING_DURATION,
                );
            }
        }
        let mut hud = Hud::begin(gpu, [width, height]);
        if in_game {
            for (name, [x, y]) in self.player.others.name_tags(&view, center) {
                hud.label(name, x - text_width(name, 16.0) / 2.0, y, 16.0);
            }
        }
        if in_game {
            hud.crosshair();
            let top = hud.hotbar(&self.player.hotbar);
            if let Some((material, shown)) = self.player.name_shown
                && now - shown < NAME_DURATION
            {
                let text = name(material);
                hud.label(
                    text,
                    (hud.width - text_width(text, 18.0)) / 2.0,
                    top - 30.0,
                    18.0,
                );
            }
            if self.player.camera.flying {
                hud.label("Flying", 12.0, 32.0, 16.0);
            }
        }
    }
    // Shared messages and global menus use the full screen.
    fn draw_overlay(&mut self, now: f32, size: [f32; 2]) {
        let mut hud = Hud::begin(&mut self.gpu, size);
        if self.settings.game.show_stats && (self.active || self.stream.remote) {
            let [x, y, z] = self.player.camera.position;
            hud.label(
                &format!(
                    "{} FPS - {x:.0}, {y:.0}, {z:.0} - {} chunks - distance {}",
                    self.fps,
                    self.stream.chunks.len(),
                    self.stream.distance()
                ),
                12.0,
                10.0,
                16.0,
            );
        }
        let recent = self
            .messages
            .iter()
            .filter(|(_, shown)| now - shown < CHAT_TIME);
        for (row, (message, _)) in recent.rev().enumerate() {
            let y = hud.height - 74.0 - row as f32 * 24.0;
            hud.rect(8.0, y, hud.width - 16.0, 24.0, 0x0000_00c0);
            hud.label(message, 14.0, y + 4.0, 16.0);
        }
        if self.global_menu() {
            self.draw_menu_overlay(size, true);
        }
    }

    fn draw_menu_overlay(&mut self, size: [f32; 2], global: bool) {
        let context = self.context();
        if context == Context::Play {
            return;
        }
        let pointer_size = self.gpu.size();
        let primary_pointer_size = self.gpu.primary_screen_size().unwrap_or(pointer_size);
        let mut hud = Hud::begin(&mut self.gpu, size);
        if self.player.inventory {
            let help = if self.player_index == 0 {
                P::INVENTORY_HELP
            } else {
                "A assign - Z/C slot - B/2 close"
            };
            Self::draw_inventory(&mut hud, self.player.focus, &self.player.hotbar, help);
        } else if let Some(menu) = &self.player.menu {
            let layout = Layout::new(menu, P::MENU, size, [0.0; 4]);
            menu.draw(
                &mut hud,
                &layout,
                size,
                menu.focused(&layout),
                self.settings.game,
            );
        }
        // Global menus show every connected Remote; local overlays show their own cursor.
        let players = if global {
            0..P::MAX_LOCAL_PLAYERS
        } else {
            self.player_index..self.player_index + 1
        };
        for player in players.rev() {
            if let Some(cursor) = self.controller.cursor_index(player)
                && let Some(pointer) = self.controller.read_player(player, context).pointer
            {
                let [x, y, angle] = pointer;
                let pointer_size = if player == 0 {
                    primary_pointer_size
                } else {
                    pointer_size
                };
                hud.cursor(
                    [
                        x * size[0] / pointer_size[0],
                        y * size[1] / pointer_size[1],
                        angle,
                    ],
                    cursor,
                );
            }
        }
        if (global || self.player_index == 0)
            && let Some([x, y]) = self.controller.mouse_pointer()
        {
            hud.mouse([
                x * size[0] / primary_pointer_size[0],
                y * size[1] / primary_pointer_size[1],
            ]);
        }
    }
}
/// The viewport of a local player, in fractions of the full screen.
/// Two players share horizontal halves; three and four use a two-by-two grid, five a three-by-two grid.
pub fn viewport(count: usize, index: usize) -> [f32; 4] {
    match count {
        1 => [0.0, 0.0, 1.0, 1.0],
        2 => [0.0, index as f32 * 0.5, 1.0, 0.5],
        3 | 4 => [(index % 2) as f32 * 0.5, (index / 2) as f32 * 0.5, 0.5, 0.5],
        _ => [
            (index % 3) as f32 / 3.0,
            (index / 3) as f32 * 0.5,
            1.0 / 3.0,
            0.5,
        ],
    }
}

// Looks over the default world from above its spawn, like the desktop title screen.
fn background_camera() -> Camera {
    Camera {
        pitch: -0.2,
        ..Camera::default()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use alloc::boxed::Box;

    use super::super::platform::{
        Audio, Fog, Io, Lock, Mtx, Mtx44, Pass, Signal, Socket, State, Texture, Thread, Vertex,
    };
    use super::*;
    use crate::protocol::Player;
    use crate::sound::Effect;

    // No worker or network threads: tests drive the integrated server themselves.
    const LOCAL_PLAYERS: usize = 4;
    pub(crate) type TestPlatform = TestConsole<4>;
    pub(crate) struct TestConsole<const PLAYERS: usize>;
    #[derive(Default)]
    pub(crate) struct Device {
        viewports: Vec<[f32; 4]>,
        projections: Vec<[f32; 2]>,
        pads: [super::super::input::Wiimote; 5],
        draws: Vec<Draw>,
        primary_size: Option<[f32; 2]>,
        primary_screen: bool,
        desktop: Option<super::super::desktop::Desktop>,
    }
    struct Draw {
        primary_screen: bool,
        viewport: [f32; 4],
        size: [f32; 2],
        texture: Texture,
        vertices: Vec<Vertex>,
    }

    pub(crate) struct TestLock(core::sync::atomic::AtomicBool);

    impl Lock for TestLock {
        fn new() -> Self {
            Self(core::sync::atomic::AtomicBool::new(false))
        }
        fn lock(&self) {
            assert!(!self.0.swap(true, core::sync::atomic::Ordering::Acquire));
        }
        fn unlock(&self) {
            self.0.store(false, core::sync::atomic::Ordering::Release);
        }
    }
    impl Signal<TestLock> for Device {
        fn new() -> Self {
            Self::default()
        }
        fn wait(&self, _: &TestLock) {
            panic!("Tests must not wait on a worker");
        }
        fn signal(&self) {}
    }
    impl Socket for Device {
        fn tcp() -> Option<Self> {
            None
        }
        fn udp() -> Option<Self> {
            None
        }
        fn bind(&self, _: u16) -> bool {
            false
        }
        fn listen(&self, _: usize) -> bool {
            false
        }
        fn nonblocking(&self) {}
        fn broadcast(&self) {}
        fn connect(&self, _: Address) -> bool {
            false
        }
        fn accept(&self) -> Option<Self> {
            None
        }
        fn send(&self, _: &[u8]) -> Io {
            Io::Failed
        }
        fn receive(&self, _: &mut [u8]) -> Io {
            Io::Wait
        }
        fn send_to(&self, _: &[u8], _: Address) {}
        fn receive_from(&self, _: &mut [u8]) -> Option<(usize, Address)> {
            None
        }
    }
    impl Gpu for Device {
        type Mesh = ();
        fn mesh(_: &super::super::mesh::Mesh) {}
        fn has_alpha(_: usize) -> bool {
            false
        }
        fn draws(_: &(), _: usize, _: Pass) -> bool {
            false
        }
        fn size(&self) -> [f32; 2] {
            [640.0, 480.0]
        }
        fn primary_screen_size(&self) -> Option<[f32; 2]> {
            self.primary_size
        }
        fn begin_primary_screen(&mut self) {
            self.primary_screen = true;
        }
        fn begin(&mut self, _: [u8; 3], _: f32, _: f32) {
            self.primary_screen = false;
        }
        fn viewport(&mut self, rect: [f32; 4]) {
            self.viewports.push(rect);
        }
        fn projection(&mut self, matrix: &Mtx44, orthographic: bool) {
            if orthographic {
                self.projections
                    .push([2.0 / matrix[0][0], -2.0 / matrix[1][1]]);
            }
        }
        fn model_view(&mut self, _: &Mtx) {}
        fn state(&mut self, _: State) {}
        fn fog(&mut self, _: Option<Fog>) {}
        fn quads(&mut self, texture: Texture, vertices: &[Vertex]) {
            self.draws.push(Draw {
                primary_screen: self.primary_screen,
                viewport: *self.viewports.last().unwrap(),
                size: *self.projections.last().unwrap(),
                texture,
                vertices: vertices.to_vec(),
            });
        }
        fn lines(&mut self, _: &[Vertex]) {}
        fn chunk(&mut self, _: &(), _: usize, _: Pass) {}
        fn present(&mut self) {}
    }
    impl Audio for Device {
        fn play(&mut self, _: Effect, _: [f32; 2], _: f32) {}
    }
    impl Controller for Device {
        fn poll(&mut self) {}
        fn read_player(&self, player: usize, context: Context) -> Pad {
            self.pads[player].pad(context)
        }
        fn desktop(&self, context: Context) -> Pad {
            self.desktop
                .as_ref()
                .map_or_else(Pad::default, |desktop| desktop.pad(context))
        }
        fn text(&self) -> &str {
            self.desktop.as_ref().map_or("", |desktop| desktop.text())
        }
        fn mouse_pointer(&self) -> Option<[f32; 2]> {
            self.desktop.as_ref().and_then(|desktop| desktop.pointer())
        }
    }
    impl<const PLAYERS: usize> Platform for TestConsole<PLAYERS> {
        type Lock = TestLock;
        type Signal = Device;
        type Socket = Device;
        type Gpu = Device;
        type Audio = Device;
        type Controller = Device;
        const NAME: &'static str = "Test";
        const MENU: menu::Platform = menu::Platform {
            keyboard: menu::Keyboard::OnScreen,
            quits: true,
            distance: false,
            confirm: "+",
            open: "Press A",
            controls: &[],
        };
        const LIMITS: crate::server::Limits = crate::server::Limits {
            fluid_radius: 0,
            fluid_updates: 0,
        };
        const RENDER_DISTANCE: i64 = 8;
        const MAX_GUESTS: usize = if PLAYERS > 4 { 7 } else { 3 };
        const MAX_LOCAL_PLAYERS: usize = PLAYERS;
        const INVENTORY_HELP: &'static str = "";
        fn spawn(_: Thread, _: Box<dyn FnOnce() + Send>) {}
        fn seconds() -> f64 {
            0.0
        }
        fn random() -> u64 {
            1
        }
        fn idle() {}
        fn connect_network() -> Result<[u8; 4], i32> {
            Err(-1)
        }
        fn mount_storage() -> bool {
            false
        }
        fn nickname() -> Option<String> {
            None
        }
        fn running() -> bool {
            true
        }
    }

    fn game_for<const PLAYERS: usize>() -> Game<TestConsole<PLAYERS>> {
        let mut game = Game::<TestConsole<PLAYERS>>::new(
            Device::default(),
            Device::default(),
            Device::default(),
        );
        let mut saved = Saved::default();
        // Seed guest saves so this test requires no mounted SD card.
        for slot in 2..=PLAYERS {
            saved
                .guests
                .insert(format!("Local Player {slot}"), Player::default());
        }
        game.start(Opened {
            seed: world::WORLD_SEED,
            saved,
        });
        game
    }

    fn game() -> Game<TestPlatform> {
        game_for::<4>()
    }

    #[test]
    fn console_players_draw_selected_blocks_with_their_material_textures() {
        let mut gpu = Device {
            viewports: alloc::vec![[0.0, 0.0, 1.0, 1.0]],
            projections: alloc::vec![[640.0, 480.0]],
            ..Device::default()
        };
        let renderer = Renderer::new([640.0, 480.0], 8);
        let camera = Camera::default();
        let view = camera.view(640, 480, 8);
        let mut others = Others::default();
        for &(material, _) in world::MATERIALS {
            others.update(alloc::vec![PlayerInfo {
                id: 1,
                name: "Guest".into(),
                pose: crate::protocol::Pose::default(),
                held: material,
            }]);
            gpu.draws.clear();
            renderer.draw_players(&mut gpu, &view, camera.chunk(), &others);
            let tiles: Vec<_> = gpu
                .draws
                .iter()
                .filter_map(|draw| match draw.texture {
                    Texture::Tile(tile) => Some(tile),
                    _ => None,
                })
                .collect();
            let expected: Vec<_> = if world::is_sprite(material) {
                alloc::vec![crate::tiles::tile(material, 6)]
            } else {
                (0..world::boxes(material).len().max(1))
                    .flat_map(|_| (0..6).map(|face| crate::tiles::tile(material, face)))
                    .collect()
            };
            assert_eq!(tiles, expected, "{material}");
            assert!(
                gpu.draws
                    .iter()
                    .flat_map(|draw| &draw.vertices)
                    .all(|vertex| {
                        vertex.position.iter().all(|v| v.is_finite())
                            && vertex.uv.iter().all(|v| (0.0..=1.0).contains(v))
                    })
            );
        }
    }

    #[test]
    fn wii_u_supports_five_players_with_four_existing_cursor_images() {
        let mut game = game_for::<5>();
        game.set_local_players(5);
        assert_eq!(game.locals.len(), 4);
        assert_eq!(game.server.as_ref().unwrap().players(), 5);
        assert_eq!(
            game.locals
                .iter()
                .map(|player| player.id)
                .collect::<Vec<_>>(),
            [1, 2, 3, 4]
        );
        game.set_local_players(6);
        assert_eq!(game.locals.len(), 4);
        for pad in &mut game.controller.pads {
            pad.pointer = Some([320.0, 240.0, 0.0]);
        }
        game.draw_frame(0.0, 0.0);
        assert_eq!(
            game.gpu
                .draws
                .iter()
                .filter(|draw| matches!(draw.texture, Texture::Cursor(_)))
                .count(),
            4
        );
        game.show(None);
        game.controller.pads[4].held = super::super::input::Wiimote::UP;
        game.update_local_players(TICK, 0.0, false);
        assert_eq!(game.locals[3].camera.input.direction, [0.0, 1.0]);
        game.hosting = true;
        game.saved.guests.insert("Online".into(), Player::default());
        game.network
            .inject(network::Event::Joined(5, "Online".into()));
        game.network_events(0.0);
        assert_eq!(game.server.as_ref().unwrap().players(), 6);
        game.network.inject(network::Event::Left(5));
        game.network_events(0.0);
        assert_eq!(game.server.as_ref().unwrap().players(), 5);
        assert_eq!(game.locals[3].id, 4);
    }

    #[test]
    fn local_players_have_independent_requests_predictions_and_saved_inventories() {
        let mut game = game();
        game.set_local_players(4);
        assert_eq!(game.locals.len(), 3);
        assert_eq!(game.server.as_ref().unwrap().players(), 4);
        let host = game.player.hotbar;
        for index in 0..3 {
            mem::swap(&mut game.player, &mut game.locals[index]);
            let mut hotbar = game.player.hotbar;
            hotbar.selected = index + 1;
            game.set_hotbar(hotbar);
            game.command("/fly on");
            mem::swap(&mut game.player, &mut game.locals[index]);
        }
        game.serve(|server, world| server.tick(world));
        assert_eq!(game.player.hotbar, host);
        assert_eq!(game.player.players.len(), 3);
        for (other, local) in game.player.players.iter().zip(&game.locals) {
            assert_eq!(other.id, local.id);
            assert_eq!(other.held, local.hotbar.material());
        }
        assert!(!game.player.camera.flying);
        for player in &game.locals {
            assert!(player.camera.flying);
        }
        game.set_local_players(1);
        assert!(game.locals.is_empty());
        assert_eq!(game.server.as_ref().unwrap().players(), 1);
        for slot in 2..=4 {
            let saved = game.saved.guests[&format!("Local Player {slot}")];
            assert_eq!(saved.hotbar.selected, slot - 1);
            assert!(saved.flying);
        }
        game.set_local_players(4);
        for (index, player) in game.locals.iter().enumerate() {
            assert_eq!(player.hotbar.selected, index + 1);
            assert!(player.camera.flying);
        }
        // Closing the host must not recursively treat local guests as network disconnects.
        game.serve(|server, world| server.stop(world));
        assert!(game.active);
    }

    #[test]
    fn wii_local_players_from_old_overlapping_saves_are_spaced_apart() {
        let mut game = game();
        game.set_local_players(4);
        let mut positions = alloc::vec![game.player.camera.position];
        for player in &game.locals {
            for other in &positions {
                let distance = (player.camera.position[0] - other[0])
                    .hypot(player.camera.position[2] - other[2]);
                assert!(distance >= 3.0);
            }
            positions.push(player.camera.position);
        }
        for slot in 2..=4 {
            assert_eq!(
                game.saved.guests[&format!("Local Player {slot}")].position,
                game.locals[slot - 2].camera.position
            );
        }
    }

    #[test]
    fn split_screen_shares_join_chat_and_leave_messages_without_dropping_repeats() {
        let mut game = game();
        game.set_local_players(4);
        assert_eq!(
            game.messages
                .iter()
                .map(|(text, _)| text.as_str())
                .collect::<Vec<_>>(),
            [
                "Local Player 2 joined the game",
                "Local Player 3 joined the game",
                "Local Player 4 joined the game",
            ]
        );
        game.messages.clear();
        for _ in 0..2 {
            game.send(Request::Chat("Hello".into()));
        }
        assert_eq!(game.messages.len(), 2);
        assert_eq!(game.messages[0].0, game.messages[1].0);
        game.messages.clear();
        game.set_local_players(1);
        assert_eq!(
            game.messages
                .iter()
                .map(|(text, _)| text.as_str())
                .collect::<Vec<_>>(),
            [
                "Local Player 4 left the game",
                "Local Player 3 left the game",
                "Local Player 2 left the game",
            ]
        );
    }

    #[test]
    fn local_player_count_is_only_changed_by_a_host_and_stays_in_range() {
        let mut game = game();
        game.set_local_players(0);
        game.set_local_players(5);
        assert!(game.locals.is_empty());
        game.active = false;
        game.set_local_players(2);
        assert!(game.locals.is_empty());
    }

    #[test]
    fn local_slots_and_online_connections_remain_separate_and_share_capacity() {
        let mut game = game();
        game.set_local_players(3);
        assert_eq!(
            game.locals
                .iter()
                .map(|player| player.id)
                .collect::<Vec<_>>(),
            [1, 2]
        );
        game.hosting = true;
        game.saved.guests.insert("Online".into(), Player::default());
        let online = LOCAL_PLAYERS as u32;
        game.network
            .inject(network::Event::Joined(online, "Online".into()));
        game.network_events(0.0);
        assert_eq!(game.server.as_ref().unwrap().players(), 4);
        game.set_local_players(4);
        assert_eq!(game.locals.len(), 2);
        assert_eq!(game.server.as_ref().unwrap().players(), 4);
        game.network.inject(network::Event::Left(online));
        game.network.inject(network::Event::Left(1));
        game.network_events(0.0);
        assert_eq!(game.server.as_ref().unwrap().players(), 3);
        assert_eq!(
            game.locals
                .iter()
                .map(|player| player.id)
                .collect::<Vec<_>>(),
            [1, 2]
        );
        game.set_local_players(4);
        assert_eq!(game.locals[2].id, 3);
        assert_eq!(game.server.as_ref().unwrap().players(), 4);
        // A full split-screen session must also refuse an incoming online guest.
        game.network
            .inject(network::Event::Joined(online + 1, "Another".into()));
        game.network_events(0.0);
        assert_eq!(game.server.as_ref().unwrap().players(), 4);
    }

    #[test]
    fn inventory_only_stops_its_player_and_global_pause_stops_everyone() {
        use super::super::input::Wiimote;
        let mut game = game();
        game.set_local_players(4);
        game.show(None);
        game.player.pad = Wiimote {
            pressed: Wiimote::TWO,
            ..Wiimote::default()
        }
        .pad(Context::Play);
        game.update_player(0.0, 0.0);
        assert!(game.player.inventory);
        game.controller.pads[1].stick = Some([0.0, 1.0]);
        game.controller.pads[1].held = Wiimote::UP;
        game.update_local_players(TICK, 0.0, false);
        assert_eq!(game.locals[0].camera.input.direction, [0.0, 1.0]);
        game.show(Some(Page::Pause));
        game.update_local_players(TICK, 0.0, false);
        assert_eq!(game.locals[0].camera.input, Input::default());
        game.player.pad.pointer = Some([320.0, 240.0, 0.0]);
        game.map_pointer(0);
        assert_eq!(game.player.pad.pointer, Some([320.0, 240.0, 0.0]));
        game.draw_frame(0.0, 0.0);
        assert_eq!(game.gpu.viewports.last(), Some(&[0.0, 0.0, 1.0, 1.0]));
        assert!(
            game.gpu
                .projections
                .last()
                .unwrap()
                .iter()
                .zip([640.0, 480.0])
                .all(|(actual, expected)| (actual - expected).abs() < 0.001)
        );
        game.set_local_players(2);
        game.show(Some(Page::Pause));
        game.draw_frame(0.0, 0.0);
        assert_eq!(game.gpu.viewports.last(), Some(&[0.0, 0.0, 1.0, 1.0]));
        assert!(
            game.gpu
                .projections
                .last()
                .unwrap()
                .iter()
                .zip([640.0, 480.0])
                .all(|(actual, expected)| (actual - expected).abs() < 0.001)
        );
        assert!(
            game.gpu
                .projections
                .iter()
                .any(|size| (size[0] - 1280.0).abs() < 0.001 && (size[1] - 480.0).abs() < 0.001)
        );
    }

    #[test]
    fn secondary_inventory_tools_and_chat_are_local_and_pause_is_shared() {
        use super::super::input::Wiimote;

        for primary_size in [None, Some([854.0, 480.0])] {
            for player in 1..=4 {
                let mut game = game_for::<5>();
                game.gpu.primary_size = primary_size;
                game.set_local_players(5);
                game.show(None);
                game.controller.pads[player].pressed = Wiimote::TWO;
                game.update_local_players(0.0, 0.0, false);
                assert!(game.locals[player - 1].inventory);
                assert!(!game.player.inventory);
                assert!(game.player.menu.is_none());
                let host_hotbar = game.player.hotbar;
                let other_hotbars: Vec<_> = game.locals.iter().map(|p| p.hotbar).collect();
                game.locals[player - 1].focus = 1;
                game.controller.pads[player].pressed = Wiimote::A;
                game.update_local_players(0.0, 0.0, false);
                assert_eq!(game.player.hotbar, host_hotbar);
                for (index, other) in game.locals.iter().enumerate() {
                    if index == player - 1 {
                        assert_eq!(other.hotbar.material(), world::MATERIALS[1].0);
                    } else {
                        assert_eq!(other.hotbar, other_hotbars[index]);
                    }
                }
                game.gpu.draws.clear();
                game.draw_frame(0.0, 0.0);
                let inventories: Vec<_> = game
                    .gpu
                    .draws
                    .iter()
                    .filter(|draw| {
                        draw.texture == Texture::None
                            && draw.vertices.iter().all(|v| v.color == 0x0000_00aa)
                    })
                    .collect();
                assert_eq!(inventories.len(), 1);
                assert!(!inventories[0].primary_screen);
                let screen_players = if primary_size.is_some() { 4 } else { 5 };
                let view = if primary_size.is_some() {
                    player - 1
                } else {
                    player
                };
                assert_eq!(inventories[0].viewport, viewport(screen_players, view));

                game.controller.pads[player].pressed = Wiimote::TWO;
                game.update_local_players(0.0, 0.0, false);
                assert!(!game.locals[player - 1].inventory);
                game.controller.pads[player].pressed = Wiimote::MINUS;
                game.update_local_players(0.0, 0.0, false);
                assert_eq!(
                    game.locals[player - 1].menu.as_ref().unwrap().page,
                    Page::Tools
                );
                assert!(game.player.menu.is_none());
                mem::swap(&mut game.player, &mut game.locals[player - 1]);
                game.menu_action(Page::Tools, Action::Tool(5));
                game.show(Some(Page::Tools));
                game.menu_action(Page::Tools, Action::Tool(7));
                game.player.menu.as_mut().unwrap().text = "/fly off".into();
                mem::swap(&mut game.player, &mut game.locals[player - 1]);
                game.serve(|server, world| server.tick(world));
                assert!(game.locals[player - 1].camera.flying);
                assert!(!game.player.camera.flying);
                assert_eq!(
                    game.locals[player - 1].menu.as_ref().unwrap().page,
                    Page::Chat
                );
                game.controller.pads[player].pressed = Wiimote::PLUS;
                game.update_local_players(0.0, 0.0, false);
                assert!(game.locals[player - 1].menu.is_none());
                assert!(game.player.menu.is_none());
                game.serve(|server, world| server.tick(world));
                assert!(!game.locals[player - 1].camera.flying);

                game.update_local_players(0.0, 0.0, false);
                assert_eq!(game.page(), Some(Page::Pause));
                game.update_controls(0.0, 0.0);
                assert!(game.player.menu.is_none());
                assert_eq!(game.player_index, 0);
            }
        }
    }

    #[test]
    fn usb_slash_commands_toggle_flight_on_both_consoles() {
        use super::super::desktop::{Desktop, DesktopEvents};

        fn check<const PLAYERS: usize>() {
            let events = DesktopEvents::new();
            let mut game = game_for::<PLAYERS>();
            game.show(None);
            game.controller.desktop = Some(Desktop::new([640.0, 480.0]));
            for expected in [true, false] {
                for code in [0x38, 9, 15, 28, 0x28] {
                    events.key(code, true, None);
                    events.key(code, false, None);
                    game.controller.desktop.as_mut().unwrap().poll(&events, 0.0);
                    game.player.pad = game.controller.desktop(game.context());
                    game.update_controls(TICK, 0.0);
                }
                assert!(game.player.menu.is_none());
                game.serve(|server, world| server.tick(world));
                assert_eq!(game.player.camera.flying, expected);
                assert!(game.messages.iter().any(|(message, _)| {
                    message == if expected { "Flying on" } else { "Flying off" }
                }));
            }
        }
        check::<4>();
        check::<5>();
    }

    #[test]
    fn slash_commands_keep_characters_received_in_the_opening_frame() {
        use super::super::desktop::{Desktop, DesktopEvents};

        let events = DesktopEvents::new();
        let mut game = game();
        game.show(None);
        let mut desktop = Desktop::new([640.0, 480.0]);
        for code in [0x38, 9, 15, 28] {
            events.key(code, true, None);
            events.key(code, false, None);
        }
        desktop.poll(&events, 0.0);
        game.controller.desktop = Some(desktop);
        game.player.pad = game.controller.desktop(Context::Play);
        game.update_controls(TICK, 0.0);
        assert_eq!(game.player.menu.as_ref().unwrap().text, "/fly");
        events.key(0x28, true, None);
        game.controller.desktop.as_mut().unwrap().poll(&events, 0.1);
        game.player.pad = game.controller.desktop(Context::Menu);
        game.update_controls(TICK, 0.1);
        game.serve(|server, world| server.tick(world));
        assert!(game.player.camera.flying);
    }

    #[test]
    fn on_screen_slash_commands_toggle_flight_on_both_consoles() {
        fn check<const PLAYERS: usize>() {
            let mut game = game_for::<PLAYERS>();
            for expected in [true, false] {
                game.show(Some(Page::Chat));
                // The on-screen keys for /fly.
                for index in [46, 29, 34, 18] {
                    game.menu_action(Page::Chat, Action::Key(index));
                }
                assert_eq!(game.player.menu.as_ref().unwrap().text, "/fly");
                game.menu_action(Page::Chat, Action::Enter);
                assert!(game.player.menu.is_none());
                game.serve(|server, world| server.tick(world));
                assert_eq!(game.player.camera.flying, expected);
            }
        }
        check::<4>();
        check::<5>();
    }

    #[test]
    fn desktop_number_keys_assign_the_hovered_inventory_block() {
        let mut game = game();
        game.show(None);
        game.player.inventory = true;
        let left = (game.control_size()[0] - CELL * COLUMNS as f32) / 2.0;
        let mut pad = Pad::default();
        pad.hotbar = Some(2);
        pad.pointer = Some([left + CELL * 1.5, INVENTORY_TOP + CELL / 2.0, 0.0]);
        game.player.pad = pad;
        game.inventory();
        assert_eq!(game.player.hotbar.selected, 0);
        assert_eq!(game.player.hotbar.slots[2], world::MATERIALS[1].0);
    }

    #[test]
    fn inventory_number_keys_do_not_assign_empty_cells() {
        let mut game = game();
        game.show(None);
        game.player.inventory = true;
        game.player.focus = world::MATERIALS.len() - 1;
        let empty = world::MATERIALS.len() - page_start(game.player.focus);
        assert!(empty < INVENTORY_PAGE);
        let left = (game.control_size()[0] - CELL * COLUMNS as f32) / 2.0;
        let hotbar = game.player.hotbar;
        game.player.pad.hotbar = Some(2);
        game.player.pad.pointer = Some([
            left + CELL * ((empty % COLUMNS) as f32 + 0.5),
            INVENTORY_TOP + CELL * ((empty / COLUMNS) as f32 + 0.5),
            0.0,
        ]);
        game.inventory();
        assert_eq!(game.player.hotbar.slots, hotbar.slots);
        assert_eq!(game.player.hotbar.selected, 2);
    }

    #[test]
    fn usb_mouse_wheel_scrolls_shared_lists_without_moving_the_pointer() {
        use super::super::desktop::{Desktop, DesktopEvents};

        let events = DesktopEvents::new();
        let mut game = game();
        game.show(Some(Page::Worlds));
        game.player.menu.as_mut().unwrap().set_rows(
            (0..12)
                .map(|index| Row {
                    title: format!("World {index}"),
                    details: String::new(),
                    enabled: true,
                })
                .collect(),
            None,
        );
        let mut desktop = Desktop::new([640.0, 480.0]);
        events.mouse(0, 0, 0, -1);
        desktop.poll(&events, 0.0);
        game.controller.desktop = Some(desktop);
        game.update_controls(0.0, 0.0);
        let layout = Layout::new(
            game.player.menu.as_ref().unwrap(),
            TestPlatform::MENU,
            game.control_size(),
            [0.0; 4],
        );
        assert!(!layout.actions().any(|action| action == Action::Row(0)));
        assert!(layout.actions().any(|action| action == Action::Row(1)));
    }

    #[test]
    fn usb_desktop_controls_only_p1_and_types_chat_with_a_mouse_cursor() {
        use super::super::desktop::{Desktop, DesktopEvents};

        let events = DesktopEvents::new();
        let mut game = game_for::<5>();
        game.gpu.primary_size = Some([854.0, 480.0]);
        game.set_local_players(2);
        game.show(None);
        let mut desktop = Desktop::new([854.0, 480.0]);
        events.key(26, true, None);
        events.mouse(0, 20, -10, 0);
        desktop.poll(&events, 0.0);
        game.controller.desktop = Some(desktop);
        let yaw = game.player.camera.yaw;
        let pitch = game.player.camera.pitch;
        game.player.pad = game.controller.desktop(Context::Play);
        game.update_controls(TICK, 0.0);
        game.tick_player(TICK);
        assert_eq!(game.player.camera.input.direction, [0.0, 1.0]);
        assert_eq!(game.locals[0].camera.input.direction, [0.0; 2]);
        assert!(
            (game.player.camera.yaw - (yaw + 0.04).rem_euclid(TAU)).abs() < 0.001,
            "yaw before {yaw}, after {}",
            game.player.camera.yaw
        );
        assert!((game.player.camera.pitch - pitch - 0.02).abs() < 0.001);
        events.key(26, false, None);
        events.key(8, true, None);
        game.controller.desktop.as_mut().unwrap().poll(&events, 0.1);
        game.player.pad = game.controller.desktop(Context::Play);
        game.update_controls(TICK, 0.1);
        assert!(game.player.inventory);
        assert!(!game.locals[0].inventory);
        game.gpu.draws.clear();
        game.draw_frame(0.1, 0.0);
        let arrows: Vec<_> = game
            .gpu
            .draws
            .iter()
            .filter(|draw| draw.texture == Texture::Mouse)
            .collect();
        assert_eq!(arrows.len(), 1);
        assert!(arrows[0].primary_screen);
        let pointer = game.controller.mouse_pointer().unwrap();
        assert_eq!(arrows[0].vertices[0].position[0] + 4.5, pointer[0]);
        assert_eq!(arrows[0].vertices[0].position[1] + 4.5, pointer[1]);
        assert_eq!(
            arrows[0].vertices[1].position[0] - arrows[0].vertices[0].position[0],
            48.0
        );
        events.key(8, false, None);
        events.key(0x29, true, None);
        game.controller.desktop.as_mut().unwrap().poll(&events, 0.2);
        game.player.pad = game.controller.desktop(Context::Inventory);
        game.update_controls(TICK, 0.2);
        assert!(!game.player.inventory);
        events.key(0x29, false, None);
        events.key(23, true, None);
        game.controller.desktop.as_mut().unwrap().poll(&events, 0.3);
        game.player.pad = game.controller.desktop(Context::Play);
        game.update_controls(TICK, 0.3);
        assert_eq!(game.page(), Some(Page::Chat));
        events.key(23, false, None);
        for code in [11, 8, 15, 15, 18] {
            events.key(code, true, None);
            events.key(code, false, None);
        }
        game.controller.desktop.as_mut().unwrap().poll(&events, 0.4);
        game.player.pad = game.controller.desktop(Context::Menu);
        game.update_controls(TICK, 0.4);
        assert_eq!(game.player.menu.as_ref().unwrap().text, "hello");
        assert!(game.locals[0].menu.is_none());
        events.key(0x28, true, None);
        game.controller.desktop.as_mut().unwrap().poll(&events, 0.5);
        game.player.pad = game.controller.desktop(Context::Menu);
        game.update_controls(TICK, 0.5);
        assert!(game.player.menu.is_none());
        assert!(
            game.messages
                .iter()
                .any(|(message, _)| message.ends_with(": hello"))
        );
    }

    #[test]
    fn mouse_hotspot_is_the_opaque_upper_left_tip_of_the_original_png() {
        let image = image::decode(include_bytes!("../../assets/mouse.png")).unwrap();
        assert_eq!([image.width(), image.height()], [64, 64]);
        let pixels = image.pixels();
        assert!(pixels[(6 * 64 + 6) * 4 + 3] >= 128);
        assert!(pixels[(5 * 64 + 5) * 4 + 3] < 128);
    }

    #[test]
    fn all_connected_controllers_can_navigate_main_menus_without_joining() {
        use super::super::input::Wiimote;

        fn check<const PLAYERS: usize>(primary: Option<[f32; 2]>) {
            for controller in 0..PLAYERS {
                let mut game = game_for::<PLAYERS>();
                game.gpu.primary_size = primary;
                game.show(Some(Page::Title));
                assert!(game.locals.is_empty());
                let layout = Layout::new(
                    game.player.menu.as_ref().unwrap(),
                    TestPlatform::MENU,
                    game.control_size(),
                    [0.0; 4],
                );
                let first = game.player.menu.as_ref().unwrap().focused(&layout);
                game.controller.pads[controller].pressed = Wiimote::DOWN;
                game.update_controls(0.0, 0.0);
                assert_ne!(game.player.menu.as_ref().unwrap().focused(&layout), first);
                game.controller.pads[controller].pressed = Wiimote::UP;
                game.update_controls(0.0, 0.0);
                assert_eq!(game.player.menu.as_ref().unwrap().focused(&layout), first);
                game.controller.pads[controller].pressed = Wiimote::A;
                game.update_controls(0.0, 0.0);
                assert_eq!(game.page(), Some(Page::Worlds));
                game.controller.pads[controller].pressed = Wiimote::B;
                game.update_controls(0.0, 0.0);
                assert_eq!(game.page(), Some(Page::Title));
                game.show(None);
                game.controller.pads[controller].pressed = Wiimote::PLUS;
                game.player.pad = game.controller.read_player(0, Context::Play);
                game.update_controls(0.0, 0.0);
                assert_eq!(game.page(), Some(Page::Pause));
                game.update_controls(0.0, 0.0);
                assert!(game.player.menu.is_none());
            }
        }
        check::<4>(None);
        check::<5>(Some([854.0, 480.0]));
    }

    #[test]
    fn each_remote_can_click_shared_settings_using_its_own_pointer() {
        use super::super::input::Wiimote;

        for primary in [None, Some([854.0, 480.0])] {
            for controller in 1..5 {
                let mut game = game_for::<5>();
                game.gpu.primary_size = primary;
                game.show(Some(Page::Pause));
                let size = game.control_size();
                let layout = Layout::new(
                    game.player.menu.as_ref().unwrap(),
                    TestPlatform::MENU,
                    size,
                    [0.0; 4],
                );
                let y = (0..480)
                    .find(|&y| {
                        layout.action_at([size[0] / 2.0, y as f32]) == Some(Action::Settings)
                    })
                    .unwrap() as f32;
                game.controller.pads[controller].pointer = Some([320.0, y, 0.0]);
                game.update_controls(0.0, 0.0);
                game.controller.pads[controller].pressed = Wiimote::A;
                game.update_controls(0.0, 0.0);
                assert_eq!(game.page(), Some(Page::Settings));
                game.controller.pads[controller].pressed = Wiimote::B;
                game.update_controls(0.0, 0.0);
                assert_eq!(game.page(), Some(Page::Pause));
            }
        }
    }

    #[test]
    fn an_unassigned_remote_can_change_the_split_screen_player_count() {
        use super::super::input::Wiimote;

        let mut game = game_for::<5>();
        game.gpu.primary_size = Some([854.0, 480.0]);
        game.set_local_players(2);
        for action in [Action::SplitScreen, Action::LocalPlayers(3)] {
            let size = game.control_size();
            let layout = Layout::new(
                game.player.menu.as_ref().unwrap(),
                TestPlatform::MENU,
                size,
                [0.0; 4],
            );
            let y = (0..480)
                .find(|&y| layout.action_at([size[0] / 2.0, y as f32]) == Some(action))
                .unwrap();
            game.controller.pads[4].pointer = Some([320.0, y as f32, 0.0]);
            game.controller.pads[4].pressed = Wiimote::A;
            game.update_controls(0.0, 0.0);
        }
        assert_eq!(game.locals.len(), 2);
        assert_eq!(game.page(), Some(Page::Pause));
    }

    #[test]
    fn resuming_with_a_secondary_remote_does_not_also_place_a_block() {
        use super::super::input::Wiimote;

        let mut game = game();
        game.set_local_players(2);
        game.controller.pads[1].pressed = Wiimote::A;
        game.controller.pads[1].held = Wiimote::A | Wiimote::UP;
        game.update_controls(TICK, 0.0);
        assert!(game.player.menu.is_none());
        assert_eq!(game.locals[0].camera.input, Input::default());
        assert!(!game.locals[0].pad.pressed(Button::Place));
    }

    #[test]
    fn stationary_shared_cursors_do_not_override_another_controllers_dpad() {
        use super::super::input::Wiimote;

        let mut game = game();
        game.show(Some(Page::Pause));
        let layout = Layout::new(
            game.player.menu.as_ref().unwrap(),
            TestPlatform::MENU,
            game.control_size(),
            [0.0; 4],
        );
        let first = layout.actions().next().unwrap();
        let y = (0..480)
            .find(|&y| layout.action_at([320.0, y as f32]) == Some(first))
            .unwrap();
        game.controller.pads[0].pointer = Some([320.0, y as f32, 0.0]);
        game.update_controls(0.0, 0.0);
        game.controller.pads[3].pressed = Wiimote::DOWN;
        game.update_controls(0.0, 0.0);
        let focused = game.player.menu.as_ref().unwrap().focused(&layout);
        assert_ne!(focused, Some(first));
        game.controller.pads[3].pressed = 0;
        game.update_controls(0.0, 0.0);
        assert_eq!(game.player.menu.as_ref().unwrap().focused(&layout), focused);
    }

    #[test]
    fn local_overlays_can_be_open_together_while_other_players_keep_moving() {
        use super::super::input::Wiimote;

        let mut game = game_for::<5>();
        game.gpu.primary_size = Some([854.0, 480.0]);
        game.set_local_players(5);
        game.show(None);
        game.player.inventory = true;
        game.controller.pads[1].pressed = Wiimote::MINUS;
        game.controller.pads[2].pressed = Wiimote::TWO;
        game.controller.pads[3].pressed = Wiimote::MINUS;
        game.controller.pads[4].held = Wiimote::UP;
        game.update_local_players(TICK, 0.0, false);
        assert!(game.player.inventory);
        assert_eq!(game.locals[0].menu.as_ref().unwrap().page, Page::Tools);
        assert!(game.locals[1].inventory);
        assert_eq!(game.locals[2].menu.as_ref().unwrap().page, Page::Tools);
        assert_eq!(game.locals[3].camera.input.direction, [0.0, 1.0]);
        for remote in &mut game.controller.pads {
            remote.pressed = 0;
        }
        game.show(Some(Page::Pause));
        game.update_local_players(TICK, 0.0, false);
        assert!(
            game.locals
                .iter()
                .all(|player| player.camera.input == Input::default())
        );
        assert_eq!(game.locals[0].menu.as_ref().unwrap().page, Page::Tools);
        assert!(game.locals[1].inventory);
        game.show(None);
        game.update_local_players(TICK, 0.0, false);
        assert_eq!(game.locals[3].camera.input.direction, [0.0, 1.0]);
    }

    #[test]
    fn a_stationary_pointer_does_not_undo_dpad_navigation() {
        use super::super::input::Wiimote;

        let mut game = game();
        game.show(Some(Page::Pause));
        let layout = Layout::new(
            game.player.menu.as_ref().unwrap(),
            TestPlatform::MENU,
            game.control_size(),
            [0.0; 4],
        );
        let first = layout.actions().next().unwrap();
        let last = layout.actions().last().unwrap();
        let pointer = (0..480)
            .find_map(|y| {
                (0..640).find_map(|x| {
                    (layout.action_at([x as f32, y as f32]) == Some(first))
                        .then_some([x as f32, y as f32, 0.0])
                })
            })
            .unwrap();
        let mut remote = Wiimote {
            pointer: Some(pointer),
            pressed: Wiimote::UP,
            ..Wiimote::default()
        };
        game.player.pad = remote.pad(Context::Menu);
        game.menu();
        assert_eq!(
            game.player.menu.as_ref().unwrap().focused(&layout),
            Some(last)
        );
        remote.pressed = 0;
        game.player.pad = remote.pad(Context::Menu);
        game.menu();
        assert_eq!(
            game.player.menu.as_ref().unwrap().focused(&layout),
            Some(last)
        );
        game.show(None);
        game.player.inventory = true;
        let x = (game.control_size()[0] - CELL * COLUMNS as f32) / 2.0 + CELL / 2.0;
        remote.pointer = Some([x, INVENTORY_TOP + CELL / 2.0, 0.0]);
        remote.pressed = Wiimote::RIGHT;
        game.player.pad = remote.pad(Context::Inventory);
        game.inventory();
        assert_eq!(game.player.focus, 1);
        remote.pressed = 0;
        game.player.pad = remote.pad(Context::Inventory);
        game.inventory();
        assert_eq!(game.player.focus, 1);
    }

    #[test]
    fn connected_remote_cursors_are_visible_before_enabling_split_screen() {
        let mut game = game();
        game.show(Some(Page::Title));
        assert!(game.locals.is_empty());
        for (index, remote) in game.controller.pads[..4].iter_mut().enumerate() {
            remote.pointer = Some([100.0 + index as f32 * 100.0, 200.0, 0.0]);
        }
        game.gpu.draws.clear();
        game.draw_frame(0.0, 0.0);
        let cursors: Vec<_> = game
            .gpu
            .draws
            .iter()
            .filter_map(|draw| {
                if let Texture::Cursor(index) = draw.texture {
                    Some(index)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(cursors, [3, 2, 1, 0]);
        game.controller.pads[2].pointer = None;
        game.gpu.draws.clear();
        game.draw_frame(0.0, 0.0);
        assert!(
            !game
                .gpu
                .draws
                .iter()
                .any(|draw| draw.texture == Texture::Cursor(2))
        );
    }

    #[test]
    fn secondary_remotes_move_with_the_dpad_and_look_with_the_nunchuk() {
        use super::super::input::Wiimote;

        let mut game = game_for::<5>();
        game.set_local_players(5);
        game.show(None);
        for remote in &mut game.controller.pads[1..] {
            remote.held = Wiimote::UP;
            remote.stick = Some([0.0, 0.5]);
        }
        game.update_local_players(TICK, 0.0, false);
        for player in &game.locals {
            assert!(!player.camera.input.fast);
            assert_eq!(player.camera.input.direction, [0.0, 1.0]);
        }
        assert!(!game.player.camera.input.fast);
    }

    #[test]
    fn numbered_menu_cursors_are_visible_for_connected_remotes() {
        use super::super::input::Wiimote;
        let mut game = game();
        for count in 2..=LOCAL_PLAYERS {
            game.set_local_players(count);
            let layout = Layout::new(
                game.player.menu.as_ref().unwrap(),
                TestPlatform::MENU,
                game.gpu.size(),
                [0.0; 4],
            );
            let settings_y = (0..480)
                .find(|&y| layout.action_at([320.0, y as f32]) == Some(Action::Settings))
                .unwrap() as f32;
            game.controller.pads[0].pointer = Some([20.0, 30.0, 0.0]);
            game.player.pad = game.controller.read_player(0, Context::Menu);
            game.menu();
            for pad in game.controller.pads.iter_mut().take(count).skip(1) {
                *pad = Wiimote {
                    pointer: Some([320.0, settings_y, 0.0]),
                    pressed: Wiimote::A,
                    held: Wiimote::A,
                    ..Wiimote::default()
                };
            }
            game.gpu.draws.clear();
            game.draw_frame(0.0, 0.0);
            let cursors: Vec<_> = game
                .gpu
                .draws
                .iter()
                .filter_map(|draw| match draw.texture {
                    Texture::Cursor(player) => Some((player, draw)),
                    _ => None,
                })
                .collect();
            assert_eq!(
                cursors
                    .iter()
                    .map(|(player, _)| *player)
                    .collect::<Vec<_>>(),
                (0..count).rev().collect::<Vec<_>>()
            );
            for (player, draw) in cursors {
                assert_eq!(draw.viewport, [0.0, 0.0, 1.0, 1.0]);
                let pointer = game.controller.pads[player].pointer.unwrap();
                assert_eq!(
                    draw.vertices[0].position,
                    [pointer[0] - 24.0, pointer[1] - 24.0, 0.0]
                );
            }
            game.controller.pads[count - 1].pointer = None;
            game.gpu.draws.clear();
            game.draw_frame(0.0, 0.0);
            assert!(
                !game
                    .gpu
                    .draws
                    .iter()
                    .any(|draw| draw.texture == Texture::Cursor(count - 1))
            );
            game.show(None);
            game.gpu.draws.clear();
            game.draw_frame(0.0, 0.0);
            assert!(
                !game
                    .gpu
                    .draws
                    .iter()
                    .any(|draw| matches!(draw.texture, Texture::Cursor(_)))
            );
        }
    }

    #[test]
    fn numbered_cursors_follow_tilt_without_moving_the_pointer_tip() {
        let mut game = game();
        game.set_local_players(LOCAL_PLAYERS);
        let diagonal = 24.0 / 2.0_f32.sqrt();
        for (angle, top) in [
            (0.0, [0.0, -24.0]),
            (45.0, [diagonal, -diagonal]),
            (90.0, [24.0, 0.0]),
            (-45.0, [-diagonal, -diagonal]),
            (-90.0, [-24.0, 0.0]),
            (180.0, [0.0, 24.0]),
        ] {
            for (player, pad) in game.controller.pads.iter_mut().enumerate() {
                pad.pointer = Some([100.0 + player as f32 * 80.0, 240.0, angle]);
            }
            game.gpu.draws.clear();
            game.draw_frame(0.0, 0.0);
            for player in 0..LOCAL_PLAYERS {
                let draw = game
                    .gpu
                    .draws
                    .iter()
                    .find(|draw| draw.texture == Texture::Cursor(player))
                    .unwrap();
                let pointer = game.controller.pads[player].pointer.unwrap();
                for axis in 0..2 {
                    let center = draw
                        .vertices
                        .iter()
                        .map(|vertex| vertex.position[axis])
                        .sum::<f32>()
                        / 4.0;
                    assert!((center - pointer[axis]).abs() < 0.001);
                    let edge =
                        (draw.vertices[0].position[axis] + draw.vertices[1].position[axis]) / 2.0;
                    assert!((edge - pointer[axis] - top[axis]).abs() < 0.001);
                }
            }
        }
    }

    #[test]
    fn chat_and_debug_info_are_drawn_once_across_the_full_screen() {
        let mut game = game();
        game.settings.game.show_stats = true;
        for count in 1..=LOCAL_PLAYERS {
            game.set_local_players(count);
            game.messages.clear();
            game.messages.push_back(("Expired".into(), -CHAT_TIME));
            game.messages.push_back(("First message".into(), 0.0));
            game.messages.push_back(("Second message".into(), 0.5));
            game.gpu.draws.clear();
            game.draw_frame(1.0, 0.0);
            let rows: Vec<_> = game
                .gpu
                .draws
                .iter()
                .filter(|draw| {
                    draw.texture == Texture::None
                        && draw.vertices.iter().all(|v| v.color == 0x0000_00c0)
                })
                .collect();
            assert_eq!(rows.len(), 2);
            for draw in &rows {
                assert_eq!(draw.viewport, [0.0, 0.0, 1.0, 1.0]);
                assert!((draw.size[0] - 640.0).abs() < 0.001);
                assert!((draw.size[1] - 480.0).abs() < 0.001);
                assert_eq!(
                    draw.vertices[1].position[0] - draw.vertices[0].position[0],
                    624.0
                );
            }
            assert_eq!(rows[0].vertices[0].position[1], 406.0);
            assert_eq!(rows[1].vertices[0].position[1], 382.0);
            // While paused, all text (including debug information) belongs to the global UI.
            assert!(
                game.gpu
                    .draws
                    .iter()
                    .filter(|draw| draw.texture == Texture::Font)
                    .all(|draw| draw.viewport == [0.0, 0.0, 1.0, 1.0])
            );
            game.gpu.draws.clear();
            game.draw_frame(CHAT_TIME + 1.0, 0.0);
            assert!(
                !game
                    .gpu
                    .draws
                    .iter()
                    .any(|draw| draw.texture == Texture::None
                        && draw.vertices.iter().all(|v| v.color == 0x0000_00c0))
            );
        }
    }

    #[test]
    fn separate_primary_screen_draws_one_full_view_with_chat_menus_and_inventory() {
        let mut game = game_for::<5>();
        let primary_size = [854.0, 480.0];
        game.gpu.primary_size = Some(primary_size);
        game.settings.game.show_stats = true;
        for count in 1..=5 {
            game.set_local_players(count);
            game.messages.clear();
            game.messages.push_back(("Shared chat".into(), 0.0));
            for context in [Context::Play, Context::Menu, Context::Inventory] {
                game.show(if context == Context::Menu {
                    Some(Page::Pause)
                } else {
                    None
                });
                game.player.inventory = context == Context::Inventory;
                game.player.swing = 0.5;
                game.gpu.draws.clear();
                game.gpu.viewports.clear();
                game.draw_frame(1.0, 0.1);
                assert!((game.player.swing - 0.4).abs() < 0.001);
                let screen_players = (count - 1).max(1);
                assert_eq!(
                    &game.gpu.viewports[..screen_players],
                    &(0..screen_players)
                        .map(|index| viewport(screen_players, index))
                        .collect::<Vec<_>>()
                );
                assert_eq!(
                    &game.gpu.viewports[screen_players..],
                    &[[0.0, 0.0, 1.0, 1.0]; 2]
                );
                for primary in [false, true] {
                    let size = if primary {
                        primary_size
                    } else {
                        [640.0, 480.0]
                    };
                    let draws: Vec<_> = game
                        .gpu
                        .draws
                        .iter()
                        .filter(|draw| draw.primary_screen == primary)
                        .collect();
                    assert!(!draws.is_empty());
                    let rows: Vec<_> = draws
                        .iter()
                        .filter(|draw| {
                            draw.texture == Texture::None
                                && draw.vertices.iter().all(|v| v.color == 0x0000_00c0)
                        })
                        .collect();
                    assert_eq!(rows.len(), 1);
                    assert_eq!(rows[0].viewport, [0.0, 0.0, 1.0, 1.0]);
                    assert!((rows[0].size[0] - size[0]).abs() < 0.001);
                    assert_eq!(
                        rows[0].vertices[1].position[0] - rows[0].vertices[0].position[0],
                        size[0] - 16.0
                    );
                    if primary {
                        assert!(
                            draws
                                .iter()
                                .all(|draw| draw.viewport == [0.0, 0.0, 1.0, 1.0])
                        );
                        assert!(
                            draws
                                .iter()
                                .all(|draw| (draw.size[0] - size[0]).abs() < 0.001
                                    && (draw.size[1] - size[1]).abs() < 0.001)
                        );
                    }
                    if context == Context::Inventory {
                        assert_eq!(
                            draws.iter().any(|draw| draw.texture == Texture::None
                                && draw.vertices.iter().all(|v| v.color == 0x0000_00aa)),
                            primary || count == 1
                        );
                    }
                    if context != Context::Play {
                        assert!(
                            draws
                                .iter()
                                .filter(|draw| draw.texture == Texture::Font)
                                .all(|draw| draw.viewport == [0.0, 0.0, 1.0, 1.0])
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn separate_primary_screen_uses_full_screen_touch_coordinates() {
        let mut game = game_for::<5>();
        game.gpu.primary_size = Some([854.0, 480.0]);
        game.set_local_players(2);
        game.show(None);
        game.player.pad.pointer = Some([427.0, 450.0, 0.0]);
        game.map_pointer(0);
        assert_eq!(game.player.pad.pointer, Some([427.0, 450.0, 0.0]));
        assert_eq!(game.hotbar_slot(game.player.pad.pointer, true), Some(4));
        game.player.pad.pointer = Some([320.0, 450.0, 0.0]);
        game.map_pointer(1);
        assert_eq!(game.player.pad.pointer, Some([320.0, 450.0, 0.0]));
        assert_eq!(game.hotbar_slot(game.player.pad.pointer, false), Some(4));
        for count in 2..=5 {
            game.set_local_players(count);
            game.show(None);
            for index in 1..count {
                game.player.pad.pointer = Some([320.0, 450.0, 0.0]);
                game.map_pointer(index);
                let width = if count == 3 { 1280.0 } else { 640.0 };
                assert_eq!(game.view_size(), [width, 480.0]);
                assert_eq!(game.player.pad.pointer, Some([width / 2.0, 450.0, 0.0]));
                assert_eq!(game.hotbar_slot(game.player.pad.pointer, false), Some(4));
            }
        }
        game.player.inventory = true;
        assert_eq!(game.hotbar_slot(Some([427.0, 450.0, 0.0]), true), Some(4));
        game.show(Some(Page::Pause));
        assert_eq!(game.control_size(), [854.0, 480.0]);
        let layout = Layout::new(
            game.player.menu.as_ref().unwrap(),
            TestPlatform::MENU,
            game.control_size(),
            [0.0; 4],
        );
        let y = (0..480)
            .find(|&y| layout.action_at([427.0, y as f32]) == Some(Action::Settings))
            .unwrap() as f32;
        game.player.pad.pointer = Some([427.0, y, 0.0]);
        game.menu();
        assert_eq!(
            game.player.menu.as_ref().unwrap().focused(&layout),
            Some(Action::Settings)
        );
    }

    #[test]
    fn split_viewports_are_disjoint_and_keep_hud_coordinates_readable() {
        for count in 1..=5 {
            let views: Vec<_> = (0..count).map(|index| viewport(count, index)).collect();
            for (index, &[x, y, w, h]) in views.iter().enumerate() {
                assert!(x >= 0.0 && y >= 0.0 && x + w <= 1.0 && y + h <= 1.0);
                for &[ox, oy, ow, oh] in &views[index + 1..] {
                    assert!(x + w <= ox || ox + ow <= x || y + h <= oy || oy + oh <= y);
                }
            }
        }
        let mut game = game();
        game.set_local_players(2);
        assert_eq!(game.view_size(), [1280.0, 480.0]);
        game.show(None);
        game.player.pad.pointer = Some([320.0, 240.0, 0.0]);
        game.map_pointer(0);
        assert_eq!(game.player.pad.pointer, Some([640.0, 240.0, 0.0]));
        game.set_local_players(4);
        assert_eq!(game.view_size(), [640.0, 480.0]);
    }
}
