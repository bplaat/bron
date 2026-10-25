/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::time::{Duration, Instant};

use bcanvas::{CanvasRenderingContext2d, Color, OffscreenCanvas, TextAlign, TextBaseline};
use bwindow::LogicalInsets;

const STATS_WIDTH: u32 = 360;
const STATS_INTERVAL: Duration = Duration::from_millis(250);
const STATS_LINES: u32 = 6;
const LINE_HEIGHT: f32 = 18.0;
pub(crate) const MARGIN: f32 = 16.0;
// Draws bcanvas bitmaps as screen space sprites over the scene.
pub(crate) struct Hud {
    pipeline: bwebgpu::RenderPipeline,
    layout: bwebgpu::BindGroupLayout,
    sampler: bwebgpu::Sampler,
}

impl Hud {
    pub(crate) fn new(device: &bwebgpu::Device, format: bwebgpu::TextureFormat) -> Self {
        let layout = device.create_bind_group_layout(&bwebgpu::BindGroupLayoutDescriptor {
            label: Some("HUD layout"),
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
        let shader = device.create_shader_module(bwebgpu::include_wgsl!("hud.wgsl"));
        let pipeline_layout = device.create_pipeline_layout(&bwebgpu::PipelineLayoutDescriptor {
            label: Some("HUD pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
        });
        let pipeline = device.create_render_pipeline(&bwebgpu::RenderPipelineDescriptor {
            label: Some("fs_hud"),
            layout: Some(&pipeline_layout),
            vertex: bwebgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_hud"),
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
                module: &shader,
                entry_point: Some("fs_hud"),
                compilation_options: Default::default(),
                targets: &[Some(bwebgpu::ColorTargetState {
                    format,
                    blend: Some(bwebgpu::BlendState::ALPHA_BLENDING),
                    write_mask: bwebgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&bwebgpu::SamplerDescriptor {
            mag_filter: bwebgpu::FilterMode::Linear,
            min_filter: bwebgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            layout,
            sampler,
        }
    }

    // Creates a transparent sprite of a logical size at the display scale.
    pub(crate) fn sprite(
        &self,
        device: &bwebgpu::Device,
        width: u32,
        height: u32,
        scale: f32,
    ) -> Sprite {
        let canvas = OffscreenCanvas::with_scale_factor(width, height, scale);
        let texture = device.create_texture(&bwebgpu::TextureDescriptor {
            label: Some("HUD sprite"),
            size: bwebgpu::Extent3d {
                width: canvas.width(),
                height: canvas.height(),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: bwebgpu::TextureDimension::D2,
            format: bwebgpu::TextureFormat::Rgba8UnormSrgb,
            usage: bwebgpu::TextureUsages::TEXTURE_BINDING | bwebgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let group = device.create_bind_group(&bwebgpu::BindGroupDescriptor {
            label: Some("HUD sprite resources"),
            layout: &self.layout,
            entries: &[
                bwebgpu::BindGroupEntry {
                    binding: 1,
                    resource: bwebgpu::BindingResource::TextureView(&view),
                },
                bwebgpu::BindGroupEntry {
                    binding: 2,
                    resource: bwebgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        Sprite {
            canvas,
            texture,
            group,
        }
    }

    // Stretches a sprite over the whole target, for a uniform overlay.
    pub(crate) fn fill(
        &self,
        pass: &mut bwebgpu::RenderPass<'_>,
        sprite: &Sprite,
        (width, height): (u32, u32),
    ) {
        pass.set_viewport(0.0, 0.0, width as f32, height as f32, 0.0, 1.0);
        pass.set_scissor_rect(0, 0, width, height);
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &sprite.group, &[]);
        pass.draw(0..3, 0..1);
    }

    // Draws a sprite with its top left corner at physical pixels, kept inside the target.
    pub(crate) fn draw(
        &self,
        pass: &mut bwebgpu::RenderPass<'_>,
        sprite: &Sprite,
        (x, y): (f32, f32),
        (width, height): (u32, u32),
    ) {
        let (sprite_width, sprite_height) = (sprite.canvas.width(), sprite.canvas.height());
        if sprite_width > width || sprite_height > height {
            return;
        }
        let x = (x.round().max(0.0) as u32).min(width - sprite_width);
        let y = (y.round().max(0.0) as u32).min(height - sprite_height);
        pass.set_viewport(
            x as f32,
            y as f32,
            sprite_width as f32,
            sprite_height as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(x, y, sprite_width, sprite_height);
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &sprite.group, &[]);
        pass.draw(0..3, 0..1);
    }
}

pub(crate) struct Sprite {
    canvas: OffscreenCanvas,
    texture: bwebgpu::Texture,
    group: bwebgpu::BindGroup,
}

impl Sprite {
    pub(crate) const fn scale_factor(&self) -> f32 {
        self.canvas.scale_factor()
    }

    // Clears the sprite, draws it in logical pixels and uploads it.
    pub(crate) fn redraw(
        &mut self,
        queue: &bwebgpu::Queue,
        draw: impl FnOnce(&mut CanvasRenderingContext2d<'_>),
    ) {
        self.canvas.draw(|ctx| {
            ctx.clear_rect(0.0, 0.0, ctx.width(), ctx.height());
            draw(ctx);
        });
        let (width, height) = (self.canvas.width(), self.canvas.height());
        let bytes_per_row = self.canvas.bytes_per_row();
        queue.write_texture(
            bwebgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: Default::default(),
                aspect: bwebgpu::TextureAspect::All,
            },
            self.canvas.pixels(),
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

// Multi-line stats text in the top right corner.
pub(crate) struct Stats {
    sprite: Sprite,
    lines: Vec<String>,
    drawn: Instant,
}

impl Stats {
    pub(crate) fn new(hud: &Hud, device: &bwebgpu::Device, scale: f32) -> Self {
        Self {
            sprite: Self::sprite(hud, device, scale),
            lines: Vec::new(),
            drawn: Instant::now(),
        }
    }

    fn sprite(hud: &Hud, device: &bwebgpu::Device, scale: f32) -> Sprite {
        let height = (STATS_LINES as f32 * LINE_HEIGHT + MARGIN / 2.0) as u32;
        hud.sprite(device, STATS_WIDTH, height, scale)
    }

    // Redraws the text only when the lines or the display scale changed.
    pub(crate) fn update(
        &mut self,
        hud: &Hud,
        device: &bwebgpu::Device,
        queue: &bwebgpu::Queue,
        scale: f32,
        lines: Vec<String>,
    ) {
        if (self.sprite.scale_factor() - scale).abs() > 0.001 {
            self.sprite = Self::sprite(hud, device, scale);
            self.lines.clear();
        }
        // The position changes every frame while moving, so redraw a few times a second.
        if self.lines == lines || !self.lines.is_empty() && self.drawn.elapsed() < STATS_INTERVAL {
            return;
        }
        self.lines = lines;
        self.drawn = Instant::now();
        self.sprite.redraw(queue, |ctx| {
            ctx.set_font("sans-serif", 14.0);
            ctx.set_text_align(TextAlign::Right);
            ctx.set_text_baseline(TextBaseline::Top);
            let x = ctx.width() - MARGIN / 2.0;
            for (index, line) in self.lines.iter().enumerate() {
                let y = MARGIN / 4.0 + index as f32 * LINE_HEIGHT;
                // A dark offset copy keeps the text readable over bright sky and terrain.
                ctx.set_fill_style(Color::rgba(0, 0, 0, 160));
                ctx.fill_text(line, x + 1.0, y + 1.0);
                ctx.set_fill_style(Color::rgb(255, 255, 255));
                ctx.fill_text(line, x, y);
            }
        });
    }

    pub(crate) fn draw(
        &self,
        hud: &Hud,
        pass: &mut bwebgpu::RenderPass<'_>,
        safe_area: LogicalInsets,
        target: (u32, u32),
    ) {
        let scale = self.sprite.scale_factor();
        let x = target.0 as f32
            - (safe_area.right + MARGIN / 2.0) * scale
            - self.sprite.canvas.width() as f32;
        hud.draw(
            pass,
            &self.sprite,
            (x, (safe_area.top + MARGIN / 2.0) * scale),
            target,
        );
    }
}

// A sprite that is only redrawn when the key of what it shows changes.
pub(crate) struct Cached {
    key: [u32; 5],
    scale: f32,
    sprite: Sprite,
}

impl Cached {
    pub(crate) const fn sprite(&self) -> &Sprite {
        &self.sprite
    }
}

// Creates and redraws cached sprites at the display scale.
pub(crate) struct Painter<'a> {
    pub(crate) hud: &'a Hud,
    pub(crate) device: &'a bwebgpu::Device,
    pub(crate) queue: &'a bwebgpu::Queue,
    pub(crate) scale: f32,
}

impl Painter<'_> {
    // Redraws a sprite when its key changed, the first two key values set its size.
    pub(crate) fn paint(
        &self,
        cached: &mut Option<Cached>,
        key: [u32; 5],
        (width, height): (f32, f32),
        draw: impl FnOnce(&mut CanvasRenderingContext2d<'_>),
    ) {
        if cached
            .as_ref()
            .is_some_and(|cached| cached.key == key && cached.scale == self.scale)
        {
            return;
        }
        let mut sprite = match cached.take() {
            Some(old) if old.scale == self.scale && old.key[..2] == key[..2] => old.sprite,
            _ => self.hud.sprite(
                self.device,
                width.ceil() as u32,
                height.ceil() as u32,
                self.scale,
            ),
        };
        sprite.redraw(self.queue, draw);
        *cached = Some(Cached {
            key,
            scale: self.scale,
            sprite,
        });
    }
}
