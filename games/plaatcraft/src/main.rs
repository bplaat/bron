/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! PlaatCraft, a first-person voxel sandbox rendered with bwebgpu and bwindow.
#![allow(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use bwindow::{
    Event, EventLoopBuilder, KeyCode, LogicalPoint, LogicalSize, Modifiers, MouseButton, Theme,
    Window, WindowBuilder, WindowEvent,
};

mod biome;
mod camera;
mod database;
mod interaction;
mod lighting;
mod perlin;
mod scroll;
mod stream;
mod textures;
mod world;

const TITLE: &str = "PlaatCraft v0.2";

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
    night: bool,
    stream: stream::Stream,
    chunks: HashMap<world::Coord, GpuChunk>,
    gpu_center: world::Coord,
    started: Instant,
    last_frame: Instant,
    frames: u32,
    last_stats: Instant,
    database: database::Database,
    window_state: database::WindowState,
    last_save: Instant,
}

impl Game {
    fn new(
        window: Window,
        attachment: bwindow::WindowAttachment,
        database: database::Database,
        saved_window: Option<database::WindowState>,
    ) -> Self {
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
        let origins = device.create_buffer(&bwebgpu::BufferDescriptor {
            label: Some("Chunk origin table"),
            size: (stream::RESIDENT_CAPACITY * 16) as u64,
            usage: bwebgpu::BufferUsages::STORAGE | bwebgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
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
        let selection_buffer = device.create_buffer(&bwebgpu::BufferDescriptor {
            label: Some("Target block"),
            size: 12,
            usage: bwebgpu::BufferUsages::VERTEX | bwebgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let interaction =
            interaction::Interaction::new(database.selected().expect("Restore selected block"));
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
        let position = window.position();
        let size = window.size();
        let window_state = saved_window.unwrap_or(database::WindowState {
            x: position.x.round() as i32,
            y: position.y.round() as i32,
            width: size.width.round().max(1.0) as u32,
            height: size.height.round().max(1.0) as u32,
            maximized: false,
        });
        let stream = stream::Stream::open(database::PATH);
        let depth = Self::create_depth(&device, &config);
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
            night,
            stream,
            chunks: HashMap::new(),
            gpu_center: (0, 0),
            started: Instant::now(),
            last_frame: Instant::now(),
            frames: 0,
            last_stats: Instant::now(),
            database,
            window_state,
            last_save: Instant::now(),
        }
    }

    fn pointer_lock_changed(&mut self) {
        self.camera.keys.clear();
        self.camera.modifiers = Modifiers::NONE;
        self.interaction.scroll.reset();
    }

    fn camera_view(&self) -> camera::View {
        self.camera.view(
            self.config.width,
            self.config.height,
            stream::RENDER_DISTANCE,
        )
    }

    fn update_chunks(&mut self) {
        let center = self.camera.chunk();
        let view = self.camera_view();
        self.stream
            .update(center, |coord| view.visible(offset(coord, center)));
        let mut origins_changed = self.gpu_center != center;
        if origins_changed {
            self.dirty
                .retain(|&coord| stream::in_range(coord, center, stream::RENDER_DISTANCE));
            self.chunks.retain(|&coord, chunk| {
                let keep = stream::in_range(coord, center, stream::RENDER_DISTANCE);
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
                    let extent =
                        u32::from_ne_bytes(input[4..].try_into().expect("Packed quad extent"))
                            | slot << 18;
                    output[4..].copy_from_slice(&extent.to_ne_bytes());
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
            if uploaded >= 32 || upload_started.elapsed() >= Duration::from_millis(1) {
                break;
            }
        }
        if origins_changed {
            self.queue
                .write_buffer(&self.origins, 0, &self.origin_bytes);
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
            self.interaction.select(&self.database, hit.material);
            return;
        }
        let (position, material) = match button {
            MouseButton::Left => (hit.position, 0),
            MouseButton::Right => {
                let Some(position) = hit.adjacent else {
                    return;
                };
                if interaction::overlaps_player(position, self.camera.position) {
                    return;
                }
                let block = self.interaction.block(&self.database, position, |coord| {
                    self.chunks.contains_key(&coord)
                });
                if !matches!(block, Some(0 | world::WATER)) {
                    return;
                }
                (position, self.interaction.selected)
            }
            _ => return,
        };
        if !(3..world::HEIGHT as i64).contains(&position[1]) || hit.material == 23 && material == 0
        {
            return;
        }
        match self.database.edit(world::Edit { position, material }) {
            Ok(revision) => {
                let center = (position[0].div_euclid(16), position[2].div_euclid(16));
                for coord in self.stream.invalidate(center, revision) {
                    if self.chunks.contains_key(&coord) {
                        self.dirty.insert(coord);
                    }
                }
                self.interaction
                    .apply_edit(world::Edit { position, material });
            }
            Err(error) => eprintln!("Could not save block edit: {error}"),
        }
    }

    fn update_window_state(&mut self) {
        let position = self.window.position();
        self.window_state.x = position.x.round() as i32;
        self.window_state.y = position.y.round() as i32;
        let size = self.window.size();
        if size.width >= 100.0 && size.height >= 100.0 {
            self.window_state.width = size.width.round() as u32;
            self.window_state.height = size.height.round() as u32;
        }
        self.window_state.maximized = false;
    }

    fn save_session(&mut self) {
        self.update_window_state();
        if let Err(error) = self
            .database
            .save_session(&self.camera, self.night, self.window_state)
        {
            eprintln!("Could not save session: {error}");
        }
        self.last_save = Instant::now();
    }

    fn render(&mut self) {
        if self.last_save.elapsed() >= Duration::from_secs(5) {
            self.save_session();
        }
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32();
        self.last_frame = now;
        if self.window.pointer_locked() {
            self.camera.update(dt, |position| {
                if position[1] < 0 {
                    return true;
                }
                !matches!(
                    self.interaction
                        .block(&self.database, position, |coord| self
                            .chunks
                            .contains_key(&coord)),
                    Some(0 | 12..=15 | world::WATER | world::LAVA)
                )
            });
        }
        let (width, height) = self.surface.size();
        if width == 0 || height == 0 {
            return;
        }
        self.update_chunks();
        if self.window.pointer_locked() {
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
        let mut uniforms = camera.bytes(
            self.started.elapsed().as_secs_f32() % 1024.0,
            if self.night { 0.035 } else { 1.0 },
            eye_material,
        );
        uniforms[92..96].copy_from_slice(&f32::from(self.interaction.selected).to_ne_bytes());
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
            if self.window.pointer_locked() {
                if self.interaction.target.is_some() {
                    pass.set_pipeline(&self.selection_pipeline);
                    pass.set_vertex_buffer(0, self.selection_buffer.slice(..));
                    pass.draw(0..72, 0..1);
                }
                pass.set_pipeline(&self.preview_pipeline);
                let logical = self.window.size();
                let scale = if logical.width > 0.0 {
                    f64::from(self.config.width) / f64::from(logical.width)
                } else {
                    1.0
                };
                let shortest = self.config.width.min(self.config.height);
                let margin = (16.0 * scale)
                    .round()
                    .min(f64::from(shortest.saturating_sub(1) / 2))
                    as u32;
                let size = (96.0 * scale)
                    .round()
                    .min(f64::from(shortest - margin * 2))
                    .max(1.0) as u32;
                let y = self.config.height - margin - size;
                pass.set_viewport(margin as f32, y as f32, size as f32, size as f32, 0.0, 1.0);
                pass.set_scissor_rect(margin, y, size, size);
                pass.draw(0..3, 0..1);
                pass.set_viewport(
                    0.0,
                    0.0,
                    self.config.width as f32,
                    self.config.height as f32,
                    0.0,
                    1.0,
                );
                pass.set_scissor_rect(
                    (self.config.width / 2).saturating_sub(12),
                    (self.config.height / 2).saturating_sub(12),
                    self.config.width.min(24),
                    self.config.height.min(24),
                );
                pass.set_pipeline(&self.crosshair_pipeline);
                pass.draw(0..3, 0..1);
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
            let fps = self.frames as f32 / elapsed;
            let cached = self.stream.cache.len();
            let [x, y, z] = self.camera.position;
            let hint = if self.window.pointer_locked() {
                if self.camera.flying {
                    "Flying | F walk | WASD | Space/Shift up/down | Ctrl fast | LMB break | RMB place | MMB pick | Wheel select | N day/night | Esc release"
                } else {
                    "Walking | F fly | WASD | Space jump | Ctrl sprint | Shift slow | LMB break | RMB place | MMB pick | Wheel select | N day/night | Esc release"
                }
            } else {
                "Click to play"
            };
            self.window.set_title(format!("{TITLE} - {} chunks - {fps:.0} FPS - {visible} visible / {cached} cached - {x:.0}, {y:.0}, {z:.0} - {} - {hint}", stream::RENDER_DISTANCE, self.interaction.name()));
            self.frames = 0;
            self.last_stats = Instant::now();
        }
    }
}

fn main() {
    let event_loop = EventLoopBuilder::new()
        .app_id("nl", "bplaat", "PlaatCraft")
        .build();
    let database = database::Database::open(database::PATH).expect("Open world database");
    let mut saved_window = database.load_window().expect("Restore window state");
    if let Some(state) = &mut saved_window {
        // A disconnected monitor should not leave the restored window offscreen.
        let visible = event_loop.available_monitors().iter().any(|monitor| {
            let position = monitor.position();
            let size = monitor.size();
            state.x as f32 + state.width as f32 > position.x + 64.0
                && state.y as f32 + state.height as f32 > position.y + 64.0
                && (state.x as f32) < position.x + size.width - 64.0
                && (state.y as f32) < position.y + size.height - 64.0
        });
        if !visible {
            let monitor = event_loop.primary_monitor();
            let position = monitor.position();
            let size = monitor.size();
            state.x = (position.x + 32.0).round() as i32;
            state.y = (position.y + 32.0).round() as i32;
            state.width = state.width.min((size.width - 64.0).max(100.0) as u32);
            state.height = state.height.min((size.height - 64.0).max(100.0) as u32);
        }
    }

    let mut builder = WindowBuilder::new()
        .title(format!("{TITLE} - click to play"))
        .size(LogicalSize::new(1280.0, 800.0))
        .theme(Theme::Dark)
        .background_color(0x000000)
        .center();
    if let Some(state) = saved_window {
        builder = builder
            .position(LogicalPoint::new(state.x as f32, state.y as f32))
            .size(LogicalSize::new(state.width as f32, state.height as f32));
    }
    let window = builder.build();
    let window_id = window.id();
    let attachment = window.attach_content().expect("Attach graphics surface");
    let mut game = Game::new(window, attachment, database, saved_window);
    game.window.request_animation_frame();

    event_loop.run(move |event| match event {
        Event::Window(id, WindowEvent::CloseRequested(_)) if id == window_id => {
            game.window.exit_pointer_lock();
            game.save_session();
        }
        Event::Window(id, WindowEvent::Blur) if id == window_id => {
            game.window.exit_pointer_lock();
        }
        Event::Window(id, WindowEvent::PointerLockChange) if id == window_id => {
            game.pointer_lock_changed();
        }
        Event::Window(id, WindowEvent::MouseDown(event)) if id == window_id => {
            game.camera.modifiers = event.modifiers;
            if game.window.pointer_locked() {
                game.click_block(event.button);
            } else if event.button == MouseButton::Left
                && let Err(error) = game.window.request_pointer_lock()
            {
                eprintln!("Could not lock pointer: {error}");
            }
        }
        Event::Window(
            id,
            WindowEvent::MouseMove {
                movement,
                modifiers,
                ..
            },
        ) if id == window_id => {
            game.camera.modifiers = modifiers;
            if game.window.pointer_locked() {
                game.camera
                    .look(f64::from(movement.x), f64::from(movement.y));
            }
        }
        Event::Window(id, WindowEvent::Wheel(delta, modifiers))
            if id == window_id && game.window.pointer_locked() =>
        {
            game.camera.modifiers = modifiers;
            let steps = game.interaction.scroll.steps(delta, Instant::now());
            game.interaction.scroll(&game.database, steps);
        }
        Event::Window(id, WindowEvent::KeyDown(event)) if id == window_id => {
            game.camera.modifiers = event.modifiers;
            if let Some(key) = event.code {
                if key == KeyCode::KeyF && game.window.pointer_locked() && !event.repeat {
                    game.camera.set_flying(!game.camera.flying);
                    game.save_session();
                } else if key == KeyCode::KeyN && !event.repeat {
                    game.night = !game.night;
                } else if key == KeyCode::Escape {
                    game.window.exit_pointer_lock();
                } else if game.window.pointer_locked() {
                    game.camera.keys.insert(key);
                }
            }
        }
        Event::Window(id, WindowEvent::KeyUp(event)) if id == window_id => {
            game.camera.modifiers = event.modifiers;
            if let Some(key) = event.code {
                game.camera.keys.remove(&key);
            }
        }
        Event::Window(id, WindowEvent::Move(_)) if id == window_id => {
            game.update_window_state();
        }
        Event::Window(id, WindowEvent::Resize(_)) if id == window_id => {
            game.update_window_state();
            game.resize();
            game.window.request_animation_frame();
        }
        Event::Window(id, WindowEvent::RedrawRequested) if id == window_id => {
            game.render();
            game.window.request_animation_frame();
        }
        _ => {}
    });
}
