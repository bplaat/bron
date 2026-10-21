/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Draws a colored triangle with bwebgpu and bwindow.

use bwindow::{Event, EventLoop, KeyCode, LogicalSize, Theme, WindowBuilder, WindowEvent};

const BACKGROUND_COLOR: u32 = 0x000000;

struct Gpu {
    surface: bwebgpu::Surface<'static>,
    device: bwebgpu::Device,
    queue: bwebgpu::Queue,
    config: bwebgpu::SurfaceConfiguration,
    pipeline: bwebgpu::RenderPipeline,
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
        let pipeline = device.create_render_pipeline(&bwebgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: bwebgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
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

fn main() {
    let event_loop = EventLoop::new();
    let mut window = WindowBuilder::new()
        .title("Bassie WebGPU Triangle")
        .size(LogicalSize::new(900.0, 650.0))
        .min_size(LogicalSize::new(480.0, 360.0))
        .resizable(true)
        .theme(Theme::Dark)
        .background_color(BACKGROUND_COLOR)
        .center()
        .build();
    let mut gpu = Gpu::new(window.attach_content().expect("Attach graphics surface"));
    let window_id = window.id();

    window.request_redraw();
    event_loop.run(move |event| match event {
        Event::Window(id, WindowEvent::RedrawRequested) if id == window_id => {
            if gpu.render() {
                window.request_animation_frame();
            }
        }
        Event::Window(id, WindowEvent::Resize(_)) if id == window_id => {
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
