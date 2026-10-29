/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#![allow(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use bwindow::{
    Event, EventLoopBuilder, Key, KeyCode, KeyboardEvent, LogicalInsets, LogicalPoint, LogicalSize,
    Modifiers, MouseButton, NamedKey, Theme, Window, WindowBuilder, WindowEvent,
};
use plaatcraft_engine::prediction::Prediction;
use plaatcraft_engine::{avatars, camera, particles, protocol, world};

mod controls;
mod database;
mod hud;
mod interaction;
mod inventory;
mod lan;
mod menu;
mod scroll;
mod server;
mod settings;
mod sound;
mod stream;
mod textures;
mod worlds;

const TITLE: &str = "PlaatCraft";
// Keep terrain uploads short so rendering and input take priority over loading.
const CHUNK_UPLOAD_BUDGET: Duration = Duration::from_millis(1);
const MAX_CHUNK_UPLOADS: usize = 32;
// The title screen background turns around a nearby generated world, like Minecraft.
const BACKGROUND_DISTANCE: u8 = 16;
const BACKGROUND_TURN_SPEED: f32 = 0.05;
// How long the held block swings when the player breaks or places a block, in seconds.
const SWING_DURATION: f32 = 0.3;
// Typed server addresses can be long host names.
const MAX_ADDRESS_LENGTH: usize = 64;
const DIGITS: [KeyCode; inventory::SLOTS] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
];

// The world the client joined, its server and what the client streamed of it.
struct World {
    server: server::Connection,
    stream: stream::Stream,
    interaction: interaction::Interaction,
    others: avatars::Others,
    particles: particles::Particles,
    // The player moves in physics ticks like the server, ahead of the server.
    prediction: Prediction,
}

struct GpuChunk {
    faces: bwebgpu::Buffer,
    sections: stream::SectionRanges,
    slot: u32,
}

const fn offset(coord: world::Coord, center: world::Coord) -> [f32; 3] {
    [
        ((coord.0 - center.0) * world::CHUNK_SIZE as i64) as f32,
        0.0,
        ((coord.1 - center.1) * world::CHUNK_SIZE as i64) as f32,
    ]
}

fn offset_bytes(value: [f32; 3]) -> [u8; 16] {
    let mut bytes = [0; 16];
    for (word, value) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(value) {
        *word = value.to_ne_bytes();
    }
    bytes
}

fn pipeline(
    device: &bwebgpu::Device,
    source: bwebgpu::ShaderModuleDescriptor<'_>,
    layout: &bwebgpu::PipelineLayout,
    format: bwebgpu::TextureFormat,
    fragment: &str,
) -> bwebgpu::RenderPipeline {
    let shader = device.create_shader_module(source);
    let liquid = fragment == "fs_liquid";
    let terrain = fragment == "fs_terrain" || liquid;
    let selection = fragment == "fs_selection";
    let player = fragment == "fs_player";
    let held = fragment == "fs_held";
    let buffers = [Some(bwebgpu::VertexBufferLayout {
        array_stride: if selection {
            12
        } else if player {
            avatars::INSTANCE_BYTES
        } else {
            world::QUAD_BYTES as u64
        },
        step_mode: bwebgpu::VertexStepMode::Instance,
        attributes: if selection {
            &bwebgpu::vertex_attr_array![0 => Float32x3] as &[_]
        } else if player {
            &bwebgpu::vertex_attr_array![
                0 => Float32x3, 1 => Float32x3, 2 => Float32x3, 3 => Float32x3
            ]
        } else {
            &bwebgpu::vertex_attr_array![0 => Uint32x3]
        },
    })];
    device.create_render_pipeline(&bwebgpu::RenderPipelineDescriptor {
        label: Some(fragment),
        layout: Some(layout),
        vertex: bwebgpu::VertexState {
            module: &shader,
            entry_point: Some(if selection {
                "vs_selection"
            } else if held {
                "vs_held"
            } else if player {
                "vs_player"
            } else if liquid {
                "vs_liquid"
            } else if terrain {
                "vs_terrain"
            } else {
                "vs_screen"
            }),
            compilation_options: Default::default(),
            buffers: if terrain || selection || player {
                &buffers
            } else {
                &[]
            },
        },
        primitive: bwebgpu::PrimitiveState {
            cull_mode: if terrain && !liquid || held {
                Some(bwebgpu::Face::Back)
            } else {
                None
            },
        },
        depth_stencil: Some(bwebgpu::DepthStencilState {
            format: bwebgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(terrain && !liquid || player),
            depth_compare: Some(
                if matches!(fragment, "fs_crosshair" | "fs_preview" | "fs_held") {
                    bwebgpu::CompareFunction::Always
                } else {
                    bwebgpu::CompareFunction::LessEqual
                },
            ),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        fragment: Some(bwebgpu::FragmentState {
            module: &shader,
            entry_point: Some(fragment),
            compilation_options: Default::default(),
            targets: &[Some(bwebgpu::ColorTargetState {
                format,
                blend: (liquid || fragment == "fs_preview")
                    .then_some(bwebgpu::BlendState::ALPHA_BLENDING),
                write_mask: bwebgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

struct Game {
    window: Window,
    surface: bwebgpu::Surface<'static>,
    device: bwebgpu::Device,
    queue: bwebgpu::Queue,
    config: bwebgpu::SurfaceConfiguration,
    terrain_pipeline: bwebgpu::RenderPipeline,
    liquid_pipeline: bwebgpu::RenderPipeline,
    sky_pipeline: bwebgpu::RenderPipeline,
    crosshair_pipeline: bwebgpu::RenderPipeline,
    selection_pipeline: bwebgpu::RenderPipeline,
    preview_pipeline: bwebgpu::RenderPipeline,
    player_pipeline: bwebgpu::RenderPipeline,
    held_pipeline: bwebgpu::RenderPipeline,
    // The box instances of the other players and of the bits of broken blocks.
    player_buffer: bwebgpu::Buffer,
    particle_buffer: bwebgpu::Buffer,
    hud: hud::Hud,
    stats: hud::Stats,
    chat: hud::Chat,
    name_tags: hud::NameTags,
    // On-screen controls on touch screens, where the game is always playing.
    touch: Option<controls::TouchControls>,
    safe_area: LogicalInsets,
    hotbar: inventory::Hotbar,
    inventory: inventory::Inventory,
    scroll: scroll::Scroll,
    // The mouse position over the window, for hovering the inventory.
    pointer: Option<LogicalPoint>,
    menu: menu::Menu,
    // Listens for LAN worlds while the multiplayer page is open.
    discovery: Option<lan::Discovery>,
    settings: settings::Settings,
    preferences: settings::Store,
    sounds: sound::Sounds,
    gpu_radius: i64,
    // Touch screens have no pointer lock, so they track the menu themselves.
    paused: bool,
    selection_buffer: bwebgpu::Buffer,
    worlds: worlds::Worlds,
    world: Option<World>,
    // The unsaved default world behind the menu while no world is joined.
    background: Option<stream::Stream>,
    dirty: HashSet<world::Coord>,
    bind_group: bwebgpu::BindGroup,
    origins: bwebgpu::Buffer,
    origin_bytes: Vec<u8>,
    free_slots: Vec<u32>,
    uniforms: bwebgpu::Buffer,
    depth: bwebgpu::TextureView,
    camera: camera::Camera,
    // The held keys and modifiers, and the held touch button that descends like Shift.
    keys: HashSet<KeyCode>,
    modifiers: Modifiers,
    descend: bool,
    night: bool,
    chunks: HashMap<world::Coord, GpuChunk>,
    gpu_center: world::Coord,
    started: Instant,
    last_frame: Instant,
    // The progress of the held block's swing, from 0 to 1, when it swings.
    swing: Option<f32>,
    frames: u32,
    fps: u32,
    visible: u32,
    last_stats: Instant,
}

impl Game {
    fn new(window: Window, attachment: bwindow::WindowAttachment, worlds: worlds::Worlds) -> Self {
        let preferences = settings::Store::new().expect("Open preferences");
        let mut menu = menu::Menu::default();
        menu.username.clone_from(&preferences.identity().name);
        let settings = preferences.settings();
        let scale = attachment.scale_factor();
        let instance = bwebgpu::Instance::default();
        let surface = instance.create_surface(attachment).expect("Create surface");
        let adapter = instance
            .request_adapter(&bwebgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
            })
            .expect("Find graphics adapter");
        let (device, queue) = adapter
            .request_device(&bwebgpu::DeviceDescriptor::default())
            .expect("Create graphics device");
        let (width, height) = surface.size();
        let mut config = surface
            .get_default_config(&adapter, width.max(1), height.max(1))
            .expect("Surface is supported");
        config.format = surface
            .get_capabilities(&adapter)
            .formats
            .into_iter()
            .find(bwebgpu::TextureFormat::is_srgb)
            .unwrap_or(config.format);
        config.present_mode = bwebgpu::PresentMode::AutoVsync;
        surface.configure(&device, &config);
        let uniforms = device.create_buffer(&bwebgpu::BufferDescriptor {
            label: Some("Fly camera"),
            size: 96,
            usage: bwebgpu::BufferUsages::UNIFORM | bwebgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // The material table followed by the chunk origins, the one storage binding.
        let origins = device.create_buffer(&bwebgpu::BufferDescriptor {
            label: Some("Material and chunk origin tables"),
            size: (textures::MATERIAL_TABLE_BYTES + stream::RESIDENT_CAPACITY * 16) as u64,
            usage: bwebgpu::BufferUsages::STORAGE | bwebgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&origins, 0, &textures::material_table());
        let scene_layout = device.create_bind_group_layout(&bwebgpu::BindGroupLayoutDescriptor {
            label: Some("Scene layout"),
            entries: &[
                bwebgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: bwebgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: bwebgpu::BindingType::Buffer {
                        ty: bwebgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                bwebgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: bwebgpu::ShaderStages::FRAGMENT,
                    ty: bwebgpu::BindingType::Texture {
                        sample_type: bwebgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: bwebgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                bwebgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: bwebgpu::ShaderStages::FRAGMENT,
                    ty: bwebgpu::BindingType::Sampler(bwebgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                bwebgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: bwebgpu::ShaderStages::VERTEX,
                    ty: bwebgpu::BindingType::Buffer {
                        ty: bwebgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&bwebgpu::PipelineLayoutDescriptor {
            label: Some("Scene pipeline layout"),
            bind_group_layouts: &[Some(&scene_layout)],
        });
        let terrain_pipeline = pipeline(
            &device,
            bwebgpu::include_wgsl!("terrain.wgsl"),
            &layout,
            config.format,
            "fs_terrain",
        );
        let liquid_pipeline = pipeline(
            &device,
            bwebgpu::include_wgsl!("liquid.wgsl"),
            &layout,
            config.format,
            "fs_liquid",
        );
        let sky_pipeline = pipeline(
            &device,
            bwebgpu::include_wgsl!("sky.wgsl"),
            &layout,
            config.format,
            "fs_sky",
        );
        let crosshair_pipeline = pipeline(
            &device,
            bwebgpu::include_wgsl!("crosshair.wgsl"),
            &layout,
            config.format,
            "fs_crosshair",
        );
        let selection_pipeline = pipeline(
            &device,
            bwebgpu::include_wgsl!("selection.wgsl"),
            &layout,
            config.format,
            "fs_selection",
        );
        let preview_pipeline = pipeline(
            &device,
            bwebgpu::include_wgsl!("preview.wgsl"),
            &layout,
            config.format,
            "fs_preview",
        );
        let player_pipeline = pipeline(
            &device,
            bwebgpu::include_wgsl!("player.wgsl"),
            &layout,
            config.format,
            "fs_player",
        );
        let held_pipeline = pipeline(
            &device,
            bwebgpu::include_wgsl!("held.wgsl"),
            &layout,
            config.format,
            "fs_held",
        );
        let player_buffer = device.create_buffer(&bwebgpu::BufferDescriptor {
            label: Some("Other players"),
            size: avatars::INSTANCE_BYTES * avatars::MAX_INSTANCES as u64,
            usage: bwebgpu::BufferUsages::VERTEX | bwebgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let particle_buffer = device.create_buffer(&bwebgpu::BufferDescriptor {
            label: Some("Block particles"),
            size: avatars::INSTANCE_BYTES * particles::MAX_PARTICLES as u64,
            usage: bwebgpu::BufferUsages::VERTEX | bwebgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let hud = hud::Hud::new(&device, config.format);
        let stats = hud::Stats::new(&hud, &device, scale);
        let selection_buffer = device.create_buffer(&bwebgpu::BufferDescriptor {
            label: Some("Target block"),
            size: 12,
            usage: bwebgpu::BufferUsages::VERTEX | bwebgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let (tiles, sampler) = textures::load(&device, &queue);
        let bind_group = device.create_bind_group(&bwebgpu::BindGroupDescriptor {
            label: Some("Scene resources"),
            layout: &scene_layout,
            entries: &[
                bwebgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                bwebgpu::BindGroupEntry {
                    binding: 1,
                    resource: bwebgpu::BindingResource::TextureView(&tiles),
                },
                bwebgpu::BindGroupEntry {
                    binding: 2,
                    resource: bwebgpu::BindingResource::Sampler(&sampler),
                },
                bwebgpu::BindGroupEntry {
                    binding: 3,
                    resource: origins.as_entire_binding(),
                },
            ],
        });
        let depth = Self::create_depth(&device, &config);
        let safe_area = window.safe_area();
        Self {
            window,
            surface,
            device,
            queue,
            config,
            terrain_pipeline,
            liquid_pipeline,
            sky_pipeline,
            crosshair_pipeline,
            selection_pipeline,
            preview_pipeline,
            player_pipeline,
            held_pipeline,
            player_buffer,
            particle_buffer,
            hud,
            chat: hud::Chat::default(),
            name_tags: hud::NameTags::default(),
            stats,
            touch: cfg!(any(target_os = "ios", target_os = "android"))
                .then(controls::TouchControls::default),
            safe_area,
            hotbar: inventory::Hotbar::default(),
            inventory: inventory::Inventory::default(),
            scroll: scroll::Scroll::default(),
            pointer: None,
            menu,
            discovery: None,
            sounds: sound::Sounds::new(&settings),
            settings,
            preferences,
            gpu_radius: 0,
            paused: true,
            selection_buffer,
            worlds,
            world: None,
            background: Some(stream::Stream::new(0)),
            dirty: HashSet::new(),
            bind_group,
            origins,
            origin_bytes: vec![0; stream::RESIDENT_CAPACITY * 16],
            free_slots: (0..stream::RESIDENT_CAPACITY as u32).rev().collect(),
            uniforms,
            depth,
            camera: background_camera(),
            keys: HashSet::new(),
            modifiers: Modifiers::NONE,
            descend: false,
            night: false,
            chunks: HashMap::new(),
            gpu_center: (0, 0),
            started: Instant::now(),
            last_frame: Instant::now(),
            swing: None,
            frames: 0,
            fps: 0,
            visible: 0,
            last_stats: Instant::now(),
        }
    }

    // Whether input controls the player: a locked pointer or touch controls.
    fn playing(&self) -> bool {
        !self.inventory.open
            && !self.chat.typing()
            && (self.window.pointer_locked() || self.touch.is_some() && !self.paused)
    }

    // The main menu shows at startup and while paused: on desktops while the pointer is
    // released and the inventory is closed.
    fn menu_open(&self) -> bool {
        if self.touch.is_some() {
            self.paused
        } else {
            !self.window.pointer_locked() && !self.inventory.open && !self.chat.typing()
        }
    }

    fn menu_layout(&self) -> menu::Layout {
        menu::Layout::new(
            self.window.size(),
            self.safe_area,
            self.touch.is_none(),
            &self.menu,
        )
    }

    fn save_settings(&mut self) {
        self.sounds.apply(&self.settings);
        if let Err(error) = self.preferences.save(self.settings) {
            eprintln!("Could not save settings: {error}");
        }
    }

    fn menu_press(&mut self, position: LogicalPoint) {
        use menu::{Action, Page};
        let layout = self.menu_layout();
        let action = layout.action_at(position);
        if action.is_some() {
            self.sounds.click();
        }
        match action {
            Some(Action::SinglePlayer) => self.show_worlds(None),
            Some(Action::MultiPlayer) => self.show_servers(),
            Some(Action::Resume) => self.play(),
            Some(Action::Settings) => self.menu.page = Page::Settings,
            Some(Action::Quit) => {
                self.leave_world();
                self.window.close();
            }
            Some(Action::OpenToLan) => self.open_to_lan(),
            Some(Action::QuitToTitle) => {
                let guest = self.menu.session == menu::Session::Guest;
                self.leave_world();
                if guest {
                    self.show_servers();
                } else {
                    self.menu.page = Page::Title;
                }
            }
            Some(Action::Row(index)) => {
                if self.menu.selected == Some(index) {
                    self.play_selected();
                } else {
                    self.menu.selected = Some(index);
                    self.menu.start_list_drag(&layout, position);
                }
            }
            Some(Action::PlayWorld | Action::JoinServer) => self.play_selected(),
            Some(Action::CreateWorld) if self.touch.is_some() => {
                // Touch screens have no keyboard, so new worlds get a free name.
                let name = self.worlds.unused_name();
                self.create_world(&name);
            }
            Some(Action::CreateWorld) => self.edit_text(Page::Create),
            Some(Action::RenameWorld) => self.edit_text(Page::Rename),
            Some(Action::DirectConnect) => self.edit_text(Page::DirectConnect),
            Some(Action::Username) => self.edit_text(Page::Username),
            Some(Action::DeleteWorld) => self.menu.page = Page::Delete,
            Some(Action::Confirm) => self.menu_confirm(),
            Some(Action::Back) => self.menu_back(),
            Some(action @ (Action::Distance | Action::Volume)) => {
                self.menu.dragging = Some(action);
                self.menu_drag(position);
            }
            Some(Action::Stats) => self.settings.show_stats = !self.settings.show_stats,
            Some(Action::AutoJump) => self.settings.auto_jump = !self.settings.auto_jump,
            Some(Action::Reset) => self.settings = settings::defaults(),
            // The build tools and the on-screen keyboard are for consoles, which have no chat
            // and keyboard.
            Some(
                Action::Tool(_)
                | Action::Key(_)
                | Action::Space
                | Action::Erase
                | Action::Tab
                | Action::Shift(_)
                | Action::CapsLock
                | Action::Enter,
            ) => {}
            None if layout.in_list(position) => {
                self.menu.start_list_drag(&layout, position);
            }
            None => {}
        }
        self.save_settings();
    }

    fn menu_drag(&mut self, position: LogicalPoint) {
        let layout = self.menu_layout();
        let fraction = layout.slider_fraction(position);
        match self.menu.dragging {
            Some(menu::Action::Distance) => {
                self.settings.distance = settings::distance_at(fraction);
            }
            Some(menu::Action::Volume) => {
                self.settings.volume = (fraction * f32::from(settings::MAX_VOLUME)).round() as u8;
                self.sounds.apply(&self.settings);
            }
            _ => self.menu.list_drag(&layout, position),
        }
    }

    fn end_menu_drag(&mut self) {
        self.menu.end_drag();
        self.save_settings();
    }

    fn show_worlds(&mut self, selected: Option<&str>) {
        self.menu.show_worlds(self.worlds.list(), selected);
    }

    // Shows the worlds on the LAN, which are found while the page is open.
    fn show_servers(&mut self) {
        self.menu
            .username
            .clone_from(&self.preferences.identity().name);
        self.menu.show_servers();
        if self.discovery.is_none() {
            match lan::Discovery::start() {
                Ok(discovery) => self.discovery = Some(discovery),
                Err(error) => {
                    self.menu.error = Some(format!("Can't search the LAN: {error}"));
                }
            }
        }
    }

    // Opens a page with a text field, filled with what it edits.
    fn edit_text(&mut self, page: menu::Page) {
        self.menu.text = match page {
            menu::Page::Create => self.worlds.unused_name(),
            menu::Page::Rename => self.menu.selected_world().unwrap_or_default().to_owned(),
            menu::Page::Username => self.preferences.identity().name.clone(),
            _ => String::new(),
        };
        self.menu.error = None;
        self.menu.page = page;
    }

    fn menu_confirm(&mut self) {
        use menu::Page;
        let selected = self.menu.selected_world().map(str::to_owned);
        let text = self.menu.text.clone();
        match (self.menu.page, selected) {
            (Page::Create, _) => self.create_world(&text),
            (Page::Rename, Some(selected)) => match self.worlds.rename(&selected, &text) {
                Ok(name) => self.show_worlds(Some(&name)),
                Err(error) => self.menu.error = Some(error),
            },
            (Page::Delete, Some(selected)) => {
                if let Err(error) = self.worlds.delete(&selected) {
                    eprintln!("Could not delete world: {error}");
                }
                self.show_worlds(None);
            }
            (Page::Username, _) => match self.preferences.set_name(&text) {
                Ok(()) => {
                    self.menu
                        .username
                        .clone_from(&self.preferences.identity().name);
                    self.menu.page = Page::Settings;
                }
                Err(error) => self.menu.error = Some(error),
            },
            (Page::DirectConnect, _) => {
                if let Err(error) = self.join_world(&text) {
                    self.menu.error = Some(error);
                }
            }
            _ => {}
        }
    }

    // Goes back to the page the current page was opened from.
    fn menu_back(&mut self) {
        use menu::Page;
        self.menu.end_drag();
        match self.menu.page {
            Page::Create | Page::Rename | Page::Delete => {
                let selected = self.menu.selected_world().map(str::to_owned);
                self.show_worlds(selected.as_deref());
            }
            Page::DirectConnect => self.show_servers(),
            Page::Username => self.menu.page = Page::Settings,
            Page::Settings if self.world.is_some() => self.menu.page = Page::Pause,
            _ => {
                self.discovery = None;
                self.menu.page = Page::Title;
            }
        }
    }

    // Handles the keys the menu uses, and returns whether it used the key.
    fn menu_key(&mut self, event: &KeyboardEvent) -> bool {
        use menu::Page;
        let page = self.menu.page;
        match event.code {
            Some(KeyCode::Escape) if !matches!(page, Page::Title | Page::Pause) => {
                self.menu_back();
                self.save_settings();
            }
            Some(KeyCode::Enter) => match page {
                Page::Title => self.show_worlds(None),
                Page::Pause => self.play(),
                Page::Worlds | Page::Multiplayer => self.play_selected(),
                Page::Settings => self.menu_back(),
                _ => self.menu_confirm(),
            },
            Some(KeyCode::ArrowUp) if matches!(page, Page::Worlds | Page::Multiplayer) => {
                self.menu.select_step(-1);
            }
            Some(KeyCode::ArrowDown) if matches!(page, Page::Worlds | Page::Multiplayer) => {
                self.menu.select_step(1);
            }
            _ if page.typing() => self.type_text(event),
            _ => return false,
        }
        true
    }

    // Types in the text field, addresses also take colons and brackets.
    fn type_text(&mut self, event: &KeyboardEvent) {
        let (allowed, length): (fn(char) -> bool, usize) = match self.menu.page {
            menu::Page::DirectConnect => (
                |c| c.is_ascii_alphanumeric() || ".:-_[]%".contains(c),
                MAX_ADDRESS_LENGTH,
            ),
            menu::Page::Username => (protocol::valid_char, protocol::MAX_NAME_LENGTH),
            _ => (protocol::valid_char, worlds::MAX_NAME_LENGTH),
        };
        match &event.key {
            Key::Named(NamedKey::Backspace) => {
                self.menu.text.pop();
            }
            Key::Character(text)
                if !event.modifiers.contains(Modifiers::CONTROL)
                    && !event.modifiers.contains(Modifiers::META) =>
            {
                for c in text.chars().filter(|&c| allowed(c)) {
                    if self.menu.text.chars().count() < length {
                        self.menu.text.push(c);
                    }
                }
            }
            _ => return,
        }
        self.menu.error = None;
    }

    fn create_world(&mut self, name: &str) {
        match self.worlds.create(name) {
            Ok(name) => self.enter_world(&name),
            Err(error) => {
                if self.menu.page == menu::Page::Create {
                    self.menu.error = Some(error);
                } else {
                    eprintln!("Could not create world: {error}");
                }
            }
        }
    }

    // Plays the selected world, or joins the selected LAN world.
    fn play_selected(&mut self) {
        if self.menu.page == menu::Page::Multiplayer {
            if let Some(found) = self.menu.selected_server().filter(|found| found.compatible) {
                let address = found.address.to_string();
                if let Err(error) = self.join_world(&address) {
                    self.menu.error = Some(error);
                }
            }
        } else if let Some(name) = self.menu.selected_world().map(str::to_owned) {
            self.enter_world(&name);
        }
    }

    // Starts the server of a world and joins it as the host.
    fn enter_world(&mut self, name: &str) {
        let identity = self.preferences.identity().clone();
        match server::Connection::host(self.worlds.path(name), name.to_owned(), identity) {
            Ok((server, welcome)) => self.start_world(server, &welcome),
            Err(error) => {
                eprintln!("Could not open world {name}: {error}");
                self.show_worlds(Some(name));
                self.menu.error = Some(error);
            }
        }
    }

    // Joins a world on the LAN.
    fn join_world(&mut self, address: &str) -> Result<(), String> {
        let (server, welcome) = server::Connection::join(address, self.preferences.identity())?;
        self.discovery = None;
        self.start_world(server, &welcome);
        Ok(())
    }

    fn start_world(&mut self, server: server::Connection, welcome: &protocol::Welcome) {
        self.menu.session = if server.is_host() {
            menu::Session::Host { lan: None }
        } else {
            menu::Session::Guest
        };
        self.background = None;
        self.clear_chunks();
        self.camera = camera::Camera::default();
        welcome.player.restore(&mut self.camera);
        self.night = welcome.player.night;
        self.hotbar = welcome.player.hotbar;
        self.world = Some(World {
            server,
            stream: stream::Stream::new(welcome.revision),
            interaction: interaction::Interaction::default(),
            others: avatars::Others::default(),
            particles: particles::Particles::default(),
            prediction: Prediction::new(&self.camera),
        });
        self.chat.clear();
        self.play();
    }

    fn open_to_lan(&mut self) {
        let Some(world) = &mut self.world else {
            return;
        };
        match world.server.open_to_lan() {
            Ok(port) => {
                self.menu.session = menu::Session::Host { lan: Some(port) };
                self.chat.push(
                    None,
                    format!("Opened to LAN on port {port}, others can join now"),
                );
            }
            Err(error) => self.chat.push(None, error),
        }
    }

    // Saves the player and stops the server, which closes the world, then shows the
    // background again.
    fn leave_world(&mut self) {
        self.save_settings();
        self.close_chat();
        if self.world.take().is_none() {
            return;
        }
        self.menu.session = menu::Session::None;
        self.clear_chunks();
        self.background = Some(stream::Stream::new(0));
        if self.inventory.open {
            self.inventory.toggle();
        }
        self.camera = background_camera();
        self.night = false;
        self.paused = true;
    }

    fn clear_chunks(&mut self) {
        for (_, chunk) in self.chunks.drain() {
            self.free_slots.push(chunk.slot);
        }
        self.dirty.clear();
        self.gpu_radius = 0;
    }

    // The stream of the joined world, or else of the background.
    fn stream(&self) -> Option<&stream::Stream> {
        self.world
            .as_ref()
            .map(|world| &world.stream)
            .or(self.background.as_ref())
    }

    // Releases the touch controls and shows the menu on touch screens.
    fn pause(&mut self) {
        self.touch_event(&WindowEvent::Blur);
        self.paused = true;
    }

    fn play(&mut self) {
        if self.world.is_none() {
            return;
        }
        self.menu.end_drag();
        self.menu.touch_id = None;
        self.menu.page = menu::Page::Pause;
        if self.touch.is_some() {
            self.paused = false;
        } else if let Err(error) = self.window.request_pointer_lock() {
            eprintln!("Could not lock pointer: {error}");
        }
    }

    // Sends a chat message or a command to the server.
    fn send_chat(&self, text: &str) {
        if let Some(world) = &self.world {
            world.server.send(protocol::Request::Chat(text.to_owned()));
        }
    }

    fn open_chat(&mut self, text: &str) {
        self.chat.open(text);
        self.window.exit_pointer_lock();
    }

    fn close_chat(&mut self) {
        if self.chat.typing() {
            self.chat.close();
            if self.world.is_some()
                && self.touch.is_none()
                && let Err(error) = self.window.request_pointer_lock()
            {
                eprintln!("Could not lock pointer: {error}");
            }
        }
    }

    // Types in the chat, Enter sends the message and Escape closes it.
    fn chat_key(&mut self, event: &KeyboardEvent) {
        match (&event.key, event.code) {
            (_, Some(KeyCode::Escape)) => self.close_chat(),
            (_, Some(KeyCode::Enter)) => {
                let text = self.chat.text().trim().to_owned();
                if !text.is_empty() {
                    self.send_chat(&text);
                }
                self.close_chat();
            }
            (Key::Named(NamedKey::Backspace), _) => self.chat.backspace(),
            (Key::Character(text), _)
                if !event.modifiers.contains(Modifiers::CONTROL)
                    && !event.modifiers.contains(Modifiers::META) =>
            {
                self.chat.type_text(text, protocol::MAX_CHAT_LENGTH);
            }
            _ => {}
        }
    }

    fn touch_layout(&self) -> controls::Layout {
        controls::Layout::new(self.window.size(), self.safe_area, self.camera.flying)
    }

    fn inventory_layout(&self) -> inventory::Layout {
        inventory::Layout::new(
            self.window.size(),
            self.safe_area,
            self.touch.is_some(),
            self.inventory.open,
            self.touch
                .as_ref()
                .map(|_| self.touch_layout().hotbar_band()),
        )
    }

    fn set_hotbar(&mut self, hotbar: inventory::Hotbar) {
        if hotbar == self.hotbar {
            return;
        }
        self.sounds.click();
        if hotbar.material() != self.hotbar.material() {
            self.inventory.show_name(hotbar.material());
        }
        self.hotbar = hotbar;
        if let Some(world) = &self.world {
            world.server.send(protocol::Request::Hotbar(hotbar));
        }
    }

    // The inventory needs the mouse, so it releases the pointer lock while open.
    fn toggle_inventory(&mut self) {
        self.inventory.toggle();
        self.sounds.click();
        if let Some(touch) = &mut self.touch {
            if self.inventory.open {
                let actions = touch.reset();
                self.camera.input.stick = [0.0; 2];
                self.touch_actions(actions);
            }
            return;
        }
        if self.inventory.open {
            self.window.exit_pointer_lock();
        } else if let Err(error) = self.window.request_pointer_lock() {
            eprintln!("Could not lock pointer: {error}");
        }
    }

    fn inventory_action(&mut self, action: inventory::Action) {
        let mut hotbar = self.hotbar;
        match action {
            inventory::Action::Select(slot) => hotbar.selected = slot,
            inventory::Action::Assign(slot, material) => hotbar.assign(slot, material),
            inventory::Action::Toggle => return self.toggle_inventory(),
        }
        self.set_hotbar(hotbar);
    }

    // Selects a hotbar slot, or puts the hovered inventory block in it.
    fn number_key(&mut self, slot: usize) {
        let mut hotbar = self.hotbar;
        match self.inventory.hovered() {
            Some(material) if self.inventory.open => hotbar.assign(slot, material),
            _ => hotbar.selected = slot,
        }
        self.set_hotbar(hotbar);
    }

    fn touch_event(&mut self, event: &WindowEvent) {
        if self.paused {
            match event {
                WindowEvent::TouchStart(event) => {
                    event.prevent_default();
                    if self.menu.touch_id.is_none()
                        && let Some(touch) = event.changed_touches.first()
                    {
                        self.menu.touch_id = Some(touch.identifier);
                        self.menu_press(touch.position);
                    }
                }
                WindowEvent::TouchMove(event) => {
                    if let Some(touch) = event
                        .changed_touches
                        .iter()
                        .find(|touch| Some(touch.identifier) == self.menu.touch_id)
                    {
                        self.menu_drag(touch.position);
                    }
                }
                WindowEvent::TouchEnd(event) | WindowEvent::TouchCancel(event) => {
                    if event
                        .changed_touches
                        .iter()
                        .any(|touch| Some(touch.identifier) == self.menu.touch_id)
                    {
                        self.menu.touch_id = None;
                        self.end_menu_drag();
                    }
                }
                _ => {
                    self.menu.end_drag();
                    self.menu.touch_id = None;
                }
            }
            return;
        }
        let layout = self.touch_layout();
        let inventory_layout = self.inventory_layout();
        let selected = self.hotbar.selected;
        let mut inventory_actions = Vec::new();
        let Some(touch) = &mut self.touch else {
            return;
        };
        let actions = match event {
            WindowEvent::TouchStart(event) => {
                // The controls handle every touch, so skip the emulated mouse events.
                event.prevent_default();
                let mut controls = Vec::new();
                for &changed in &event.changed_touches {
                    if self.inventory.open {
                        self.inventory
                            .touch_start(changed.identifier, changed.position);
                    } else if let Some(action) =
                        self.inventory
                            .click(&inventory_layout, changed.position, selected)
                    {
                        inventory_actions.push(action);
                    } else {
                        controls.push(changed);
                    }
                }
                touch.start(&controls, &layout, Instant::now())
            }
            WindowEvent::TouchMove(event) => {
                for changed in &event.changed_touches {
                    self.inventory.touch_move(
                        &inventory_layout,
                        changed.identifier,
                        changed.position,
                    );
                }
                touch.moved(&event.changed_touches)
            }
            WindowEvent::TouchEnd(event) => {
                for changed in &event.changed_touches {
                    inventory_actions.extend(self.inventory.touch_end(
                        &inventory_layout,
                        changed.identifier,
                        changed.position,
                        selected,
                    ));
                }
                touch.end(&event.changed_touches, Instant::now())
            }
            WindowEvent::TouchCancel(event) => {
                for changed in &event.changed_touches {
                    self.inventory.touch_cancel(changed.identifier);
                }
                touch.cancel(&event.changed_touches)
            }
            WindowEvent::Blur => touch.reset(),
            _ => return,
        };
        for action in inventory_actions {
            self.inventory_action(action);
        }
        self.touch_actions(actions);
    }

    fn touch_actions(&mut self, actions: Vec<controls::Action>) {
        for action in actions {
            if (self.inventory.open || self.paused)
                && !matches!(action, controls::Action::Release(_))
            {
                continue;
            }
            match action {
                controls::Action::Look(dx, dy) => {
                    self.camera.look(f64::from(dx) * 1.5, f64::from(dy) * 1.5);
                }
                controls::Action::Press(controls::Button::Jump) => {
                    self.keys.insert(KeyCode::Space);
                }
                controls::Action::Release(controls::Button::Jump) => {
                    self.keys.remove(&KeyCode::Space);
                }
                controls::Action::Press(controls::Button::Down) => self.descend = true,
                controls::Action::Release(controls::Button::Down) => self.descend = false,
                controls::Action::Break => self.click_block(MouseButton::Left),
                controls::Action::Place => self.click_block(MouseButton::Right),
                controls::Action::Press(controls::Button::Fly) => {
                    self.send_chat(if self.camera.flying {
                        "/fly off"
                    } else {
                        "/fly on"
                    });
                    self.descend = false;
                }
                controls::Action::Press(controls::Button::Night) => {
                    self.send_chat("/time");
                }
                controls::Action::Press(controls::Button::Menu) => self.pause(),
                controls::Action::Release(_) => {}
            }
        }
    }

    fn pointer_lock_changed(&mut self) {
        self.keys.clear();
        self.modifiers = Modifiers::NONE;
        self.scroll.reset();
    }

    // Combines the held keys with the touch joystick and buttons.
    fn input(&self) -> camera::Input {
        let pressed = |key| f32::from(u8::from(self.keys.contains(&key)));
        camera::Input {
            direction: [
                pressed(KeyCode::KeyD) - pressed(KeyCode::KeyA),
                pressed(KeyCode::KeyW) - pressed(KeyCode::KeyS),
            ],
            stick: self.camera.input.stick,
            jump: self.keys.contains(&KeyCode::Space),
            sneak: self.modifiers.contains(Modifiers::SHIFT) || self.descend,
            fast: self.modifiers.contains(Modifiers::CONTROL),
        }
    }

    // The camera where it is drawn, see the engine's prediction.
    fn view_camera(&self) -> camera::Camera {
        self.world
            .as_ref()
            .map_or(self.camera, |world| world.prediction.view(&self.camera))
    }

    fn camera_view(&self) -> camera::View {
        self.view_camera().view(
            self.config.width,
            self.config.height,
            i64::from(self.settings.distance),
        )
    }

    // Runs the physics ticks of the passed time, like the server does with the inputs.
    fn tick_player(&mut self, dt: f32) {
        let controls = protocol::Controls {
            input: if self.playing() {
                self.input()
            } else {
                camera::Input::default()
            },
            yaw: self.camera.yaw,
            pitch: self.camera.pitch,
            auto_jump: self.settings.auto_jump,
        };
        let Some(world) = &mut self.world else {
            return;
        };
        let (interaction, sounds) = (&world.interaction, &mut self.sounds);
        let requests = world.prediction.tick(
            dt,
            &mut self.camera,
            controls,
            |position| interaction.block(position),
            |before, after, block| {
                let ground = block(sound::ground(after));
                sounds.movement(sound::Movement::of(before), after, ground);
            },
        );
        for request in requests {
            world.server.send(request);
        }
    }

    // Moves the player to the server's body and predicts the inputs it did not handle yet.
    fn reconcile(&mut self, sequence: u32, body: protocol::Body) {
        let Some(world) = &mut self.world else {
            return;
        };
        let flying = self.camera.flying;
        let interaction = &world.interaction;
        world
            .prediction
            .reconcile(sequence, body, &mut self.camera, |position| {
                interaction.block(position)
            });
        if self.camera.flying != flying {
            self.sounds.fly(self.camera.flying);
        }
    }

    // Hands the server's responses to the parts of the client that requested them.
    fn receive(&mut self) {
        let Some(world) = &mut self.world else {
            return;
        };
        let mut disconnected = None;
        let mut state = None;
        while let Some(response) = world.server.receive() {
            match response {
                protocol::Response::Terrain {
                    coord,
                    revision,
                    chunk,
                } => world.stream.receive(coord, revision, chunk),
                protocol::Response::Blocks { coord, tag, chunk } => {
                    world
                        .interaction
                        .receive(self.camera.chunk(), coord, tag, chunk);
                }
                protocol::Response::Edited {
                    edits,
                    revision: Some(revision),
                    by,
                } => {
                    // The player already applied its own edits, others play their sound.
                    if by != world.server.id {
                        if let [edit] = edits.as_slice()
                            && by != server::WORLD
                        {
                            let broken = edit.material == 0;
                            let before = world.interaction.block(edit.position);
                            if let Some(material) =
                                if broken { before } else { Some(edit.material) }
                            {
                                self.sounds
                                    .block(&self.camera, edit.position, material, broken);
                                if broken {
                                    world.particles.burst(edit.position, material);
                                }
                            }
                        }
                        world.interaction.apply_edits(&edits);
                    }
                    let centers: HashSet<_> = edits
                        .iter()
                        .map(|edit| {
                            (
                                edit.position[0].div_euclid(16),
                                edit.position[2].div_euclid(16),
                            )
                        })
                        .collect();
                    for center in centers {
                        for coord in world.stream.invalidate(center, revision) {
                            if self.chunks.contains_key(&coord) {
                                self.dirty.insert(coord);
                            }
                        }
                    }
                }
                protocol::Response::Edited { revision: None, .. } => world.interaction.reload(),
                protocol::Response::State {
                    sequence,
                    body,
                    others,
                } => {
                    world.others.update(others);
                    state = Some((sequence, body));
                }
                protocol::Response::Chat { name, text } => self.chat.push(Some(name), text),
                protocol::Response::Message(message) => self.chat.push(None, message),
                protocol::Response::Time { night } => self.night = night,
                protocol::Response::Disconnected(reason) => {
                    disconnected = Some(reason);
                    break;
                }
            }
        }
        // Only the newest state matters.
        if let Some((sequence, body)) = state {
            self.reconcile(sequence, body);
        }
        if let Some(reason) = disconnected {
            let guest = self.menu.session == menu::Session::Guest;
            self.window.exit_pointer_lock();
            self.leave_world();
            if guest {
                self.show_servers();
            } else {
                self.show_worlds(None);
            }
            self.menu.error = Some(reason);
        }
    }

    fn update_chunks(&mut self) {
        let center = self.view_camera().chunk();
        let view = self.camera_view();
        let (stream, source, distance) = match (&mut self.world, &mut self.background) {
            (Some(world), _) => (
                &mut world.stream,
                stream::Source::Server(&world.server),
                self.settings.distance,
            ),
            (None, Some(background)) => (
                background,
                stream::Source::Generate(world::WORLD_SEED),
                self.settings.distance.min(BACKGROUND_DISTANCE),
            ),
            (None, None) => return,
        };
        stream.radius = i64::from(distance);
        stream.update(source, center, |coord| view.visible(offset(coord, center)));
        let radius = stream.radius;
        let mut origins_changed = self.gpu_center != center || self.gpu_radius != radius;
        if origins_changed {
            self.dirty
                .retain(|&coord| stream::in_range(coord, center, radius));
            self.chunks.retain(|&coord, chunk| {
                let keep = stream::in_range(coord, center, radius);
                if !keep {
                    self.free_slots.push(chunk.slot);
                }
                keep
            });
            for (&coord, chunk) in &self.chunks {
                let start = chunk.slot as usize * 16;
                self.origin_bytes[start..start + 16]
                    .copy_from_slice(&offset_bytes(offset(coord, center)));
            }
            self.gpu_center = center;
            self.gpu_radius = radius;
        }
        // A time budget handles variable mesh sizes; the count cap bounds burst work.
        let upload_started = Instant::now();
        let mut uploaded = 0;
        for index in 0..stream.loading.len() {
            let coord = stream.loading[index];
            if self.chunks.contains_key(&coord) && !self.dirty.contains(&coord) {
                continue;
            }
            let Some(cached) = stream.cache.get(&coord) else {
                continue;
            };
            let slot = self
                .chunks
                .get(&coord)
                .map(|c| c.slot)
                .unwrap_or_else(|| self.free_slots.pop().expect("Free chunk slot"));
            let faces = self.device.create_buffer(&bwebgpu::BufferDescriptor {
                label: Some("Packed greedy chunk quads"),
                size: cached.mesh.len() as u64,
                usage: bwebgpu::BufferUsages::VERTEX,
                mapped_at_creation: true,
            });
            {
                let mut mapped = faces
                    .slice(..)
                    .get_mapped_range_mut()
                    .expect("Map new chunk buffer");
                for (index, input) in cached
                    .mesh
                    .as_chunks::<{ world::QUAD_BYTES }>()
                    .0
                    .iter()
                    .enumerate()
                {
                    let mut output = *input;
                    let quad = world::with_chunk_slot(
                        [
                            u32::from_ne_bytes(
                                input[..4].try_into().expect("Packed quad position"),
                            ),
                            u32::from_ne_bytes(input[4..8].try_into().expect("Packed quad extent")),
                        ],
                        slot,
                    );
                    output[..4].copy_from_slice(&quad[0].to_ne_bytes());
                    output[4..8].copy_from_slice(&quad[1].to_ne_bytes());
                    let start = index * world::QUAD_BYTES;
                    mapped
                        .slice(start..start + world::QUAD_BYTES)
                        .copy_from_slice(&output);
                }
            }
            faces.unmap();
            let start = slot as usize * 16;
            self.origin_bytes[start..start + 16]
                .copy_from_slice(&offset_bytes(offset(coord, center)));
            origins_changed = true;
            self.chunks.insert(
                coord,
                GpuChunk {
                    faces,
                    sections: cached.sections.clone(),
                    slot,
                },
            );
            stream.mark_uploaded(coord);
            self.dirty.remove(&coord);
            uploaded += 1;
            if uploaded >= MAX_CHUNK_UPLOADS || upload_started.elapsed() >= CHUNK_UPLOAD_BUDGET {
                break;
            }
        }
        if origins_changed {
            self.queue.write_buffer(
                &self.origins,
                textures::MATERIAL_TABLE_BYTES as u64,
                &self.origin_bytes,
            );
        }
        debug_assert_eq!(
            self.free_slots.len() + self.chunks.len(),
            stream::RESIDENT_CAPACITY
        );
    }

    fn create_depth(
        device: &bwebgpu::Device,
        config: &bwebgpu::SurfaceConfiguration,
    ) -> bwebgpu::TextureView {
        device
            .create_texture(&bwebgpu::TextureDescriptor {
                label: Some("Depth"),
                size: bwebgpu::Extent3d {
                    width: config.width,
                    height: config.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: bwebgpu::TextureDimension::D2,
                format: bwebgpu::TextureFormat::Depth32Float,
                usage: bwebgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default())
    }

    fn resize(&mut self) {
        let (width, height) = self.surface.size();
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.depth = Self::create_depth(&self.device, &self.config);
    }

    // Physical pixels per logical pixel of the window content.
    fn scale(&self) -> f32 {
        let logical = self.window.size();
        if logical.width > 0.0 {
            self.config.width as f32 / logical.width
        } else {
            1.0
        }
    }

    fn stats_lines(&self) -> Vec<String> {
        let [x, y, z] = self.camera.position;
        let mut lines = vec![
            format!("{} FPS - {x:.0}, {y:.0}, {z:.0}", self.fps),
            format!(
                "{} chunks - {} visible / {} cached",
                i64::from(self.settings.distance),
                self.visible,
                self.stream().map_or(0, |stream| stream.cache.len())
            ),
        ];
        let controls: &[&str] = if self.touch.is_some() {
            &[]
        } else if self.inventory.open {
            &[
                "Inventory - E or Esc close - wheel scroll",
                "Click or 1-9 puts a block in the hotbar",
            ]
        } else if self.camera.flying {
            &[
                "Flying - /fly off walks - Ctrl fast",
                "WASD move - Space/Shift up/down",
            ]
        } else {
            &[
                "Walking - /fly on flies - Space jump",
                "WASD move - Ctrl sprint - Shift slow - swim with Space",
            ]
        };
        lines.extend(controls.iter().map(|line| line.to_string()));
        if self.touch.is_none() && self.window.pointer_locked() {
            lines.push("LMB break - RMB place - MMB pick - E blocks".to_string());
            lines.push("1-9/wheel select - T chat - Esc menu".to_string());
        }
        lines
    }

    fn aim(&mut self) {
        let direction = self.camera.direction();
        let eye = self.view_camera().position;
        if let Some(world) = &mut self.world {
            world.interaction.aim(eye, direction);
        }
    }

    // The block at a position, when the chunk around it is loaded.
    fn block(&self, position: [i64; 3]) -> Option<u8> {
        self.world.as_ref()?.interaction.block(position)
    }

    fn click_block(&mut self, button: MouseButton) {
        self.aim();
        if button != MouseButton::Middle {
            self.swing = Some(0.0);
        }
        let Some(world) = &mut self.world else {
            return;
        };
        let Some(hit) = world.interaction.target else {
            return;
        };
        if button == MouseButton::Middle {
            if let Some(material) = world::item_material(hit.material) {
                let mut hotbar = self.hotbar;
                hotbar.pick(material);
                self.set_hotbar(hotbar);
            }
            return;
        }
        let edit = match button {
            MouseButton::Left => hit.break_edit(),
            MouseButton::Right => {
                hit.place_edit(self.hotbar.material(), self.camera.position, |position| {
                    world.interaction.block(position)
                })
            }
            _ => None,
        };
        let Some(edit) = edit else {
            return;
        };
        // The edit applies right away, the server rebuilds the chunks around it.
        world.server.send(protocol::Request::Edit(edit));
        world.interaction.apply_edit(edit);
        let broken = button == MouseButton::Left;
        let material = if broken { hit.material } else { edit.material };
        self.sounds
            .block(&self.camera, edit.position, material, broken);
        if broken {
            world.particles.burst(edit.position, material);
        }
    }

    // Draws block icons with the preview pipeline, the instance index is the material.
    fn draw_icons(
        &self,
        pass: &mut bwebgpu::RenderPass<'_>,
        icons: &[inventory::Icon],
        scale: f32,
    ) {
        let (width, height) = (self.config.width as f32, self.config.height as f32);
        pass.set_pipeline(&self.preview_pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        for icon in icons {
            let [x, y, size] = [icon.x, icon.y, icon.size].map(|value| (value * scale).round());
            // Viewports must stay inside the target, scissors clip to the inventory grid.
            if size < 1.0 || x < 0.0 || y < 0.0 || x + size > width || y + size > height {
                continue;
            }
            let (clip_x, clip_y, clip_width, clip_height) = icon.clip;
            let left = x.max(clip_x * scale);
            let top = y.max(clip_y * scale);
            let right = (x + size).min((clip_x + clip_width) * scale);
            let bottom = (y + size).min((clip_y + clip_height) * scale);
            if right <= left || bottom <= top {
                continue;
            }
            pass.set_viewport(x, y, size, size, 0.0, 1.0);
            pass.set_scissor_rect(
                left as u32,
                top as u32,
                (right - left) as u32,
                (bottom - top) as u32,
            );
            let material = u32::from(icon.material);
            pass.draw(0..3, material..material + 1);
        }
    }

    fn render(&mut self) {
        self.receive();
        if let Some(discovery) = &self.discovery {
            self.menu.update_servers(discovery.receive());
        }
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32();
        self.last_frame = now;
        if let Some(touch) = &mut self.touch {
            self.camera.input.stick = touch.stick();
            let actions = touch.update(now);
            self.touch_actions(actions);
        }
        if self.world.is_none() {
            self.camera.yaw =
                (self.camera.yaw + dt * BACKGROUND_TURN_SPEED) % std::f32::consts::TAU;
        }
        if let Some(world) = &mut self.world {
            world
                .interaction
                .prefetch(&world.server, self.camera.chunk(), |coord| {
                    self.chunks.contains_key(&coord)
                });
        }
        // Paused, in the inventory or chatting, the player keeps falling.
        self.tick_player(dt);
        self.swing = self
            .swing
            .map(|progress| progress + dt / SWING_DURATION)
            .filter(|&progress| progress < 1.0);
        if let Some(world) = &mut self.world {
            world.others.animate(dt);
            let interaction = &world.interaction;
            world
                .particles
                .update(dt, |position| interaction.block(position));
        }
        let (width, height) = self.surface.size();
        if width == 0 || height == 0 {
            return;
        }
        self.update_chunks();
        if self.playing() {
            self.aim();
        }
        let (frame, suboptimal) = match self.surface.get_current_texture() {
            bwebgpu::CurrentSurfaceTexture::Success(frame) => (frame, false),
            bwebgpu::CurrentSurfaceTexture::Suboptimal(frame) => (frame, true),
            bwebgpu::CurrentSurfaceTexture::Outdated | bwebgpu::CurrentSurfaceTexture::Lost => {
                self.resize();
                return;
            }
            bwebgpu::CurrentSurfaceTexture::Timeout | bwebgpu::CurrentSurfaceTexture::Occluded => {
                return;
            }
            bwebgpu::CurrentSurfaceTexture::Validation => panic!("Surface validation failed"),
        };
        let camera = self.camera_view();
        let eye_material = self.block(self.camera.position.map(|v| v.floor() as i64));
        let uniforms = camera.bytes(
            self.started.elapsed().as_secs_f32() % 1024.0,
            if self.night { 0.035 } else { 1.0 },
            eye_material,
        );
        self.queue.write_buffer(&self.uniforms, 0, &uniforms);
        let target = self
            .world
            .as_ref()
            .and_then(|world| world.interaction.target);
        if let Some(hit) = target {
            let position = [
                (hit.position[0] - self.gpu_center.0 * 16) as f32,
                hit.position[1] as f32,
                (hit.position[2] - self.gpu_center.1 * 16) as f32,
            ];
            self.queue
                .write_buffer(&self.selection_buffer, 0, &offset_bytes(position)[..12]);
        }
        let scale = self.scale();
        if self.settings.show_stats {
            let lines = self.stats_lines();
            self.stats
                .update(&self.hud, &self.device, &self.queue, scale, lines);
        }
        let inventory_layout = self.inventory_layout();
        if self.inventory.open {
            self.inventory.hover(&inventory_layout, self.pointer);
        }
        let painter = hud::Painter {
            hud: &self.hud,
            device: &self.device,
            queue: &self.queue,
            scale,
        };
        self.inventory
            .prepare(&painter, &inventory_layout, &self.hotbar);
        let menu_layout = self.menu_layout();
        let menu_open = self.menu_open();
        if menu_open {
            self.menu
                .prepare(&painter, &menu_layout, self.pointer, self.settings);
        }
        if !menu_open {
            self.chat.prepare(&painter);
        }
        let others = self.world.as_ref().map(|world| &world.others);
        let name_tags = others.map_or_else(Vec::new, |others| {
            others.name_tags(&camera, self.gpu_center)
        });
        self.name_tags
            .prepare(&painter, name_tags.iter().map(|&(name, _)| name));
        let player_instances = others.map_or_else(Vec::new, |others| {
            others.instances(self.gpu_center, textures::CHARACTER_LAYER)
        });
        if !player_instances.is_empty() {
            self.queue
                .write_buffer(&self.player_buffer, 0, &player_instances);
        }
        let particle_instances = self.world.as_ref().map_or_else(Vec::new, |world| {
            world.particles.instances(self.gpu_center, |material| {
                // Plants show their plane, blocks their side.
                textures::layer(material, if world::is_sprite(material) { 6 } else { 0 })
            })
        });
        if !particle_instances.is_empty() {
            self.queue
                .write_buffer(&self.particle_buffer, 0, &particle_instances);
        }
        let icons = self.inventory.icons(&inventory_layout, &self.hotbar);
        let touch_layout = self.touch_layout();
        if let Some(touch) = &mut self.touch {
            touch.prepare(
                &self.hud,
                &self.device,
                &self.queue,
                scale,
                self.camera.flying,
            );
        }
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let mut visible = 0;
        {
            let mut pass = encoder.begin_render_pass(&bwebgpu::RenderPassDescriptor {
                color_attachments: &[Some(bwebgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: bwebgpu::Operations {
                        load: bwebgpu::LoadOp::Clear(bwebgpu::Color::BLACK),
                        store: bwebgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(bwebgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(bwebgpu::Operations {
                        load: bwebgpu::LoadOp::Clear(1.0),
                        store: bwebgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_pipeline(&self.terrain_pipeline);
            let desired = self.stream().map_or(&[][..], |stream| &stream.desired);
            // Nearest chunks first improve early depth rejection of farther surfaces.
            for coord in desired {
                if let Some(chunk) = self.chunks.get(coord)
                    && camera.visible(offset(*coord, self.gpu_center))
                {
                    pass.set_vertex_buffer(0, chunk.faces.slice(..));
                    for (section, ranges) in chunk.sections.iter().enumerate() {
                        if !ranges[0].is_empty()
                            && camera.section_visible(offset(*coord, self.gpu_center), section)
                        {
                            pass.draw(0..6, ranges[0].clone());
                            visible += 1;
                        }
                    }
                }
            }
            if !player_instances.is_empty() {
                pass.set_pipeline(&self.player_pipeline);
                pass.set_vertex_buffer(0, self.player_buffer.slice(..));
                let count = (player_instances.len() as u64 / avatars::INSTANCE_BYTES) as u32;
                pass.draw(0..36, 0..count);
            }
            if !particle_instances.is_empty() {
                pass.set_pipeline(&self.player_pipeline);
                pass.set_vertex_buffer(0, self.particle_buffer.slice(..));
                let count = (particle_instances.len() as u64 / avatars::INSTANCE_BYTES) as u32;
                pass.draw(0..36, 0..count);
            }
            // Draw the sky only into pixels not covered by terrain.
            pass.set_pipeline(&self.sky_pipeline);
            pass.draw(0..3, 0..1);
            // Draw translucent blocks after opaque geometry and sky, far to near.
            pass.set_pipeline(&self.liquid_pipeline);
            for coord in desired.iter().rev() {
                if let Some(chunk) = self.chunks.get(coord) {
                    pass.set_vertex_buffer(0, chunk.faces.slice(..));
                    for (section, ranges) in chunk.sections.iter().enumerate().rev() {
                        if !ranges[1].is_empty()
                            && camera.section_visible(offset(*coord, self.gpu_center), section)
                        {
                            pass.draw(0..6, ranges[1].clone());
                        }
                    }
                }
            }
            let held = self.hotbar.material();
            if self.world.is_some() && held != 0 {
                let swing = (self.swing.unwrap_or(0.0) * 127.0) as u32;
                let instance = u32::from(held) | swing << 7;
                pass.set_pipeline(&self.held_pipeline);
                let boxes = world::boxes(held).len().max(1) as u32;
                pass.draw(
                    0..if world::is_sprite(held) {
                        6
                    } else {
                        36 * boxes
                    },
                    instance..instance + 1,
                );
            }
            if self.playing() {
                if target.is_some() {
                    pass.set_pipeline(&self.selection_pipeline);
                    pass.set_vertex_buffer(0, self.selection_buffer.slice(..));
                    pass.draw(0..72, 0..1);
                }
                pass.set_scissor_rect(
                    (self.config.width / 2).saturating_sub(12),
                    (self.config.height / 2).saturating_sub(12),
                    self.config.width.min(24),
                    self.config.height.min(24),
                );
                pass.set_pipeline(&self.crosshair_pipeline);
                pass.draw(0..3, 0..1);
            }
            let target = (self.config.width, self.config.height);
            if menu_open {
                self.menu
                    .draw(&self.hud, &mut pass, &menu_layout, scale, target);
            } else {
                self.name_tags
                    .draw(&self.hud, &mut pass, &name_tags, target);
                if let Some(touch) = &self.touch {
                    touch.draw(&self.hud, &mut pass, &touch_layout, target);
                }
                if self.settings.show_stats {
                    self.stats
                        .draw(&self.hud, &mut pass, self.safe_area, target);
                }
                // The block icons go on top of the frames of their slots.
                let (hotbar_icons, panel_icons) = icons.split_at(inventory::SLOTS);
                self.inventory
                    .draw_hotbar(&self.hud, &mut pass, &inventory_layout, scale, target);
                self.draw_icons(&mut pass, hotbar_icons, scale);
                self.inventory
                    .draw_panel(&self.hud, &mut pass, &inventory_layout, scale, target);
                self.draw_icons(&mut pass, panel_icons, scale);
            }
            if !menu_open {
                self.chat.draw(&self.hud, &mut pass, self.safe_area, target);
            }
        }
        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
        if suboptimal {
            self.resize();
        }
        self.frames += 1;
        let elapsed = self.last_stats.elapsed().as_secs_f32();
        if elapsed >= 1.0 {
            self.fps = (self.frames as f32 / elapsed).round() as u32;
            self.visible = visible;
            self.frames = 0;
            self.last_stats = Instant::now();
        }
    }
}

// Looks over the default world from above its spawn.
fn background_camera() -> camera::Camera {
    camera::Camera {
        pitch: -0.2,
        ..camera::Camera::default()
    }
}

fn main() {
    let event_loop = EventLoopBuilder::new()
        .app_id("nl", "bplaat", "PlaatCraft")
        .build();
    let worlds = worlds::Worlds::new().expect("Open worlds folder");
    let window = WindowBuilder::new()
        .title(TITLE)
        .size(LogicalSize::new(1280.0, 800.0))
        .theme(Theme::Dark)
        .background_color(0x000000)
        .center()
        .remember_window_state()
        .build();
    let window_id = window.id();
    let attachment = window.attach_content().expect("Attach graphics surface");
    let mut game = Game::new(window, attachment, worlds);
    game.window.request_animation_frame();

    event_loop.run(move |event| match event {
        Event::Window(id, WindowEvent::CloseRequested(request)) if id == window_id => {
            // The Android back gesture goes back a menu page, then pauses to the menu first,
            // like a game console. Desktop close and quit requests always quit.
            if game.touch.is_some() {
                if game.menu_open()
                    && !matches!(game.menu.page, menu::Page::Title | menu::Page::Pause)
                {
                    request.prevent_default();
                    game.menu_back();
                    game.save_settings();
                    return;
                }
                if !game.paused {
                    request.prevent_default();
                    game.pause();
                    return;
                }
            }
            game.window.exit_pointer_lock();
            // Closing the world waits for the server to save it.
            game.leave_world();
        }
        Event::Window(id, WindowEvent::Blur) if id == window_id => {
            game.menu.touch_id = None;
            game.end_menu_drag();
            game.window.exit_pointer_lock();
            // Touch screens pause when the app goes to the background.
            if game.touch.is_some() {
                game.pause();
            }
        }
        Event::Window(
            id,
            event @ (WindowEvent::TouchStart(_)
            | WindowEvent::TouchMove(_)
            | WindowEvent::TouchEnd(_)
            | WindowEvent::TouchCancel(_)),
        ) if id == window_id => {
            game.touch_event(&event);
        }
        Event::Window(id, WindowEvent::SafeAreaChanged(safe_area)) if id == window_id => {
            game.safe_area = safe_area;
        }
        Event::Window(id, WindowEvent::PointerLockChange) if id == window_id => {
            game.pointer_lock_changed();
        }
        Event::Window(id, WindowEvent::MouseDown(event)) if id == window_id => {
            game.modifiers = event.modifiers;
            if game.inventory.open {
                let layout = game.inventory_layout();
                if event.button == MouseButton::Left
                    && let Some(action) =
                        game.inventory
                            .click(&layout, event.position, game.hotbar.selected)
                {
                    game.inventory_action(action);
                }
            } else if game.window.pointer_locked() {
                game.click_block(event.button);
            } else if event.button == MouseButton::Left && game.menu_open() {
                game.menu_press(event.position);
            }
        }
        Event::Window(id, WindowEvent::MouseUp(event)) if id == window_id => {
            if event.button == MouseButton::Left {
                game.end_menu_drag();
            }
        }

        Event::Window(
            id,
            WindowEvent::MouseMove {
                position,
                movement,
                modifiers,
            },
        ) if id == window_id => {
            game.modifiers = modifiers;
            game.pointer = Some(position);
            if game.menu_open() {
                game.menu_drag(position);
            }
            if game.window.pointer_locked() {
                game.camera
                    .look(f64::from(movement.x), f64::from(movement.y));
            }
        }
        Event::Window(id, WindowEvent::MouseLeave) if id == window_id => {
            game.pointer = None;
            game.end_menu_drag();
        }
        Event::Window(id, WindowEvent::Wheel(delta, modifiers)) if id == window_id => {
            game.modifiers = modifiers;
            let steps = game.scroll.steps(delta, Instant::now());
            if game.inventory.open {
                let layout = game.inventory_layout();
                game.inventory.scroll_steps(&layout, steps);
            } else if game.playing() {
                let mut hotbar = game.hotbar;
                hotbar.cycle(steps);
                game.set_hotbar(hotbar);
            } else if game.menu_open() {
                game.menu.scroll_steps(steps);
            }
        }
        Event::Window(id, WindowEvent::KeyDown(event)) if id == window_id => {
            game.modifiers = event.modifiers;
            if game.chat.typing() {
                game.chat_key(&event);
                return;
            }
            if game.menu_open() && game.menu_key(&event) {
                return;
            }
            if let Some(key) = event.code {
                if game.playing()
                    && !event.repeat
                    && matches!(key, KeyCode::KeyT | KeyCode::Enter | KeyCode::Slash)
                {
                    game.keys.clear();
                    game.open_chat(if key == KeyCode::Slash { "/" } else { "" });
                } else if !event.repeat
                    && (key == KeyCode::KeyE && (game.playing() || game.inventory.open)
                        || key == KeyCode::Escape && game.inventory.open)
                {
                    game.toggle_inventory();
                } else if key == KeyCode::Escape {
                    game.window.exit_pointer_lock();
                } else if let Some(slot) = DIGITS.iter().position(|&digit| digit == key)
                    && (game.playing() || game.inventory.open)
                {
                    game.number_key(slot);
                } else if game.playing() {
                    game.keys.insert(key);
                }
            }
        }
        Event::Window(id, WindowEvent::KeyUp(event)) if id == window_id => {
            game.modifiers = event.modifiers;
            if let Some(key) = event.code {
                game.keys.remove(&key);
            }
        }
        Event::Window(id, WindowEvent::Resize(_)) if id == window_id => {
            game.resize();
            game.window.request_animation_frame();
        }
        Event::Window(id, WindowEvent::RedrawRequested) if id == window_id => {
            // Request the next frame first, so a frame that takes most of a display
            // refresh does not miss the next one and wait for the refresh after it.
            game.window.request_animation_frame();
            game.render();
        }
        _ => {}
    });
}
