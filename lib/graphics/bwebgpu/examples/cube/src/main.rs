/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Draws a rotating textured cube with bwebgpu and bwindow.

use std::time::{Duration, Instant};

use bcanvas::{Color, OffscreenCanvas, TextAlign, TextBaseline};
use bwindow::{
    Event, EventLoop, KeyCode, LogicalInsets, LogicalSize, Theme, WindowBuilder, WindowEvent,
};

const BACKGROUND_COLOR: u32 = 0x000000;

struct Gpu {
    surface: bwebgpu::Surface<'static>,
    device: bwebgpu::Device,
    queue: bwebgpu::Queue,
    config: bwebgpu::SurfaceConfiguration,
    depth: bwebgpu::TextureView,
    pipeline: bwebgpu::RenderPipeline,
    overlay_pipeline: bwebgpu::RenderPipeline,
    overlay_group: bwebgpu::BindGroup,
    overlay_texture: bwebgpu::Texture,
    overlay_canvas: OffscreenCanvas,
    displayed_fps: u32,
    // The safe area top and right insets in pixels, which the FPS overlay stays inside.
    overlay_inset: (f32, f32),
    uniforms: bwebgpu::Buffer,
    vertices: bwebgpu::Buffer,
    group: bwebgpu::BindGroup,
    vertex_count: u32,
}

impl Gpu {
    fn new(attachment: bwindow::WindowAttachment) -> Self {
        // Configure the surface and device
        let instance = bwebgpu::Instance::default();
        let surface = instance.create_surface(attachment).expect("Create surface");
        let adapter = instance
            .request_adapter(&bwebgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
            })
            .expect("Find graphics adapter");
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
            .find(bwebgpu::TextureFormat::is_srgb)
            .unwrap_or(config.format);
        surface.configure(&device, &config);
        let depth = Self::depth(&device, &config);

        // Create the cube pipeline
        let entries = [
            (
                0,
                bwebgpu::ShaderStages::VERTEX,
                bwebgpu::BindingType::Buffer {
                    ty: bwebgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
            ),
            (
                1,
                bwebgpu::ShaderStages::FRAGMENT,
                bwebgpu::BindingType::Texture {
                    sample_type: bwebgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: bwebgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
            ),
            (
                2,
                bwebgpu::ShaderStages::FRAGMENT,
                bwebgpu::BindingType::Sampler(bwebgpu::SamplerBindingType::Filtering),
            ),
        ]
        .map(|(binding, visibility, ty)| bwebgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty,
            count: None,
        });
        let layout = device.create_bind_group_layout(&bwebgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&bwebgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
        });
        let shader = device.create_shader_module(bwebgpu::include_wgsl!("mesh.wgsl"));
        let pipeline = device.create_render_pipeline(&bwebgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            vertex: bwebgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(bwebgpu::VertexBufferLayout {
                    array_stride: 32,
                    step_mode: bwebgpu::VertexStepMode::Vertex,
                    attributes: &[
                        bwebgpu::VertexAttribute {
                            format: bwebgpu::VertexFormat::Float32x3,
                            offset: 0,
                            shader_location: 0,
                        },
                        bwebgpu::VertexAttribute {
                            format: bwebgpu::VertexFormat::Float32x3,
                            offset: 12,
                            shader_location: 1,
                        },
                        bwebgpu::VertexAttribute {
                            format: bwebgpu::VertexFormat::Float32x2,
                            offset: 24,
                            shader_location: 2,
                        },
                    ],
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
                    format: config.format,
                    blend: None,
                    write_mask: bwebgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        // Create the FPS overlay pipeline and resources
        let overlay_layout = device.create_bind_group_layout(&bwebgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                bwebgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: bwebgpu::ShaderStages::FRAGMENT,
                    ty: bwebgpu::BindingType::Texture {
                        sample_type: bwebgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: bwebgpu::TextureViewDimension::D2,
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
            ],
        });
        let overlay_pipeline_layout =
            device.create_pipeline_layout(&bwebgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[Some(&overlay_layout)],
            });
        let overlay_shader = device.create_shader_module(bwebgpu::include_wgsl!("overlay.wgsl"));
        let overlay_pipeline = device.create_render_pipeline(&bwebgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&overlay_pipeline_layout),
            vertex: bwebgpu::VertexState {
                module: &overlay_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: Some(bwebgpu::DepthStencilState {
                format: bwebgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(false),
                depth_compare: Some(bwebgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(bwebgpu::FragmentState {
                module: &overlay_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(bwebgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(bwebgpu::BlendState::ALPHA_BLENDING),
                    write_mask: bwebgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let overlay_canvas = OffscreenCanvas::new(120, 36);
        let overlay_texture = device.create_texture(&bwebgpu::TextureDescriptor {
            label: None,
            size: bwebgpu::Extent3d {
                width: overlay_canvas.width(),
                height: overlay_canvas.height(),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: bwebgpu::TextureDimension::D2,
            format: bwebgpu::TextureFormat::Rgba8UnormSrgb,
            usage: bwebgpu::TextureUsages::TEXTURE_BINDING | bwebgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let overlay_view = overlay_texture.create_view(&Default::default());
        let overlay_sampler = device.create_sampler(&bwebgpu::SamplerDescriptor {
            mag_filter: bwebgpu::FilterMode::Linear,
            min_filter: bwebgpu::FilterMode::Linear,
            ..Default::default()
        });
        let overlay_group = device.create_bind_group(&bwebgpu::BindGroupDescriptor {
            label: None,
            layout: &overlay_layout,
            entries: &[
                bwebgpu::BindGroupEntry {
                    binding: 1,
                    resource: bwebgpu::BindingResource::TextureView(&overlay_view),
                },
                bwebgpu::BindGroupEntry {
                    binding: 2,
                    resource: bwebgpu::BindingResource::Sampler(&overlay_sampler),
                },
            ],
        });

        // Upload the cube mesh and texture
        let uniforms = device.create_buffer(&bwebgpu::BufferDescriptor {
            label: None,
            size: 32,
            usage: bwebgpu::BufferUsages::UNIFORM | bwebgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mesh = cube();
        let (vertices, group) = Self::upload(&device, &queue, &layout, &uniforms, &mesh);

        Self {
            surface,
            device,
            queue,
            config,
            depth,
            pipeline,
            overlay_pipeline,
            overlay_group,
            overlay_texture,
            overlay_canvas,
            displayed_fps: u32::MAX,
            overlay_inset: (0.0, 0.0),
            uniforms,
            vertices,
            group,
            vertex_count: mesh.vertices.len() as u32,
        }
    }

    fn depth(
        device: &bwebgpu::Device,
        config: &bwebgpu::SurfaceConfiguration,
    ) -> bwebgpu::TextureView {
        device
            .create_texture(&bwebgpu::TextureDescriptor {
                label: None,
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

    fn set_safe_area(&mut self, safe_area: LogicalInsets, window_size: LogicalSize) {
        let scale = self.surface.size().0 as f32 / window_size.width.max(1.0);
        self.overlay_inset = (safe_area.top * scale, safe_area.right * scale);
    }

    fn resize(&mut self) {
        let (width, height) = self.surface.size();
        if width == 0 || height == 0 {
            return;
        }

        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.depth = Self::depth(&self.device, &self.config);
    }

    fn upload(
        device: &bwebgpu::Device,
        queue: &bwebgpu::Queue,
        layout: &bwebgpu::BindGroupLayout,
        uniforms: &bwebgpu::Buffer,
        mesh: &Mesh,
    ) -> (bwebgpu::Buffer, bwebgpu::BindGroup) {
        // Upload vertex data
        let bytes = mesh.bytes();
        let vertices = device.create_buffer(&bwebgpu::BufferDescriptor {
            label: None,
            size: bytes.len() as u64,
            usage: bwebgpu::BufferUsages::VERTEX,
            mapped_at_creation: true,
        });
        vertices
            .slice(..)
            .get_mapped_range_mut()
            .expect("Mapped vertex buffer")
            .copy_from_slice(&bytes);
        vertices.unmap();

        // Upload the material texture
        let texture_width = mesh.texture.width();
        let texture_height = mesh.texture.height();
        let texture = device.create_texture(&bwebgpu::TextureDescriptor {
            label: None,
            size: bwebgpu::Extent3d {
                width: texture_width,
                height: texture_height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: bwebgpu::TextureDimension::D2,
            format: bwebgpu::TextureFormat::Rgba8UnormSrgb,
            usage: bwebgpu::TextureUsages::TEXTURE_BINDING | bwebgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            bwebgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: Default::default(),
                aspect: bwebgpu::TextureAspect::All,
            },
            mesh.texture.pixels(),
            bwebgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(texture_width * 4),
                rows_per_image: Some(texture_height),
            },
            bwebgpu::Extent3d {
                width: texture_width,
                height: texture_height,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&Default::default());
        let sampler = device.create_sampler(&bwebgpu::SamplerDescriptor {
            address_mode_u: bwebgpu::AddressMode::Repeat,
            address_mode_v: bwebgpu::AddressMode::Repeat,
            mag_filter: bwebgpu::FilterMode::Linear,
            min_filter: bwebgpu::FilterMode::Linear,
            ..Default::default()
        });

        // Bind the uniforms and material
        let group = device.create_bind_group(&bwebgpu::BindGroupDescriptor {
            label: None,
            layout,
            entries: &[
                bwebgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                bwebgpu::BindGroupEntry {
                    binding: 1,
                    resource: bwebgpu::BindingResource::TextureView(&view),
                },
                bwebgpu::BindGroupEntry {
                    binding: 2,
                    resource: bwebgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        (vertices, group)
    }

    fn render(&mut self, elapsed: f32, fps: u32) -> bool {
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
                eprintln!("Surface lost; close and restart the example");
                return false;
            }
        };

        // Update per-frame resources
        let aspect = self.config.width as f32 / self.config.height as f32;
        // Pull back in portrait so the cube fits the narrower horizontal field of view.
        let distance = 4.5 / aspect.min(1.0);
        let values = [
            0.4,
            0.25,
            distance,
            aspect,
            0.,
            0.,
            elapsed * 0.55,
            elapsed * 0.35,
        ];
        let mut bytes = [0; 32];
        for (index, value) in values.into_iter().enumerate() {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_ne_bytes());
        }
        self.queue.write_buffer(&self.uniforms, 0, &bytes);
        self.update_fps(fps);

        // Draw the cube and FPS overlay
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&bwebgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(bwebgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: bwebgpu::Operations {
                        load: bwebgpu::LoadOp::Clear(bwebgpu::Color::from_rgb8(
                            BACKGROUND_COLOR,
                            self.config.format,
                        )),
                        store: bwebgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(bwebgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(bwebgpu::Operations {
                        load: bwebgpu::LoadOp::Clear(1.),
                        store: bwebgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.group, &[]);
            pass.set_vertex_buffer(0, self.vertices.slice(..));
            pass.draw(0..self.vertex_count, 0..1);

            let overlay_width = self.overlay_canvas.width() as f32;
            let overlay_height = self.overlay_canvas.height() as f32;
            let (top, right) = self.overlay_inset;
            pass.set_viewport(
                (self.config.width as f32 - overlay_width - right - 8.0).max(0.0),
                top + 8.0,
                overlay_width,
                overlay_height,
                0.0,
                1.0,
            );
            pass.set_pipeline(&self.overlay_pipeline);
            pass.set_bind_group(0, &self.overlay_group, &[]);
            pass.draw(0..3, 0..1);
        }

        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
        if reconfigure {
            self.resize();
        }

        true
    }

    fn update_fps(&mut self, fps: u32) {
        if self.displayed_fps == fps {
            return;
        }

        self.displayed_fps = fps;
        self.overlay_canvas.draw(|ctx| {
            ctx.clear_rect(0.0, 0.0, ctx.width(), ctx.height());
            ctx.set_fill_style(Color::rgb(255, 255, 255));
            ctx.set_font("sans-serif", 18.0);
            ctx.set_text_align(TextAlign::Right);
            ctx.set_text_baseline(TextBaseline::Top);
            ctx.fill_text(format!("{fps} FPS"), ctx.width() - 8.0, 8.0);
        });

        let width = self.overlay_canvas.width();
        let height = self.overlay_canvas.height();
        let bytes_per_row = self.overlay_canvas.bytes_per_row();
        self.queue.write_texture(
            bwebgpu::TexelCopyTextureInfo {
                texture: &self.overlay_texture,
                mip_level: 0,
                origin: Default::default(),
                aspect: bwebgpu::TextureAspect::All,
            },
            self.overlay_canvas.pixels(),
            bwebgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(height),
            },
            bwebgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }
}

#[derive(Default)]
struct FrameRate {
    sample_start: Duration,
    frames: u32,
    value: u32,
}

impl FrameRate {
    fn sample(&mut self, timestamp: Duration) -> u32 {
        self.frames += 1;
        let elapsed = timestamp.saturating_sub(self.sample_start);
        if elapsed >= Duration::from_secs(1) {
            self.value = (f64::from(self.frames) / elapsed.as_secs_f64()).round() as u32;
            self.sample_start = timestamp;
            self.frames = 0;
        }
        self.value
    }
}

struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
}

struct Mesh {
    vertices: Vec<Vertex>,
    texture: image::Image,
}

impl Mesh {
    fn bytes(&self) -> Vec<u8> {
        self.vertices
            .iter()
            .flat_map(|vertex| {
                [
                    vertex.position[0],
                    vertex.position[1],
                    vertex.position[2],
                    vertex.normal[0],
                    vertex.normal[1],
                    vertex.normal[2],
                    vertex.uv[0],
                    vertex.uv[1],
                ]
                .into_iter()
                .flat_map(f32::to_ne_bytes)
            })
            .collect()
    }
}

fn cube() -> Mesh {
    let mut vertices = Vec::new();
    for (normal, corners) in [
        (
            [0., 0., 1.],
            [[-1., -1., 1.], [1., -1., 1.], [1., 1., 1.], [-1., 1., 1.]],
        ),
        (
            [0., 0., -1.],
            [
                [1., -1., -1.],
                [-1., -1., -1.],
                [-1., 1., -1.],
                [1., 1., -1.],
            ],
        ),
        (
            [1., 0., 0.],
            [[1., -1., 1.], [1., -1., -1.], [1., 1., -1.], [1., 1., 1.]],
        ),
        (
            [-1., 0., 0.],
            [
                [-1., -1., -1.],
                [-1., -1., 1.],
                [-1., 1., 1.],
                [-1., 1., -1.],
            ],
        ),
        (
            [0., 1., 0.],
            [[-1., 1., 1.], [1., 1., 1.], [1., 1., -1.], [-1., 1., -1.]],
        ),
        (
            [0., -1., 0.],
            [
                [-1., -1., -1.],
                [1., -1., -1.],
                [1., -1., 1.],
                [-1., -1., 1.],
            ],
        ),
    ] {
        for index in [0, 1, 2, 0, 2, 3] {
            vertices.push(Vertex {
                position: corners[index],
                normal,
                uv: [[0., 1.], [1., 1.], [1., 0.], [0., 0.]][index],
            });
        }
    }

    Mesh {
        vertices,
        texture: image::decode(include_bytes!("../assets/crate.jpg"))
            .expect("Decode bundled crate JPEG"),
    }
}

fn main() {
    let event_loop = EventLoop::new();
    let mut window = WindowBuilder::new()
        .title("Bassie WebGPU Cube")
        .size(LogicalSize::new(960.0, 720.0))
        .theme(Theme::Dark)
        .background_color(BACKGROUND_COLOR)
        .center()
        .build();
    let mut gpu = Gpu::new(window.attach_content().expect("Attach graphics surface"));
    gpu.set_safe_area(window.safe_area(), window.size());
    let started = Instant::now();
    let mut frame_rate = FrameRate::default();
    let window_id = window.id();

    window.request_animation_frame();
    event_loop.run(move |event| match event {
        Event::Window(id, WindowEvent::RedrawRequested) if id == window_id => {
            let elapsed = started.elapsed();
            if gpu.render(elapsed.as_secs_f32(), frame_rate.sample(elapsed)) {
                window.request_animation_frame();
            }
        }
        Event::Window(id, WindowEvent::Resize(size)) if id == window_id => {
            gpu.resize();
            gpu.set_safe_area(window.safe_area(), size);
            window.request_animation_frame();
        }
        Event::Window(id, WindowEvent::SafeAreaChanged(safe_area)) if id == window_id => {
            gpu.set_safe_area(safe_area, window.size());
            window.request_animation_frame();
        }
        Event::Window(id, WindowEvent::KeyDown(event))
            if id == window_id && event.code == Some(KeyCode::Escape) =>
        {
            window.close();
        }
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_rate_updates_after_one_second() {
        let mut frame_rate = FrameRate::default();
        for frame in 1..=60 {
            frame_rate.sample(Duration::from_secs_f64(f64::from(frame) / 60.0));
        }
        assert_eq!(frame_rate.value, 60);
    }
}
