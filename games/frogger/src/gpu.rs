/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Native GPU surface and vertex rendering.

const BACKGROUND_COLOR: u32 = 0xc7dfb5;

struct VertexBuffer {
    buffer: bwebgpu::Buffer,
    capacity: usize,
}

impl VertexBuffer {
    fn new(device: &bwebgpu::Device, capacity: usize) -> Self {
        Self {
            buffer: device.create_buffer(&bwebgpu::BufferDescriptor {
                label: Some("scene vertices"),
                size: capacity as u64,
                usage: bwebgpu::BufferUsages::VERTEX | bwebgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            capacity,
        }
    }

    fn upload(&mut self, device: &bwebgpu::Device, queue: &bwebgpu::Queue, vertices: &[f32]) {
        let bytes: Vec<u8> = vertices
            .iter()
            .flat_map(|value| value.to_ne_bytes())
            .collect();
        if bytes.len() > self.capacity {
            *self = Self::new(device, bytes.len().next_power_of_two());
        }
        queue.write_buffer(&self.buffer, 0, &bytes);
    }
}

pub(super) struct Gpu {
    surface: bwebgpu::Surface<'static>,
    device: bwebgpu::Device,
    queue: bwebgpu::Queue,
    config: bwebgpu::SurfaceConfiguration,
    pipeline: bwebgpu::RenderPipeline,
    vertices: VertexBuffer,
    depth: bwebgpu::TextureView,
}

impl Gpu {
    pub(super) fn new(attachment: bwindow::WindowAttachment) -> Self {
        // Configure the surface and device
        let instance = bwebgpu::Instance::default();
        let surface = instance.create_surface(attachment).expect("Create surface");
        let adapter = instance
            .request_adapter(&bwebgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
            })
            .expect("Find adapter");
        let (device, queue) = adapter
            .request_device(&Default::default())
            .expect("Create device");

        let (width, height) = surface.size();
        let mut config = surface
            .get_default_config(&adapter, width.max(1), height.max(1))
            .expect("Surface format");
        config.format = surface
            .get_capabilities(&adapter)
            .formats
            .into_iter()
            .find(|format| *format == bwebgpu::TextureFormat::Bgra8Unorm)
            .unwrap_or(config.format);
        surface.configure(&device, &config);

        // Create the voxel and UI pipeline
        let vertices = VertexBuffer::new(&device, 8 * 1024 * 1024);
        let depth = Self::depth(&device, config.width, config.height);
        let pipeline = Self::pipeline(&device, config.format);

        Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            vertices,
            depth,
        }
    }

    fn pipeline(
        device: &bwebgpu::Device,
        format: bwebgpu::TextureFormat,
    ) -> bwebgpu::RenderPipeline {
        let shader = device.create_shader_module(bwebgpu::include_wgsl!("scene.wgsl"));
        device.create_render_pipeline(&bwebgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: bwebgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(bwebgpu::VertexBufferLayout {
                    array_stride: 28,
                    step_mode: bwebgpu::VertexStepMode::Vertex,
                    attributes: &bwebgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4],
                })],
            },
            primitive: Default::default(),
            depth_stencil: Some(bwebgpu::DepthStencilState {
                format: bwebgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(bwebgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(bwebgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(bwebgpu::ColorTargetState {
                    format,
                    blend: Some(bwebgpu::BlendState::ALPHA_BLENDING),
                    write_mask: bwebgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
    }

    pub(super) fn resize(&mut self) {
        let (width, height) = self.surface.size();
        if width == 0 || height == 0 {
            return;
        }

        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.depth = Self::depth(&self.device, self.config.width, self.config.height);
    }

    fn depth(device: &bwebgpu::Device, width: u32, height: u32) -> bwebgpu::TextureView {
        device
            .create_texture(&bwebgpu::TextureDescriptor {
                label: Some("scene depth"),
                size: bwebgpu::Extent3d {
                    width,
                    height,
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

    pub(super) fn render(&mut self, vertices: &[f32]) -> bool {
        let (frame, reconfigure) = match self.surface.get_current_texture() {
            bwebgpu::CurrentSurfaceTexture::Success(frame) => (frame, false),
            bwebgpu::CurrentSurfaceTexture::Suboptimal(frame) => (frame, true),
            bwebgpu::CurrentSurfaceTexture::Outdated => {
                self.resize();
                return true;
            }
            bwebgpu::CurrentSurfaceTexture::Timeout => return true,
            // Do not spin while hidden. A native paint/resize event resumes drawing.
            bwebgpu::CurrentSurfaceTexture::Occluded => return false,
            _ => {
                eprintln!("Graphics surface lost; restart Frogger");
                return false;
            }
        };

        let view = frame.texture.create_view(&Default::default());
        Self::draw(
            &self.device,
            &self.queue,
            &self.pipeline,
            &mut self.vertices,
            &self.depth,
            &view,
            vertices,
        );
        self.queue.present(frame);
        if reconfigure {
            self.resize();
        }

        true
    }
    fn draw(
        device: &bwebgpu::Device,
        queue: &bwebgpu::Queue,
        pipeline: &bwebgpu::RenderPipeline,
        buffer: &mut VertexBuffer,
        depth: &bwebgpu::TextureView,
        view: &bwebgpu::TextureView,
        vertices: &[f32],
    ) {
        buffer.upload(device, queue, vertices);
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&bwebgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(bwebgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: bwebgpu::Operations {
                        load: bwebgpu::LoadOp::Clear(bwebgpu::Color::from_rgb8(
                            BACKGROUND_COLOR,
                            bwebgpu::TextureFormat::Rgba8Unorm,
                        )),
                        store: bwebgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(bwebgpu::RenderPassDepthStencilAttachment {
                    view: depth,
                    depth_ops: Some(bwebgpu::Operations {
                        load: bwebgpu::LoadOp::Clear(1.0),
                        store: bwebgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
            });
            pass.set_pipeline(pipeline);
            pass.set_vertex_buffer(0, buffer.buffer.slice(..));
            pass.draw(0..(vertices.len() / 7) as u32, 0..1);
        }

        queue.submit([encoder.finish()]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{Game, State};
    use crate::scene::Scene;

    #[test]
    #[ignore = "Requires a native graphics adapter; writes previews to the temporary directory"]
    fn gpu_previews() {
        let instance = bwebgpu::Instance::default();
        let adapter = instance
            .request_adapter(&Default::default())
            .expect("Graphics adapter");
        let (device, queue) = adapter
            .request_device(&Default::default())
            .expect("GPU device");
        let pipeline = Gpu::pipeline(&device, bwebgpu::TextureFormat::Rgba8Unorm);
        let mut buffer = VertexBuffer::new(&device, 1024);
        for (name, width, height, state, mobile) in [
            ("ready", 640, 820, State::Ready, false),
            ("playing", 640, 820, State::Playing, false),
            ("mobile", 320, 480, State::Paused, true),
            ("onboarding", 390, 900, State::Ready, true),
            ("tall", 390, 900, State::Playing, true),
            ("mobile-edge", 390, 900, State::Playing, true),
            ("mobile-back", 900, 390, State::Playing, true),
            ("resized", 320, 4000, State::Playing, false),
            ("wide", 4000, 320, State::Playing, false),
            ("landscape", 1000, 600, State::Dead, false),
            ("trains", 640, 820, State::Playing, false),
            ("trains-wide", 1800, 600, State::Playing, false),
            ("trains-left", 640, 820, State::Playing, false),
            ("trains-right", 640, 820, State::Playing, false),
            ("logs", 640, 820, State::Playing, false),
            ("hop", 640, 820, State::Playing, false),
            ("crash", 640, 820, State::Dying, false),
            ("splash", 640, 820, State::Dying, false),
        ] {
            let mut game = Game::new(42, 1234);
            game.state = state;
            game.reason = "HIT BY TRAFFIC";
            if name == "mobile-edge" {
                game.x = 6.0;
            }
            if name == "mobile-back" {
                game.camera = 4.0;
                game.score = 4;
            }
            if name == "logs" {
                game.row = (20..200)
                    .find(|&row| {
                        (row..row + 3).all(|row| {
                            crate::game::Lane::new(row, game.seed).terrain
                                == crate::game::Terrain::Water
                        })
                    })
                    .expect("river");
                game.camera = game.row as f32;
                game.score = game.row;
                game.x = crate::game::Lane::new(game.row, game.seed)
                    .objects(game.time)
                    .into_iter()
                    .find(|x| x.abs() < 4.0)
                    .expect("log");
            }
            if name == "hop" {
                game.move_by(1, 0);
                game.update(0.08);
            }
            if matches!(name, "trains-left" | "trains-right") {
                let direction = if name == "trains-left" { -1.0 } else { 1.0 };
                game.seed = (0..100)
                    .find(|&seed| {
                        (44..52).any(|row| {
                            let lane = crate::game::Lane::new(row, seed);
                            lane.terrain == crate::game::Terrain::Rail
                                && lane.speed.signum() == direction
                        })
                    })
                    .expect("train direction");
            }
            if name.starts_with("trains") {
                let row = (44..52)
                    .find(|&r| {
                        crate::game::Lane::new(r, game.seed).terrain == crate::game::Terrain::Rail
                    })
                    .expect("railway");
                game.camera = (row - 2) as f32;
                game.row = row - 2;
                game.score = row - 2;
                let lane = crate::game::Lane::new(row, game.seed);
                game.time = 14.2 - lane.phase.rem_euclid(8.0);
            }
            if matches!(name, "crash" | "splash") {
                game.death_time = 0.3;
                game.facing = (1, 0);
                if name == "splash" {
                    game.reason = "FELL IN THE WATER";
                    game.row = (20..28)
                        .find(|&r| {
                            crate::game::Lane::new(r, game.seed).terrain
                                == crate::game::Terrain::Water
                        })
                        .expect("river");
                } else {
                    game.row = (4..12)
                        .find(|&r| {
                            crate::game::Lane::new(r, game.seed).terrain
                                == crate::game::Terrain::Road
                        })
                        .expect("road");
                }
                game.camera = game.row as f32;
            }
            let scene = Scene::new(
                width as f32,
                height as f32,
                &game,
                if mobile {
                    bwindow::LogicalInsets {
                        top: 32.0,
                        bottom: 24.0,
                        ..Default::default()
                    }
                } else {
                    Default::default()
                },
                mobile,
            );
            assert!(scene.vertices.iter().all(|v| v.is_finite()));
            let depth = Gpu::depth(&device, width, height);
            let target = device.create_bitmap_target(width, height);
            Gpu::draw(
                &device,
                &queue,
                &pipeline,
                &mut buffer,
                &depth,
                &target.create_view(),
                &scene.vertices,
            );
            assert!(buffer.capacity >= size_of_val(scene.vertices.as_slice()));
            let bitmap = target.read(&queue).expect("Read rendered scene");
            let path = std::env::temp_dir().join(format!("frogger-{name}.png"));
            std::fs::write(
                &path,
                image::encode(&bitmap, image::Format::Png).expect("Encode preview"),
            )
            .expect("Write preview");
            eprintln!("Preview: {}", path.display());
        }
    }
}
