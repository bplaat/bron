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
    Event, EventLoopBuilder, KeyCode, LogicalInsets, LogicalPoint, LogicalSize, Modifiers,
    MouseButton, Theme, Window, WindowBuilder, WindowEvent,
};
use plaatcraft_engine::{camera, world};

mod controls;
mod database;
mod hud;
mod interaction;
mod inventory;
mod menu;
mod scroll;
mod settings;
mod sound;
mod stream;
mod textures;

const TITLE: &str = "PlaatCraft";
// Keep terrain uploads short so rendering and input take priority over loading.
const CHUNK_UPLOAD_BUDGET: Duration = Duration::from_millis(1);
const MAX_CHUNK_UPLOADS: usize = 32;
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
    let buffers = [Some(bwebgpu::VertexBufferLayout {
        array_stride: if selection {
            12
        } else {
            world::QUAD_BYTES as u64
        },
        step_mode: bwebgpu::VertexStepMode::Instance,
        attributes: if selection {
            &bwebgpu::vertex_attr_array![0 => Float32x3]
        } else {
            &bwebgpu::vertex_attr_array![0 => Uint32x2]
        },
    })];
    device.create_render_pipeline(&bwebgpu::RenderPipelineDescriptor {
        label: Some(fragment),
        layout: Some(layout),
        vertex: bwebgpu::VertexState {
            module: &shader,
            entry_point: Some(if selection {
                "vs_selection"
            } else if liquid {
                "vs_liquid"
            } else if terrain {
                "vs_terrain"
            } else {
                "vs_screen"
            }),
            compilation_options: Default::default(),
            buffers: if terrain || selection { &buffers } else { &[] },
        },
        primitive: bwebgpu::PrimitiveState {
            cull_mode: if terrain && !liquid {
                Some(bwebgpu::Face::Back)
            } else {
                None
            },
        },
        depth_stencil: Some(bwebgpu::DepthStencilState {
            format: bwebgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(terrain && !liquid),
            depth_compare: Some(if matches!(fragment, "fs_crosshair" | "fs_preview") {
                bwebgpu::CompareFunction::Always
            } else {
                bwebgpu::CompareFunction::LessEqual
            }),
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
    hud: hud::Hud,
    stats: hud::Stats,
    // On-screen controls on touch screens, where the game is always playing.
    touch: Option<controls::TouchControls>,
    safe_area: LogicalInsets,
    hotbar: inventory::Hotbar,
    inventory: inventory::Inventory,
    scroll: scroll::Scroll,
    // The mouse position over the window, for hovering the inventory.
    pointer: Option<LogicalPoint>,
    menu: menu::Menu,
    settings: settings::Settings,
    preferences: settings::Store,
    sounds: sound::Sounds,
    gpu_radius: i64,
    // Touch screens have no pointer lock, so they track the menu themselves.
    paused: bool,
    selection_buffer: bwebgpu::Buffer,
    interaction: interaction::Interaction,
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
    stream: stream::Stream,
    chunks: HashMap<world::Coord, GpuChunk>,
    gpu_center: world::Coord,
    started: Instant,
    last_frame: Instant,
    frames: u32,
    fps: u32,
    visible: u32,
    last_stats: Instant,
    database: database::Database,
    last_save: Instant,
}

impl Game {
    fn new(
        window: Window,
        attachment: bwindow::WindowAttachment,
        database: database::Database,
    ) -> Self {
        let preferences = settings::Store::new().expect("Open preferences");
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
        let hud = hud::Hud::new(&device, config.format);
        let stats = hud::Stats::new(&hud, &device, scale);
        let selection_buffer = device.create_buffer(&bwebgpu::BufferDescriptor {
            label: Some("Target block"),
            size: 12,
            usage: bwebgpu::BufferUsages::VERTEX | bwebgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let interaction = interaction::Interaction::with_prefetch(database::path());
        let hotbar = database.hotbar().expect("Restore hotbar");
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
        world::set_seed(database.seed().expect("Read world seed"));
        let mut camera = camera::Camera::default();
        let night = database
            .restore_state(&mut camera)
            .expect("Restore game state");
        let stream = stream::Stream::open(database::path());
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
            hud,
            stats,
            touch: cfg!(any(target_os = "ios", target_os = "android"))
                .then(controls::TouchControls::default),
            safe_area,
            hotbar,
            inventory: inventory::Inventory::default(),
            scroll: scroll::Scroll::default(),
            pointer: None,
            menu: menu::Menu::default(),
            sounds: sound::Sounds::new(&settings),
            settings,
            preferences,
            gpu_radius: 0,
            paused: true,
            selection_buffer,
            interaction,
            dirty: HashSet::new(),
            bind_group,
            origins,
            origin_bytes: vec![0; stream::RESIDENT_CAPACITY * 16],
            free_slots: (0..stream::RESIDENT_CAPACITY as u32).rev().collect(),
            uniforms,
            depth,
            camera,
            keys: HashSet::new(),
            modifiers: Modifiers::NONE,
            descend: false,
            night,
            stream,
            chunks: HashMap::new(),
            gpu_center: (0, 0),
            started: Instant::now(),
            last_frame: Instant::now(),
            frames: 0,
            fps: 0,
            visible: 0,
            last_stats: Instant::now(),
            database,
            last_save: Instant::now(),
        }
    }

    // Whether input controls the player: a locked pointer or touch controls.
    fn playing(&self) -> bool {
        !self.inventory.open
            && (self.window.pointer_locked() || self.touch.is_some() && !self.paused)
    }

    // The main menu shows at startup and while paused: on desktops while the pointer is
    // released and the inventory is closed.
    fn menu_open(&self) -> bool {
        if self.touch.is_some() {
            self.paused
        } else {
            !self.window.pointer_locked() && !self.inventory.open
        }
    }

    fn menu_layout(&self) -> menu::Layout {
        menu::Layout::new(
            self.window.size(),
            self.safe_area,
            self.touch.is_none(),
            self.menu.page,
        )
    }

    fn save_settings(&mut self) {
        self.sounds.apply(&self.settings);
        if let Err(error) = self.preferences.save(self.settings) {
            eprintln!("Could not save settings: {error}");
        }
    }

    fn menu_press(&mut self, position: LogicalPoint) {
        use menu::Action;
        let action = self.menu_layout().action_at(position);
        if action.is_some() {
            self.sounds.click();
        }
        match action {
            Some(Action::Play) => self.play(),
            Some(Action::Settings) => self.menu.page = menu::Page::Settings,
            Some(Action::Quit) => {
                self.save_session();
                self.window.close();
            }
            Some(Action::Back) => self.menu.page = menu::Page::Main,
            Some(action @ (Action::Distance | Action::Volume)) => {
                self.menu.dragging = Some(action);
                self.menu_drag(position);
            }
            Some(Action::Stats) => self.settings.show_stats = !self.settings.show_stats,
            Some(Action::Reset) => self.settings = settings::Settings::default(),
            None => {}
        }
        self.save_settings();
    }

    fn menu_drag(&mut self, position: LogicalPoint) {
        let fraction = self.menu_layout().slider_fraction(position);
        match self.menu.dragging {
            Some(menu::Action::Distance) => {
                self.settings.distance = settings::distance_at(fraction);
            }
            Some(menu::Action::Volume) => {
                self.settings.volume = (fraction * f32::from(settings::MAX_VOLUME)).round() as u8;
                self.sounds.apply(&self.settings);
            }
            _ => {}
        }
    }

    // Releases the touch controls and shows the menu on touch screens.
    fn pause(&mut self) {
        self.touch_event(&WindowEvent::Blur);
        self.paused = true;
    }

    fn play(&mut self) {
        self.menu.dragging = None;
        self.menu.touch_id = None;
        self.menu.page = menu::Page::Main;
        if self.touch.is_some() {
            self.paused = false;
        } else if let Err(error) = self.window.request_pointer_lock() {
            eprintln!("Could not lock pointer: {error}");
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
        if let Err(error) = self.database.save_hotbar(&hotbar) {
            eprintln!("Could not save hotbar: {error}");
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
                        self.menu.dragging = None;
                        self.save_settings();
                    }
                }
                _ => {
                    self.menu.dragging = None;
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
                    self.camera.set_flying(!self.camera.flying);
                    self.descend = false;
                    self.save_session();
                }
                controls::Action::Press(controls::Button::Night) => self.night = !self.night,
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

    fn camera_view(&self) -> camera::View {
        self.camera.view(
            self.config.width,
            self.config.height,
            i64::from(self.settings.distance),
        )
    }

    fn update_chunks(&mut self) {
        let center = self.camera.chunk();
        let view = self.camera_view();
        self.stream.radius = i64::from(self.settings.distance);
        self.stream
            .update(center, |coord| view.visible(offset(coord, center)));
        let mut origins_changed =
            self.gpu_center != center || self.gpu_radius != self.stream.radius;
        if origins_changed {
            self.dirty.retain(|&coord| {
                stream::in_range(coord, center, i64::from(self.settings.distance))
            });
            self.chunks.retain(|&coord, chunk| {
                let keep = stream::in_range(coord, center, i64::from(self.settings.distance));
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
            self.gpu_radius = self.stream.radius;
        }
        // A time budget handles variable mesh sizes; the count cap bounds burst work.
        let upload_started = Instant::now();
        let mut uploaded = 0;
        for index in 0..self.stream.loading.len() {
            let coord = self.stream.loading[index];
            if self.chunks.contains_key(&coord) && !self.dirty.contains(&coord) {
                continue;
            }
            let Some(cached) = self.stream.cache.get(&coord) else {
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
                for (index, input) in cached.mesh.as_chunks::<8>().0.iter().enumerate() {
                    let mut output = *input;
                    let quad = world::with_chunk_slot(
                        [
                            u32::from_ne_bytes(
                                input[..4].try_into().expect("Packed quad position"),
                            ),
                            u32::from_ne_bytes(input[4..].try_into().expect("Packed quad extent")),
                        ],
                        slot,
                    );
                    output[..4].copy_from_slice(&quad[0].to_ne_bytes());
                    output[4..].copy_from_slice(&quad[1].to_ne_bytes());
                    mapped
                        .slice(index * 8..index * 8 + 8)
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
            self.stream.mark_uploaded(coord);
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
                self.stream.cache.len()
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
                "Flying - F walk - Ctrl fast",
                "WASD move - Space/Shift up/down",
            ]
        } else {
            &[
                "Walking - F fly - Space jump",
                "WASD move - Ctrl sprint - Shift slow - swim with Space",
            ]
        };
        lines.extend(controls.iter().map(|line| line.to_string()));
        if self.touch.is_none() && self.window.pointer_locked() {
            lines.push("LMB break - RMB place - MMB pick - E blocks".to_string());
            lines.push("1-9/wheel select - N day/night - Esc menu".to_string());
        }
        lines
    }

    fn aim(&mut self) {
        let (sin, cos) = self.camera.yaw.sin_cos();
        let (sp, cp) = self.camera.pitch.sin_cos();
        let direction = [f64::from(sin * cp), f64::from(sp), f64::from(-cos * cp)];
        self.interaction
            .aim(&self.database, self.camera.position, direction, |coord| {
                self.chunks.contains_key(&coord)
            });
    }

    fn click_block(&mut self, button: MouseButton) {
        self.aim();
        let Some(hit) = self.interaction.target else {
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
                    self.interaction.block(&self.database, position, |coord| {
                        self.chunks.contains_key(&coord)
                    })
                })
            }
            _ => None,
        };
        let Some(edit) = edit else {
            return;
        };
        let broken = button == MouseButton::Left;
        match self.database.edit(edit) {
            Ok(revision) => {
                let center = (
                    edit.position[0].div_euclid(16),
                    edit.position[2].div_euclid(16),
                );
                for coord in self.stream.invalidate(center, revision) {
                    if self.chunks.contains_key(&coord) {
                        self.dirty.insert(coord);
                    }
                }
                self.interaction.apply_edit(edit);
                let material = if broken { hit.material } else { edit.material };
                self.sounds
                    .block(&self.camera, edit.position, material, broken);
            }
            Err(error) => eprintln!("Could not save block edit: {error}"),
        }
    }

    fn save_session(&mut self) {
        self.save_settings();
        if let Err(error) = self.database.save_state(&self.camera, self.night) {
            eprintln!("Could not save session: {error}");
        }
        self.last_save = Instant::now();
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
        if self.last_save.elapsed() >= Duration::from_secs(5) {
            self.save_session();
        }
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32();
        self.last_frame = now;
        if let Some(touch) = &mut self.touch {
            self.camera.input.stick = touch.stick();
            let actions = touch.update(now);
            self.touch_actions(actions);
        }
        self.interaction.prefetch(self.camera.chunk(), |coord| {
            self.chunks.contains_key(&coord)
        });
        if self.playing() {
            let before = sound::Movement::of(&self.camera);
            self.camera.input = self.input();
            // The feet or waist in water or lava make the player swim.
            let [x, y, z] = self.camera.position;
            self.camera.in_liquid = [0.2, 0.9].into_iter().any(|offset| {
                let position = [
                    x.floor() as i64,
                    (y - 1.7 + offset).floor() as i64,
                    z.floor() as i64,
                ];
                matches!(
                    self.interaction
                        .block(&self.database, position, |coord| self
                            .chunks
                            .contains_key(&coord)),
                    Some(world::WATER | world::LAVA)
                )
            });
            self.camera.update(dt, |position| {
                if position[1] < 0 {
                    return true;
                }
                !matches!(
                    self.interaction
                        .block(&self.database, position, |coord| self
                            .chunks
                            .contains_key(&coord)),
                    Some(material) if world::is_passable(material)
                )
            });
            let ground =
                self.interaction
                    .block(&self.database, sound::ground(&self.camera), |coord| {
                        self.chunks.contains_key(&coord)
                    });
            self.sounds.movement(before, &self.camera, ground);
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
            bwebgpu::CurrentSurfaceTexture::Outdated => {
                self.resize();
                return;
            }
            bwebgpu::CurrentSurfaceTexture::Lost => {
                self.resize();
                return;
            }
            bwebgpu::CurrentSurfaceTexture::Timeout | bwebgpu::CurrentSurfaceTexture::Occluded => {
                return;
            }
            bwebgpu::CurrentSurfaceTexture::Validation => panic!("Surface validation failed"),
        };
        let camera = self.camera_view();
        let eye_material = self.interaction.block(
            &self.database,
            self.camera.position.map(|v| v.floor() as i64),
            |coord| self.chunks.contains_key(&coord),
        );
        let uniforms = camera.bytes(
            self.started.elapsed().as_secs_f32() % 1024.0,
            if self.night { 0.035 } else { 1.0 },
            eye_material,
        );
        self.queue.write_buffer(&self.uniforms, 0, &uniforms);
        if let Some(hit) = self.interaction.target {
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
            // Nearest chunks first improve early depth rejection of farther surfaces.
            for coord in &self.stream.desired {
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
            // Draw the sky only into pixels not covered by terrain.
            pass.set_pipeline(&self.sky_pipeline);
            pass.draw(0..3, 0..1);
            // Draw translucent blocks after opaque geometry and sky, far to near.
            pass.set_pipeline(&self.liquid_pipeline);
            for coord in self.stream.desired.iter().rev() {
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
            if self.playing() {
                if self.interaction.target.is_some() {
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

fn main() {
    let event_loop = EventLoopBuilder::new()
        .app_id("nl", "bplaat", "PlaatCraft")
        .build();
    let database = database::Database::open(database::path()).expect("Open world database");
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
    let mut game = Game::new(window, attachment, database);
    game.window.request_animation_frame();

    event_loop.run(move |event| match event {
        Event::Window(id, WindowEvent::CloseRequested(request)) if id == window_id => {
            // The Android back gesture leaves the settings, then pauses to the menu first, like
            // a game console. Desktop close and quit requests always quit.
            if game.touch.is_some() {
                if game.menu_open() && game.menu.page == menu::Page::Settings {
                    request.prevent_default();
                    game.menu.page = menu::Page::Main;
                    game.menu.dragging = None;
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
            game.save_session();
        }
        Event::Window(id, WindowEvent::Blur) if id == window_id => {
            game.menu.dragging = None;
            game.menu.touch_id = None;
            game.save_settings();
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
            if event.button == MouseButton::Left && game.menu.dragging.take().is_some() {
                game.save_settings();
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
            if game.menu.dragging.take().is_some() {
                game.save_settings();
            }
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
            }
        }
        Event::Window(id, WindowEvent::KeyDown(event)) if id == window_id => {
            game.modifiers = event.modifiers;
            if let Some(key) = event.code {
                if key == KeyCode::KeyF && game.playing() && !event.repeat {
                    game.camera.set_flying(!game.camera.flying);
                    game.save_session();
                } else if key == KeyCode::KeyN && !event.repeat {
                    game.night = !game.night;
                } else if key == KeyCode::Escape
                    && game.menu_open()
                    && game.menu.page == menu::Page::Settings
                {
                    game.menu.page = menu::Page::Main;
                    game.menu.dragging = None;
                    game.save_settings();
                } else if key == KeyCode::Enter
                    && game.menu_open()
                    && game.menu.page == menu::Page::Main
                {
                    game.play();
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
