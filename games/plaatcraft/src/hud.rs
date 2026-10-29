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

const CHAT_TIME: Duration = Duration::from_secs(10);
const CHAT_HISTORY: usize = 50;
const CHAT_RECENT: usize = 5;
const CHAT_OPEN: usize = 10;
const CHAT_WIDTH: f32 = 440.0;
const CHAT_LINE: f32 = 20.0;
// The chat stays above the hotbar.
const CHAT_BOTTOM: f32 = 84.0;

struct ChatLine {
    // Who said it, world messages have no name.
    name: Option<String>,
    text: String,
    received: Instant,
}

// The chat in the bottom left: recent messages, and all of them with an input bar while
// typing.
#[derive(Default)]
pub(crate) struct Chat {
    lines: std::collections::VecDeque<ChatLine>,
    input: Option<String>,
    cached: Option<Cached>,
}

impl Chat {
    pub(crate) fn push(&mut self, name: Option<String>, text: String) {
        if self.lines.len() == CHAT_HISTORY {
            self.lines.pop_front();
        }
        self.lines.push_back(ChatLine {
            name,
            text,
            received: Instant::now(),
        });
    }

    pub(crate) fn clear(&mut self) {
        self.lines.clear();
        self.input = None;
    }

    pub(crate) const fn typing(&self) -> bool {
        self.input.is_some()
    }

    pub(crate) fn open(&mut self, text: &str) {
        self.input = Some(text.to_owned());
    }

    pub(crate) fn close(&mut self) {
        self.input = None;
    }

    pub(crate) fn text(&self) -> &str {
        self.input.as_deref().unwrap_or_default()
    }

    pub(crate) fn backspace(&mut self) {
        if let Some(input) = &mut self.input {
            input.pop();
        }
    }

    pub(crate) fn type_text(&mut self, text: &str, length: usize) {
        if let Some(input) = &mut self.input {
            for c in text.chars().filter(|c| !c.is_control()) {
                if input.chars().count() < length {
                    input.push(c);
                }
            }
        }
    }

    pub(crate) fn prepare(&mut self, painter: &Painter<'_>) {
        let typing = self.typing();
        let shown: Vec<_> = if typing {
            self.lines.iter().rev().take(CHAT_OPEN).rev().collect()
        } else {
            let recent: Vec<_> = self
                .lines
                .iter()
                .filter(|line| line.received.elapsed() < CHAT_TIME)
                .collect();
            recent[recent.len().saturating_sub(CHAT_RECENT)..].to_vec()
        };
        if shown.is_empty() && !typing {
            self.cached = None;
            return;
        }
        let mut hasher = std::hash::DefaultHasher::new();
        for line in &shown {
            std::hash::Hash::hash(&(&line.name, &line.text), &mut hasher);
        }
        std::hash::Hash::hash(&self.input, &mut hasher);
        let hash = std::hash::Hasher::finish(&hasher);
        let rows = shown.len() + usize::from(typing);
        let height = rows as f32 * CHAT_LINE;
        let input = self.input.as_deref();
        painter.paint(
            &mut self.cached,
            [height.to_bits(), 0, hash as u32, (hash >> 32) as u32, 0],
            (CHAT_WIDTH, height),
            |ctx| {
                ctx.set_font("sans-serif", 14.0);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                for (row, line) in shown.iter().enumerate() {
                    let y = row as f32 * CHAT_LINE;
                    let text = match &line.name {
                        Some(name) => format!("<{name}> {}", line.text),
                        None => line.text.clone(),
                    };
                    let width = (ctx.measure_text(&text) + 12.0).min(CHAT_WIDTH);
                    ctx.set_fill_style(Color::rgba(0, 0, 0, 120));
                    ctx.fill_rect(0.0, y, width, CHAT_LINE);
                    ctx.set_fill_style(if line.name.is_some() {
                        Color::rgb(255, 255, 255)
                    } else {
                        Color::rgb(255, 255, 160)
                    });
                    ctx.fill_text(&text, 6.0, y + CHAT_LINE / 2.0);
                }
                if let Some(input) = input {
                    let y = shown.len() as f32 * CHAT_LINE;
                    ctx.set_fill_style(Color::rgba(0, 0, 0, 170));
                    ctx.fill_rect(0.0, y, CHAT_WIDTH, CHAT_LINE);
                    ctx.set_fill_style(Color::rgb(255, 255, 255));
                    ctx.fill_text(format!("> {input}_"), 6.0, y + CHAT_LINE / 2.0);
                }
            },
        );
    }

    pub(crate) fn draw(
        &self,
        hud: &Hud,
        pass: &mut bwebgpu::RenderPass<'_>,
        safe_area: LogicalInsets,
        target: (u32, u32),
    ) {
        if let Some(cached) = &self.cached {
            let scale = cached.sprite.scale_factor();
            let height = cached.sprite.canvas.height() as f32;
            hud.draw(
                pass,
                &cached.sprite,
                (
                    (safe_area.left + MARGIN / 2.0) * scale,
                    target.1 as f32 - (safe_area.bottom + CHAT_BOTTOM) * scale - height,
                ),
                target,
            );
        }
    }
}

const TAG_WIDTH: f32 = 200.0;
const TAG_HEIGHT: f32 = 24.0;

// The names above the other players.
#[derive(Default)]
pub(crate) struct NameTags {
    cached: std::collections::HashMap<String, Cached>,
}

impl NameTags {
    pub(crate) fn prepare<'a>(
        &mut self,
        painter: &Painter<'_>,
        names: impl Iterator<Item = &'a str>,
    ) {
        let names: Vec<_> = names.collect();
        self.cached.retain(|name, _| names.contains(&name.as_str()));
        for name in names {
            let mut cached = self.cached.remove(name);
            painter.paint(&mut cached, [0; 5], (TAG_WIDTH, TAG_HEIGHT), |ctx| {
                ctx.set_font("sans-serif", 14.0);
                ctx.set_text_align(TextAlign::Center);
                ctx.set_text_baseline(TextBaseline::Middle);
                let width = ctx.measure_text(name).min(TAG_WIDTH - 12.0) + 12.0;
                ctx.set_fill_style(Color::rgba(0, 0, 0, 110));
                ctx.fill_rect((TAG_WIDTH - width) / 2.0, 0.0, width, TAG_HEIGHT);
                ctx.set_fill_style(Color::rgb(255, 255, 255));
                ctx.fill_text(name, TAG_WIDTH / 2.0, TAG_HEIGHT / 2.0);
            });
            if let Some(cached) = cached {
                self.cached.insert(name.to_owned(), cached);
            }
        }
    }

    // Draws the tags centered above physical pixel positions.
    pub(crate) fn draw(
        &self,
        hud: &Hud,
        pass: &mut bwebgpu::RenderPass<'_>,
        tags: &[(&str, [f32; 2])],
        target: (u32, u32),
    ) {
        for (name, [x, y]) in tags {
            if let Some(cached) = self.cached.get(*name) {
                let scale = cached.sprite.scale_factor();
                let position = (x - TAG_WIDTH * scale / 2.0, y - TAG_HEIGHT * scale);
                if position.0 >= 0.0
                    && position.1 >= 0.0
                    && position.0 + TAG_WIDTH * scale <= target.0 as f32
                {
                    hud.draw(pass, &cached.sprite, position, target);
                }
            }
        }
    }
}
