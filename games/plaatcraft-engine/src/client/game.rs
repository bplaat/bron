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
use crate::sound::{Movement, ground};
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

/// The game of a console, its port runs it with [`Game::run`].
pub struct Game<P: Platform> {
    gpu: P::Gpu,
    controller: P::Controller,
    renderer: Renderer,
    stream: Stream<P>,
    camera: Camera,
    hotbar: Hotbar,
    target: Option<Hit>,
    night: bool,
    menu: Option<Menu>,
    // The inventory's block grid is open
    inventory: bool,
    exit: bool,
    focus: usize,
    pad: Pad,
    sounds: Sounds<P::Audio>,
    breaking: Repeat,
    placing: Repeat,
    last_jump: Option<f32>,
    name_shown: Option<(u8, f32)>,
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
    swing: f32,
    particles: Particles,
    network: Network<P>,
    // The integrated server of a world played on this console, and what it saves
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
    found: Vec<(Address, Announcement, f32)>,
    state_elapsed: f32,
    remote_revision: u64,
    others: Others,
}

impl<P: Platform> Game<P> {
    /// Starts the game on the title screen, with the settings of the SD card.
    pub fn new(gpu: P::Gpu, audio: P::Audio, controller: P::Controller) -> Self {
        let renderer = Renderer::new(gpu.size(), P::RENDER_DISTANCE);
        let storage = Storage::new();
        let sqlite = storage.sqlite;
        let worlds = storage.worlds();
        let settings = storage.load_settings().unwrap_or_else(Settings::new::<P>);
        let mut sounds = Sounds::new(audio);
        sounds.set_volume(settings.game.volume);
        Self {
            gpu,
            controller,
            renderer,
            storage,
            worlds,
            stream: Stream::start(sqlite),
            camera: background_camera(),
            hotbar: Hotbar::default(),
            target: None,
            night: false,
            menu: Some(Menu::default()),
            inventory: false,
            exit: false,
            focus: 0,
            pad: Pad::default(),
            sounds,
            breaking: Repeat::default(),
            placing: Repeat::default(),
            last_jump: None,
            name_shown: None,
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
            swing: 0.0,
            particles: Particles::default(),
            network: Network::new(),
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
            others: Others::default(),
        }
    }

    fn seconds(&self) -> f32 {
        (P::seconds() - self.started) as f32
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
        self.menu.as_ref().map(|menu| menu.page)
    }
    fn selected_world(&self) -> Option<String> {
        let selected = self.menu.as_ref()?.selected?;
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

    const fn context(&self) -> Context {
        if self.inventory {
            Context::Inventory
        } else if self.menu.is_some() {
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
            self.pad = self.controller.read(self.context());
            self.network_events(now);
            if self.playing()
                && !self.page().is_some_and(Page::typing)
                && self.pad.pressed(Button::Pause)
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
            self.gpu.present();
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

    // The engine's menus with the pointer and the D-pad, select presses and back goes back.
    fn menu(&mut self) {
        let size = self.gpu.size();
        let (pad, session) = (self.pad, self.session());
        let Some(menu) = &mut self.menu else {
            return;
        };
        menu.session = session;
        let page = menu.page;
        if let Some([x, y, _]) = pad.pointer {
            menu.point(&Layout::new(menu, P::MENU, size, [0.0; 4]), [x, y]);
        }
        for (button, direction) in [
            (Button::Up, [0.0, -1.0]),
            (Button::Down, [0.0, 1.0]),
            (Button::Left, [-1.0, 0.0]),
            (Button::Right, [1.0, 0.0]),
        ] {
            if pad.pressed(button) {
                menu.step(&Layout::new(menu, P::MENU, size, [0.0; 4]), direction);
            }
        }
        let layout = Layout::new(menu, P::MENU, size, [0.0; 4]);
        let focused = menu.focused(&layout);
        if focused == Some(Action::Volume) {
            let volume = &mut self.settings.game.volume;
            let step = i16::from(pad.pressed(Button::Right)) - i16::from(pad.pressed(Button::Left));
            *volume = (i16::from(*volume) + step * 10).clamp(0, menu::MAX_VOLUME.into()) as u8;
            if pad.pressed(Button::Select)
                && let Some([x, y, _]) = pad.pointer
            {
                *volume = (layout.slider_fraction([x, y]) * 10.0 + 0.5) as u8 * 10;
            }
            self.sounds.set_volume(*volume);
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
                self.settings.game = Settings::new::<P>().game;
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
                    self.tell(format!(
                        "World opened to LAN (up to {} guests)",
                        P::MAX_GUESTS
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
                network::Event::Joined(id, name) if self.hosting => {
                    let mut joined = Err(String::new());
                    self.serve(|server, world| joined = server.join(world, Some(id), &name));
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
            input: if self.menu.is_none() && !self.inventory {
                Input {
                    direction: [0.0; 2],
                    stick: self.pad.walk,
                    jump: self.pad.held(Button::Jump),
                    sneak: self.pad.held(Button::Sneak),
                    fast: self.pad.held(Button::Fast),
                }
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
        let (pad, count) = (self.pad, world::MATERIALS.len());
        let page = page_start(self.focus);
        for (button, step) in [
            (Button::Left, count - 1),
            (Button::Right, 1),
            (Button::Up, count - COLUMNS),
            (Button::Down, COLUMNS),
        ] {
            if pad.pressed(button) {
                self.focus = (self.focus + step) % count;
            }
        }
        if let Some([x, y, _]) = pad.pointer {
            let left = (self.gpu.size()[0] - CELL * COLUMNS as f32) / 2.0;
            let bottom = INVENTORY_TOP + CELL * ROWS as f32;
            if x >= left && x < left + CELL * COLUMNS as f32 && (INVENTORY_TOP..bottom).contains(&y)
            {
                let index = ((y - INVENTORY_TOP) / CELL) as usize * COLUMNS
                    + ((x - left) / CELL) as usize
                    + page;
                if index < count {
                    self.focus = index;
                }
            }
        }
        let mut hotbar = self.hotbar;
        if pad.pressed(Button::Previous) {
            hotbar.cycle(1);
        }
        if pad.pressed(Button::Next) {
            hotbar.cycle(-1);
        }
        // Pressing on a hotbar slot selects it, elsewhere it fills the selected slot
        if pad.pressed(Button::Select) {
            match self.hotbar_slot(pad.pointer) {
                Some(slot) => hotbar.selected = slot,
                None => hotbar.assign(hotbar.selected, world::MATERIALS[self.focus].0),
            }
        }
        self.set_hotbar(hotbar);
        if pad.pressed(Button::Back) {
            self.show(None);
        }
    }

    // Looks around, selects blocks and edits, the physics runs in tick_player.
    fn play(&mut self, dt: f32, now: f32) {
        let pad = self.pad;
        if pad.pressed(Button::Pick) {
            if let Some(hit) = self.target
                && let Some(material) = world::item_material(hit.material)
            {
                let mut hotbar = self.hotbar;
                hotbar.pick(material);
                self.set_hotbar(hotbar);
            }
            return;
        }
        if pad.pressed(Button::Inventory) {
            self.show(None);
            self.inventory = true;
            return;
        }
        if pad.pressed(Button::Tools) {
            self.show(Some(Page::Tools));
            return;
        }
        let turn = |value: f32| value * value.abs() * LOOK_SPEED * dt / 0.002;
        let [yaw, pitch] = pad.look;
        self.camera
            .look(f64::from(turn(yaw)), f64::from(-turn(pitch)));
        if pad.pressed(Button::Jump) {
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
        let mut hotbar = self.hotbar;
        if pad.pressed(Button::Previous) {
            hotbar.cycle(1);
        }
        if pad.pressed(Button::Next) {
            hotbar.cycle(-1);
        }
        if pad.pressed(Button::Tap)
            && let Some(slot) = self.hotbar_slot(pad.pointer)
        {
            hotbar.selected = slot;
        }
        self.set_hotbar(hotbar);
        self.aim();
        if self.breaking.update(&pad, Button::Break, now) {
            self.edit(false);
        }
        if self.placing.update(&pad, Button::Place, now) {
            self.edit(true);
        }
    }
    // The hotbar slot under the pointer.
    fn hotbar_slot(&self, pointer: Option<[f32; 3]>) -> Option<usize> {
        let [x, y, _] = pointer?;
        hud::hotbar_slot(self.gpu.size(), [x, y])
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

    // The block grid of the inventory, the consoles' version of the desktop inventory.
    fn draw_inventory(hud: &mut Hud<'_, P::Gpu>, focus: usize, hotbar: &Hotbar) {
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
        let help = P::INVENTORY_HELP;
        hud.label(help, hud.width - 24.0 - text_width(help, 16.0), 360.0, 16.0);
        hud.hotbar(hotbar);
    }

    fn render(&mut self, now: f32) {
        let gpu = &mut self.gpu;
        let daylight = if self.night { NIGHT_DAYLIGHT } else { 1.0 };
        // Between the last two physics ticks, so the player moves smoothly
        let camera = if self.active || self.stream.remote {
            self.prediction.view(&self.camera)
        } else {
            self.camera
        };
        let [width, height] = gpu.size();
        let view = camera.view(width as u32, height as u32, P::RENDER_DISTANCE);
        let center = camera.chunk();
        let in_game = self.menu.is_none() && !self.inventory;
        // The title pages show the turning default world behind them
        let eye = camera.position.map(|v| v.floor() as i64);
        let eye_material = self.stream.block(eye);
        self.renderer.begin(gpu, daylight, now, eye_material);
        self.renderer.draw_sky(gpu, &view, eye_material, now);
        self.renderer
            .draw_world(gpu, &view, center, &self.stream.chunks);
        if self.active || self.stream.remote {
            if in_game && let Some(hit) = self.target {
                self.renderer
                    .draw_selection(gpu, &view, center, hit.position, hit.material);
            }
            self.renderer.draw_players(gpu, &view, center, &self.others);
            self.renderer
                .draw_particles(gpu, &view, center, &self.particles);
            if in_game {
                self.renderer.draw_held(
                    gpu,
                    self.hotbar.material(),
                    1.0 - self.swing / SWING_DURATION,
                );
            }
        }
        let mut hud = Hud::begin(gpu);
        if in_game {
            for (name, [x, y]) in self.others.name_tags(&view, center) {
                hud.label(name, x - text_width(name, 16.0) / 2.0, y, 16.0);
            }
        }
        if self.settings.game.show_stats && (self.active || self.stream.remote) {
            let [x, y, z] = self.camera.position;
            hud.label(
                &format!(
                    "{} FPS - {x:.0}, {y:.0}, {z:.0} - {} chunks - distance {}",
                    self.fps,
                    self.stream.chunks.len(),
                    P::RENDER_DISTANCE
                ),
                12.0,
                10.0,
                16.0,
            );
        }
        match &self.menu {
            _ if self.inventory => Self::draw_inventory(&mut hud, self.focus, &self.hotbar),
            Some(menu) => {
                let size = [hud.width, hud.height];
                let layout = Layout::new(menu, P::MENU, size, [0.0; 4]);
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
                        (hud.width - text_width(text, 18.0)) / 2.0,
                        top - 30.0,
                        18.0,
                    );
                }
                if self.camera.flying {
                    hud.label("Flying", 12.0, 32.0, 16.0);
                }
            }
        }
        if !in_game && let Some(pointer) = self.pad.pointer {
            hud.cursor(pointer);
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
    }
}
// Looks over the default world from above its spawn, like the desktop title screen.
fn background_camera() -> Camera {
    Camera {
        pitch: -0.2,
        ..Camera::default()
    }
}
