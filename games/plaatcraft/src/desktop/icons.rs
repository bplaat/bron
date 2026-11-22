/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The block icons of the hotbar and the inventory: the GPU renders every material once into
//! an atlas, which the HUD layer draws like any image.

use bcanvas::{CanvasImage, CanvasRenderingContext2d, premultiplied_bgra};

use crate::desktop::inventory::Icon;

// Large enough for the biggest slots on high DPI displays, drawing scales it down.
const ICON_PIXELS: u32 = 96;
const COLUMNS: u32 = 16;
// The material table has 128 entries, see textures.rs.
const MATERIALS: u32 = 128;

pub(crate) struct IconAtlas {
    image: CanvasImage,
}

impl IconAtlas {
    // Renders the preview of every material into its cell.
    pub(crate) fn new(
        device: &bwebgpu::Device,
        queue: &bwebgpu::Queue,
        layout: &bwebgpu::PipelineLayout,
        bind_group: &bwebgpu::BindGroup,
    ) -> Self {
        let pipeline = super::pipeline(
            device,
            bwebgpu::include_wgsl!("preview.wgsl"),
            layout,
            bwebgpu::TextureFormat::Rgba8Unorm,
            "fs_preview",
        );
        let (width, height) = (COLUMNS * ICON_PIXELS, MATERIALS / COLUMNS * ICON_PIXELS);
        let target = device.create_bitmap_target(width, height);
        let depth = device
            .create_texture(&bwebgpu::TextureDescriptor {
                label: Some("Icon depth"),
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
            .create_view(&Default::default());
        let view = target.create_view();
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&bwebgpu::RenderPassDescriptor {
                color_attachments: &[Some(bwebgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: bwebgpu::Operations {
                        load: bwebgpu::LoadOp::Clear(bwebgpu::Color::TRANSPARENT),
                        store: bwebgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(bwebgpu::RenderPassDepthStencilAttachment {
                    view: &depth,
                    depth_ops: Some(bwebgpu::Operations {
                        load: bwebgpu::LoadOp::Clear(1.0),
                        store: bwebgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            for material in 0..MATERIALS {
                let (x, y) = (material % COLUMNS, material / COLUMNS);
                let size = ICON_PIXELS as f32;
                pass.set_viewport(x as f32 * size, y as f32 * size, size, size, 0.0, 1.0);
                pass.draw(0..3, material..material + 1);
            }
        }
        queue.submit([encoder.finish()]);
        let bitmap = target.read(queue).expect("Read block icons");
        // Blending onto transparent black leaves linear premultiplied colors, which the canvas
        // takes as straight sRGB.
        let mut rgba = bitmap.data().to_vec();
        for pixel in rgba.as_chunks_mut::<4>().0 {
            let alpha = f32::from(pixel[3]) / 255.0;
            if alpha > 0.0 {
                for channel in &mut pixel[..3] {
                    let linear = (f32::from(*channel) / 255.0 / alpha).min(1.0);
                    *channel = (srgb(linear) * 255.0).round() as u8;
                }
            }
        }
        Self {
            image: CanvasImage::from_premultiplied_bgra(width, height, premultiplied_bgra(&rgba)),
        }
    }

    // Draws icons in logical pixels, clipped to their clip rectangles.
    pub(crate) fn draw(&self, ctx: &mut CanvasRenderingContext2d<'_>, icons: &[Icon]) {
        let size = ICON_PIXELS as f32;
        for icon in icons {
            let material = u32::from(icon.material);
            let (column, row) = (material % COLUMNS, material / COLUMNS);
            let (clip_x, clip_y, clip_width, clip_height) = icon.clip;
            ctx.save();
            ctx.begin_path();
            ctx.rect(clip_x, clip_y, clip_width, clip_height);
            ctx.clip();
            ctx.draw_image_region(
                &self.image,
                column as f32 * size,
                row as f32 * size,
                size,
                size,
                icon.x,
                icon.y,
                icon.size,
                icon.size,
            );
            ctx.restore();
        }
    }
}

fn srgb(linear: f32) -> f32 {
    if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}
