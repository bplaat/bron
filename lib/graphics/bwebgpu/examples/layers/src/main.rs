/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Stacks native window layers: a bwebgpu triangle at the bottom, a tall bcanvas page that
//! scrolls by moving its layer without being redrawn, and a bcanvas HUD on top that is
//! redrawn every frame.

use std::time::Instant;

use bcanvas::{CanvasBuilder, Color, TextBaseline};
use bwindow::{
    Event, EventLoop, KeyCode, LogicalRect, LogicalSize, Theme, WindowBuilder, WindowEvent,
};

const BACKGROUND_COLOR: u32 = 0x000000;
const PAGE_WIDTH: f32 = 360.0;
const PAGE_HEIGHT: f32 = 2000.0;
const LINE_HEIGHT: f32 = 40.0;
const SCROLL_SPEED: f32 = 60.0;

struct Gpu {
    surface: bwebgpu::Surface<'static>,
    device: bwebgpu::Device,
    queue: bwebgpu::Queue,
    config: bwebgpu::SurfaceConfiguration,
    pipeline: bwebgpu::RenderPipeline,
    vertices: bwebgpu::Buffer,
}

impl Gpu {
    fn new(layer: &bwindow::WindowLayer) -> Self {
        // Configure the surface and device
        let instance = bwebgpu::Instance::default();
        let surface = instance.create_surface(layer).expect("Create surface");
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

        // Create the triangle pipeline
        let shader = device.create_shader_module(bwebgpu::include_wgsl!("triangle.wgsl"));
        let bytes: Vec<u8> = [
            [0.0f32, 0.75, 1.0, 0.1, 0.1, 1.0],
            [-0.7, -0.6, 0.1, 1.0, 0.2, 1.0],
            [0.7, -0.6, 0.1, 0.3, 1.0, 1.0],
        ]
        .into_iter()
        .flatten()
        .flat_map(f32::to_ne_bytes)
        .collect();
        let vertices = device.create_buffer(&bwebgpu::BufferDescriptor {
            label: Some("triangle vertices"),
            size: bytes.len() as u64,
            usage: bwebgpu::BufferUsages::VERTEX,
            mapped_at_creation: true,
        });
        vertices
            .slice(..)
            .get_mapped_range_mut()
            .expect("Mapped vertices")
            .copy_from_slice(&bytes);
        vertices.unmap();
        let pipeline = device.create_render_pipeline(&bwebgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: bwebgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(bwebgpu::VertexBufferLayout {
                    array_stride: 24,
                    step_mode: bwebgpu::VertexStepMode::Vertex,
                    attributes: &bwebgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4],
                })],
            },
            primitive: Default::default(),
            depth_stencil: None,
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

        Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            vertices,
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
    }

    fn render(&mut self) -> bool {
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
                depth_stencil_attachment: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_vertex_buffer(0, self.vertices.slice(..));
            pass.draw(0..3, 0..1);
        }

        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
        if reconfigure {
            self.resize();
        }

        false
    }
}

// Draws the page of lines into its layer, once per size.
fn draw_page(ctx: &mut bcanvas::CanvasRenderingContext2d<'_>) {
    ctx.set_fill_style(Color::rgba(20, 24, 40, 200));
    ctx.fill_rect(0.0, 0.0, ctx.width(), ctx.height());
    ctx.set_font("sans-serif", 20.0);
    ctx.set_text_baseline(TextBaseline::Middle);
    for line in 0..(PAGE_HEIGHT / LINE_HEIGHT) as u32 {
        let y = line as f32 * LINE_HEIGHT + LINE_HEIGHT / 2.0;
        ctx.set_fill_style(Color::rgb(230, 230, 240));
        ctx.fill_text(format!("Line {line} of a scrolled layer 📜"), 20.0, y);
    }
}

fn main() {
    let event_loop = EventLoop::new();
    let mut window = WindowBuilder::new()
        .title("Bassie Window Layers")
        .size(LogicalSize::new(900.0, 650.0))
        .min_size(LogicalSize::new(480.0, 360.0))
        .resizable(true)
        .theme(Theme::Dark)
        .background_color(BACKGROUND_COLOR)
        .center()
        .build();
    let window_id = window.id();
    // The GPU layer at the bottom, the canvas layers stack above it.
    let mut gpu = Gpu::new(&window.add_layer());
    let page_layer = window.add_layer();
    let mut page = CanvasBuilder::new(&page_layer).build();
    let mut hud = CanvasBuilder::new(&window.add_layer()).build();

    let start = Instant::now();
    let (mut page_redraws, mut hud_redraws) = (0u32, 0u32);
    window.request_redraw();
    event_loop.run(move |event| match event {
        Event::Window(id, WindowEvent::RedrawRequested) if id == window_id => {
            // Only the layer whose native paint delivered this redraw gets a frame.
            if page.draw(draw_page) {
                page_redraws += 1;
            }
            if hud.draw(|ctx| {
                ctx.clear_rect(0.0, 0.0, ctx.width(), ctx.height());
                ctx.set_fill_style(Color::rgba(0, 0, 0, 160));
                ctx.fill_rect(16.0, 16.0, 300.0, 64.0);
                ctx.set_fill_style(Color::rgb(255, 220, 120));
                ctx.set_font("sans-serif", 16.0);
                ctx.set_text_baseline(TextBaseline::Top);
                ctx.fill_text(format!("HUD redraws: {hud_redraws}"), 28.0, 26.0);
                ctx.fill_text(format!("Page redraws: {page_redraws}"), 28.0, 50.0);
            }) {
                hud_redraws += 1;
                // Scroll the page by moving its layer, which the compositor does without
                // redrawing it.
                let size = window.size();
                let travel = (PAGE_HEIGHT - size.height).max(0.0);
                let scroll = (start.elapsed().as_secs_f32() * SCROLL_SPEED) % travel.max(1.0);
                page_layer.set_frame(Some(LogicalRect::new(
                    size.width - PAGE_WIDTH - 32.0,
                    -scroll,
                    PAGE_WIDTH,
                    PAGE_HEIGHT,
                )));
                hud.request_animation_frame();
            } else if gpu.render() {
                gpu.surface.request_animation_frame();
            }
        }
        Event::Window(id, WindowEvent::Resize(_) | WindowEvent::ScaleFactorChanged(_))
            if id == window_id =>
        {
            gpu.resize();
            window.request_redraw();
        }
        Event::Window(id, WindowEvent::KeyDown(event))
            if id == window_id && event.code == Some(KeyCode::Escape) =>
        {
            window.close();
        }
        _ => {}
    });
}
