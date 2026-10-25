/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Renders a 64x64x64 blocks diorama with bwebgpu.

use std::num::NonZeroU64;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bcanvas::{Color, OffscreenCanvas, TextAlign, TextBaseline};
use bwindow::{
    Event, EventLoop, KeyCode, LogicalInsets, LogicalSize, Theme, WindowBuilder, WindowEvent,
};

mod world;

const MATERIAL_SIZE: u32 = 128;
const MATERIAL_MIP_COUNT: u32 = MATERIAL_SIZE.ilog2() + 1;
const BACKGROUND_COLOR: u32 = 0x2e549e;

#[derive(rust_embed::Embed)]
#[folder = "assets"]
struct MaterialAssets;

struct PipelineOptions {
    cull: bool,
    depth_write: bool,
    blend: bool,
    depth_test: bool,
}

struct Gpu {
    surface: bwebgpu::Surface<'static>,
    device: bwebgpu::Device,
    queue: bwebgpu::Queue,
    config: bwebgpu::SurfaceConfiguration,
    depth: bwebgpu::TextureView,
    sky_pipeline: bwebgpu::RenderPipeline,
    opaque_pipeline: bwebgpu::RenderPipeline,
    water_pipeline: bwebgpu::RenderPipeline,
    overlay_pipeline: bwebgpu::RenderPipeline,
    scene_group: bwebgpu::BindGroup,
    overlay_group: bwebgpu::BindGroup,
    overlay_texture: bwebgpu::Texture,
    overlay_canvas: OffscreenCanvas,
    overlay_layout: bwebgpu::BindGroupLayout,
    displayed_fps: u32,
    // The safe area top and right insets in pixels, which the FPS overlay stays inside.
    overlay_inset: (f32, f32),
    uniforms: bwebgpu::Buffer,
    indices: bwebgpu::Buffer,
    opaque_count: u32,
    face_count: u32,
}

impl Gpu {
    fn new(attachment: bwindow::WindowAttachment) -> Self {
        let scale = attachment.scale_factor();
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
        let depth = Self::depth(&device, config.width, config.height);

        let names = material_names();
        let materials = upload_materials(&device, &queue, &names);
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("System time")
            .subsec_nanos();
        let faces = world::World::generate(seed).faces_by_material();
        let (bytes, opaque_count, face_count) = world::face_bytes(&faces);
        println!(
            "Visible faces: {opaque_count} opaque, {} water",
            face_count - opaque_count
        );
        let storage = Self::mapped_buffer(
            &device,
            "voxel faces",
            bwebgpu::BufferUsages::STORAGE,
            &bytes,
        );
        let bytes: Vec<u8> = [0u16, 1, 2, 0, 2, 3]
            .into_iter()
            .flat_map(u16::to_ne_bytes)
            .collect();
        let indices = Self::mapped_buffer(
            &device,
            "face indices",
            bwebgpu::BufferUsages::INDEX,
            &bytes,
        );
        let uniforms = device.create_buffer(&bwebgpu::BufferDescriptor {
            label: Some("scene uniforms"),
            size: 16,
            usage: bwebgpu::BufferUsages::UNIFORM | bwebgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&bwebgpu::SamplerDescriptor {
            address_mode_u: bwebgpu::AddressMode::ClampToEdge,
            address_mode_v: bwebgpu::AddressMode::ClampToEdge,
            mag_filter: bwebgpu::FilterMode::Nearest,
            min_filter: bwebgpu::FilterMode::Linear,
            mipmap_filter: bwebgpu::FilterMode::Linear,
            lod_max_clamp: (MATERIAL_MIP_COUNT - 1) as f32,
            ..Default::default()
        });
        let scene_layout = device.create_bind_group_layout(&bwebgpu::BindGroupLayoutDescriptor {
            label: Some("scene layout"),
            entries: &[
                bwebgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: bwebgpu::ShaderStages::VERTEX,
                    ty: bwebgpu::BindingType::Buffer {
                        ty: bwebgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(16),
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
                        min_binding_size: NonZeroU64::new(32),
                    },
                    count: None,
                },
            ],
        });
        let scene_group = device.create_bind_group(&bwebgpu::BindGroupDescriptor {
            label: Some("scene group"),
            layout: &scene_layout,
            entries: &[
                bwebgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                bwebgpu::BindGroupEntry {
                    binding: 1,
                    resource: bwebgpu::BindingResource::TextureView(&materials),
                },
                bwebgpu::BindGroupEntry {
                    binding: 2,
                    resource: bwebgpu::BindingResource::Sampler(&sampler),
                },
                bwebgpu::BindGroupEntry {
                    binding: 3,
                    resource: storage.as_entire_binding(),
                },
            ],
        });
        let shader = device.create_shader_module(bwebgpu::include_wgsl!("blocks.wgsl"));
        let sky_shader = device.create_shader_module(bwebgpu::include_wgsl!("sky.wgsl"));
        let sky_pipeline = Self::pipeline(
            &device,
            &[],
            &sky_shader,
            config.format,
            PipelineOptions {
                cull: false,
                depth_write: false,
                blend: false,
                depth_test: false,
            },
        );
        let opaque_pipeline = Self::pipeline(
            &device,
            &[&scene_layout],
            &shader,
            config.format,
            PipelineOptions {
                cull: true,
                depth_write: true,
                blend: false,
                depth_test: true,
            },
        );
        let water_pipeline = Self::pipeline(
            &device,
            &[&scene_layout],
            &shader,
            config.format,
            PipelineOptions {
                cull: true,
                depth_write: false,
                blend: true,
                depth_test: true,
            },
        );

        let overlay_layout = device.create_bind_group_layout(&bwebgpu::BindGroupLayoutDescriptor {
            label: Some("overlay layout"),
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
        let overlay_shader = device.create_shader_module(bwebgpu::include_wgsl!("overlay.wgsl"));
        let overlay_pipeline = Self::pipeline(
            &device,
            &[&overlay_layout],
            &overlay_shader,
            config.format,
            PipelineOptions {
                cull: false,
                depth_write: false,
                blend: true,
                depth_test: false,
            },
        );
        let (overlay_canvas, overlay_texture, overlay_group) =
            Self::create_overlay(&device, &overlay_layout, scale);
        Self {
            surface,
            device,
            queue,
            config,
            depth,
            sky_pipeline,
            opaque_pipeline,
            water_pipeline,
            overlay_pipeline,
            scene_group,
            overlay_group,
            overlay_texture,
            overlay_canvas,
            overlay_layout,
            displayed_fps: u32::MAX,
            overlay_inset: (0.0, 0.0),
            uniforms,
            indices,
            opaque_count,
            face_count,
        }
    }

    fn pipeline(
        device: &bwebgpu::Device,
        groups: &[&bwebgpu::BindGroupLayout],
        shader: &bwebgpu::ShaderModule,
        format: bwebgpu::TextureFormat,
        options: PipelineOptions,
    ) -> bwebgpu::RenderPipeline {
        let layout = device.create_pipeline_layout(&bwebgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &groups.iter().map(|group| Some(*group)).collect::<Vec<_>>(),
        });
        device.create_render_pipeline(&bwebgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&layout),
            vertex: bwebgpu::VertexState {
                module: shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: bwebgpu::PrimitiveState {
                cull_mode: if options.cull {
                    Some(bwebgpu::Face::Back)
                } else {
                    None
                },
            },
            depth_stencil: Some(bwebgpu::DepthStencilState {
                format: bwebgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(options.depth_write),
                depth_compare: Some(if options.depth_test {
                    bwebgpu::CompareFunction::Less
                } else {
                    bwebgpu::CompareFunction::Always
                }),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(bwebgpu::FragmentState {
                module: shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(bwebgpu::ColorTargetState {
                    format,
                    blend: if options.blend {
                        Some(bwebgpu::BlendState {
                            alpha: bwebgpu::BlendComponent {
                                src_factor: bwebgpu::BlendFactor::SrcAlpha,
                                ..bwebgpu::BlendComponent::OVER
                            },
                            ..bwebgpu::BlendState::ALPHA_BLENDING
                        })
                    } else {
                        None
                    },
                    write_mask: bwebgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
    }

    fn mapped_buffer(
        device: &bwebgpu::Device,
        label: &'static str,
        usage: bwebgpu::BufferUsages,
        bytes: &[u8],
    ) -> bwebgpu::Buffer {
        let buffer = device.create_buffer(&bwebgpu::BufferDescriptor {
            label: Some(label),
            size: bytes.len() as u64,
            usage,
            mapped_at_creation: true,
        });
        buffer
            .slice(..)
            .get_mapped_range_mut()
            .expect("Mapped buffer")
            .copy_from_slice(bytes);
        buffer.unmap();
        buffer
    }

    fn depth(device: &bwebgpu::Device, width: u32, height: u32) -> bwebgpu::TextureView {
        device
            .create_texture(&bwebgpu::TextureDescriptor {
                label: Some("depth texture"),
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

    fn create_overlay(
        device: &bwebgpu::Device,
        overlay_layout: &bwebgpu::BindGroupLayout,
        scale: f32,
    ) -> (OffscreenCanvas, bwebgpu::Texture, bwebgpu::BindGroup) {
        let overlay_canvas = OffscreenCanvas::with_scale_factor(160, 56, scale);
        let overlay_texture = device.create_texture(&bwebgpu::TextureDescriptor {
            label: Some("stats overlay"),
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
            label: Some("overlay group"),
            layout: overlay_layout,
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
        (overlay_canvas, overlay_texture, overlay_group)
    }

    fn set_safe_area(&mut self, safe_area: LogicalInsets, window_size: LogicalSize) {
        let width = self.surface.size().0;
        if width == 0 || window_size.width <= 0.0 {
            return;
        }
        let scale = width as f32 / window_size.width;
        self.overlay_inset = (safe_area.top * scale, safe_area.right * scale);
        if (self.overlay_canvas.scale_factor() - scale).abs() > 0.001 {
            (
                self.overlay_canvas,
                self.overlay_texture,
                self.overlay_group,
            ) = Self::create_overlay(&self.device, &self.overlay_layout, scale);
            self.displayed_fps = u32::MAX;
        }
    }

    fn resize(&mut self) {
        let (width, height) = self.surface.size();
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.depth = Self::depth(&self.device, width, height);
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
            bwebgpu::CurrentSurfaceTexture::Occluded => return false,
            _ => {
                eprintln!("Surface lost; close and restart the example");
                return false;
            }
        };
        let mut uniform_bytes = [0; 16];
        for (index, value) in [
            elapsed % (std::f32::consts::TAU / 0.16),
            self.config.width as f32 / self.config.height as f32,
            0.0,
            0.0,
        ]
        .into_iter()
        .enumerate()
        {
            uniform_bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_ne_bytes());
        }
        self.queue.write_buffer(&self.uniforms, 0, &uniform_bytes);
        self.update_fps(fps);
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&bwebgpu::RenderPassDescriptor {
                label: Some("blocks scene"),
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
                        load: bwebgpu::LoadOp::Clear(1.0),
                        store: bwebgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
            });
            pass.set_pipeline(&self.sky_pipeline);
            pass.draw(0..3, 0..1);
            pass.set_bind_group(0, &self.scene_group, &[]);
            pass.set_pipeline(&self.opaque_pipeline);
            pass.set_index_buffer(self.indices.slice(..), bwebgpu::IndexFormat::Uint16);
            pass.draw_indexed(0..6, 0, 0..self.opaque_count);
            if self.face_count > self.opaque_count {
                pass.set_pipeline(&self.water_pipeline);
                pass.draw_indexed(0..6, 0, self.opaque_count..self.face_count);
            }
            let padding = 8.0 * self.overlay_canvas.scale_factor();
            let (top, right) = self.overlay_inset;
            let x = self.config.width as f32 - self.overlay_canvas.width() as f32 - right - padding;
            let y = top + padding;
            if x >= 0.0 && y + self.overlay_canvas.height() as f32 <= self.config.height as f32 {
                pass.set_viewport(
                    x,
                    y,
                    self.overlay_canvas.width() as f32,
                    self.overlay_canvas.height() as f32,
                    0.0,
                    1.0,
                );
                pass.set_pipeline(&self.overlay_pipeline);
                pass.set_bind_group(0, &self.overlay_group, &[]);
                pass.draw(0..3, 0..1);
            }
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
            ctx.set_font("sans-serif", 16.0);
            ctx.set_text_align(TextAlign::Right);
            ctx.set_text_baseline(TextBaseline::Top);
            ctx.fill_text(format!("{fps} FPS"), ctx.width() - 8.0, 6.0);
            ctx.fill_text(
                format!("{} faces", self.face_count),
                ctx.width() - 8.0,
                28.0,
            );
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

fn material_names() -> Vec<String> {
    world::Material::ALL
        .into_iter()
        .map(|material| material.file_name().to_owned())
        .collect()
}

fn upload_materials(
    device: &bwebgpu::Device,
    queue: &bwebgpu::Queue,
    material_names: &[String],
) -> bwebgpu::TextureView {
    let layer_count = u32::try_from(material_names.len()).expect("Material layer count");

    // Decode the embedded PNG layers
    let texture = device.create_texture(&bwebgpu::TextureDescriptor {
        label: Some("material texture array"),
        size: bwebgpu::Extent3d {
            width: MATERIAL_SIZE,
            height: MATERIAL_SIZE,
            depth_or_array_layers: layer_count,
        },
        mip_level_count: MATERIAL_MIP_COUNT,
        sample_count: 1,
        dimension: bwebgpu::TextureDimension::D2,
        format: bwebgpu::TextureFormat::Rgba8UnormSrgb,
        usage: bwebgpu::TextureUsages::COPY_DST | bwebgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let mut layers = material_names
        .iter()
        .map(|name| {
            let asset = MaterialAssets::get(name)
                .unwrap_or_else(|| panic!("Missing texture asset: {name}"));
            let image = image::decode(asset.data.as_ref())
                .unwrap_or_else(|error| panic!("Decode {name}: {error}"));
            assert_eq!(image.width(), MATERIAL_SIZE, "Unexpected width for {name}");
            assert_eq!(
                image.height(),
                MATERIAL_SIZE,
                "Unexpected height for {name}"
            );
            image.pixels().to_vec()
        })
        .collect::<Vec<_>>();

    // Upload every generated mip level as one texture-array write
    let mut width = MATERIAL_SIZE;
    let mut height = MATERIAL_SIZE;
    for mip_level in 0..MATERIAL_MIP_COUNT {
        let pixels = layers.iter().flatten().copied().collect::<Vec<_>>();
        queue.write_texture(
            bwebgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level,
                origin: Default::default(),
                aspect: bwebgpu::TextureAspect::All,
            },
            &pixels,
            bwebgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            bwebgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: layer_count,
            },
        );
        if width > 1 || height > 1 {
            layers = layers
                .iter()
                .map(|pixels| downsample_rgba(pixels, width, height))
                .collect();
            width = (width / 2).max(1);
            height = (height / 2).max(1);
        }
    }

    texture.create_view(&bwebgpu::TextureViewDescriptor {
        dimension: Some(bwebgpu::TextureViewDimension::D2Array),
    })
}

fn downsample_rgba(pixels: &[u8], width: u32, height: u32) -> Vec<u8> {
    let next_width = (width / 2).max(1);
    let next_height = (height / 2).max(1);
    let mut output = Vec::with_capacity((next_width * next_height * 4) as usize);
    for y in 0..next_height {
        for x in 0..next_width {
            let mut alpha_sum = 0.0;
            let mut color_sum = [0.0; 3];
            for offset_y in 0..2 {
                for offset_x in 0..2 {
                    let source_x = (x * 2 + offset_x).min(width - 1);
                    let source_y = (y * 2 + offset_y).min(height - 1);
                    let index = ((source_y * width + source_x) * 4) as usize;
                    let alpha = f32::from(pixels[index + 3]) / 255.0;
                    alpha_sum += alpha;
                    for channel in 0..3 {
                        color_sum[channel] += srgb_to_linear(pixels[index + channel]) * alpha;
                    }
                }
            }
            for color in color_sum {
                output.push(linear_to_srgb(if alpha_sum > 0.0 {
                    color / alpha_sum
                } else {
                    0.0
                }));
            }
            output.push(((alpha_sum / 4.0) * 255.0).round() as u8);
        }
    }

    output
}

fn srgb_to_linear(value: u8) -> f32 {
    let value = f32::from(value) / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(value: f32) -> u8 {
    let value = if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn main() {
    let event_loop = EventLoop::new();
    let mut window = WindowBuilder::new()
        .title("Bassie WebGPU Blocks")
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
        Event::Window(id, WindowEvent::ScaleFactorChanged(_)) if id == window_id => {
            gpu.resize();
            gpu.set_safe_area(window.safe_area(), window.size());
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
    fn materials_match_world_voxel_types() {
        let names = material_names();
        assert_eq!(names.len(), world::Material::COUNT);
        for material in world::Material::ALL {
            assert_eq!(names[material.index()], material.file_name());
        }
        for name in names {
            let asset = MaterialAssets::get(&name).expect("Embedded material");
            let image = image::decode(asset.data.as_ref()).expect("Decode material");
            assert_eq!(image.format(), image::Format::Png);
            assert_eq!(
                (image.width(), image.height()),
                (MATERIAL_SIZE, MATERIAL_SIZE)
            );
        }
    }

    #[test]
    fn world_faces_use_valid_materials() {
        let names = material_names();
        let faces = world::World::generate(42).faces_by_material();
        let (bytes, opaque, count) = world::face_bytes(&faces);
        assert!(opaque > 0 && opaque < count);
        assert_eq!(bytes.len(), count as usize * 32);
        assert_eq!(faces.iter().map(Vec::len).sum::<usize>(), count as usize);
        for (index, face) in bytes.as_chunks::<32>().0.iter().enumerate() {
            let side = u32::from_ne_bytes(face[16..20].try_into().unwrap());
            let layer = u32::from_ne_bytes(face[20..24].try_into().unwrap());
            let water = u32::from_ne_bytes(face[24..28].try_into().unwrap());
            assert!(side < 6 && layer < names.len() as u32);
            assert_eq!(water, u32::from(index >= opaque as usize));
            assert_eq!(layer == world::Material::Water as u32, water != 0);
        }
    }

    #[test]
    fn mip_downsampling_averages_in_linear_color_space() {
        let pixels = [
            0, 0, 0, 255, 255, 255, 255, 255, 0, 0, 0, 255, 255, 255, 255, 255,
        ];
        assert_eq!(downsample_rgba(&pixels, 2, 2), [188, 188, 188, 255]);
    }
}
