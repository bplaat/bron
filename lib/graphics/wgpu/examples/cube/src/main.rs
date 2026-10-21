/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Draws a rotating textured cube with wgpu and bwindow.

use std::time::{Duration, Instant};

use bcanvas::{Color, OffscreenCanvas, TextAlign, TextBaseline};
use bwindow::{
    Event, EventLoop, KeyCode, LogicalInsets, LogicalSize, Theme, WindowBuilder, WindowEvent,
};

const BACKGROUND_COLOR: u32 = 0x000000;

struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    depth: wgpu::TextureView,
    pipeline: wgpu::RenderPipeline,
    overlay_pipeline: wgpu::RenderPipeline,
    overlay_group: wgpu::BindGroup,
    overlay_texture: wgpu::Texture,
    overlay_canvas: OffscreenCanvas,
    displayed_fps: u32,
    // The safe area top and right insets in pixels, which the FPS overlay stays inside.
    overlay_inset: (f32, f32),
    uniforms: wgpu::Buffer,
    vertices: wgpu::Buffer,
    group: wgpu::BindGroup,
    vertex_count: u32,
}

impl Gpu {
    fn new(attachment: bwindow::WindowAttachment) -> Self {
        // Configure the surface and device
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(attachment).expect("Create surface");
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
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
            .find(wgpu::TextureFormat::is_srgb)
            .unwrap_or(config.format);
        surface.configure(&device, &config);
        let depth = Self::depth(&device, &config);

        // Create the cube pipeline
        let entries = [
            (
                0,
                wgpu::ShaderStages::VERTEX,
                wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
            ),
            (
                1,
                wgpu::ShaderStages::FRAGMENT,
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
            ),
            (
                2,
                wgpu::ShaderStages::FRAGMENT,
                wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            ),
        ]
        .map(|(binding, visibility, ty)| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty,
            count: None,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
        });
        let shader = device.create_shader_module(wgpu::include_wgsl!("mesh.wgsl"));
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 32,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x3,
                            offset: 0,
                            shader_location: 0,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x3,
                            offset: 12,
                            shader_location: 1,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 24,
                            shader_location: 2,
                        },
                    ],
                })],
            },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        // Create the FPS overlay pipeline and resources
        let overlay_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let overlay_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[Some(&overlay_layout)],
            });
        let overlay_shader = device.create_shader_module(wgpu::include_wgsl!("overlay.wgsl"));
        let overlay_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&overlay_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &overlay_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &overlay_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let overlay_canvas = OffscreenCanvas::new(120, 36);
        let overlay_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: overlay_canvas.width(),
                height: overlay_canvas.height(),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let overlay_view = overlay_texture.create_view(&Default::default());
        let overlay_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let overlay_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &overlay_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&overlay_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&overlay_sampler),
                },
            ],
        });

        // Upload the cube mesh and texture
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
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

    fn depth(device: &wgpu::Device, config: &wgpu::SurfaceConfiguration) -> wgpu::TextureView {
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: config.width,
                    height: config.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
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
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        uniforms: &wgpu::Buffer,
        mesh: &Mesh,
    ) -> (wgpu::Buffer, wgpu::BindGroup) {
        // Upload vertex data
        let bytes = mesh.bytes();
        let vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: bytes.len() as u64,
            usage: wgpu::BufferUsages::VERTEX,
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
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: texture_width,
                height: texture_height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: Default::default(),
                aspect: wgpu::TextureAspect::All,
            },
            mesh.texture.pixels(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(texture_width * 4),
                rows_per_image: Some(texture_height),
            },
            wgpu::Extent3d {
                width: texture_width,
                height: texture_height,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&Default::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        // Bind the uniforms and material
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        (vertices, group)
    }

    fn render(&mut self, elapsed: f32, fps: u32) -> bool {
        let (frame, reconfigure) = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => (frame, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => (frame, true),
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.resize();
                return true;
            }
            wgpu::CurrentSurfaceTexture::Timeout => return true,
            // Do not spin while hidden. A native paint/resize event resumes drawing.
            wgpu::CurrentSurfaceTexture::Occluded => return false,
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
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::from_rgb8(
                            BACKGROUND_COLOR,
                            self.config.format,
                        )),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.),
                        store: wgpu::StoreOp::Discard,
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
            wgpu::TexelCopyTextureInfo {
                texture: &self.overlay_texture,
                mip_level: 0,
                origin: Default::default(),
                aspect: wgpu::TextureAspect::All,
            },
            self.overlay_canvas.pixels(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
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
        .title("WGPU Cube")
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
