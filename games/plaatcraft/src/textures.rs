/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use rust_embed::Embed;

#[derive(Embed)]
#[folder = "assets"]
struct Assets;

const TILES: [&str; 75] = [
    "tiles/grass_top.png",
    "tiles/dirt_grass.png",
    "tiles/dirt.png",
    "tiles/stone.png",
    "tiles/sand.png",
    "tiles/snow.png",
    "tiles/dirt_snow.png",
    "tiles/trunk_side.png",
    "tiles/trunk_top.png",
    "tiles/trunk_white_side.png",
    "tiles/trunk_white_top.png",
    "tiles/leaves.png",
    "tiles/leaves_orange.png",
    "tiles/grass1.png",
    "tiles/grass2.png",
    "tiles/grass3.png",
    "tiles/grass4.png",
    "tiles/stone_coal.png",
    // Legacy iron layers alias the canonical art to keep saved material IDs stable.
    "tiles/stone_iron.png",
    "tiles/stone_gold.png",
    "tiles/stone_diamond.png",
    "tiles/stone_silver.png",
    "tiles/water.png",
    "tiles/lava.png",
    "tiles/cactus_side.png",
    "tiles/cactus_top.png",
    "tiles/cactus_inside.png",
    "tiles/brick_grey.png",
    "tiles/brick_red.png",
    "tiles/wood.png",
    "tiles/wood_red.png",
    "tiles/cotton_blue.png",
    "tiles/cotton_green.png",
    "tiles/cotton_red.png",
    "tiles/cotton_white.png",
    "tiles/gravel_dirt.png",
    "tiles/gravel_stone.png",
    "tiles/oven.png",
    "tiles/redstone.png",
    "tiles/greystone.png",
    "tiles/cotton_tan.png",
    "tiles/dirt_sand.png",
    "tiles/greysand.png",
    "tiles/greystone_ruby.png",
    "tiles/greystone_ruby_alt.png",
    "tiles/greystone_sand.png",
    "tiles/ice.png",
    "tiles/redsand.png",
    "tiles/redstone_emerald.png",
    "tiles/redstone_emerald_alt.png",
    "tiles/redstone_sand.png",
    "tiles/stone_iron.png",
    "tiles/stone_iron_alt.png",
    "tiles/stone_coal_alt.png",
    "tiles/stone_diamond_alt.png",
    "tiles/stone_dirt.png",
    "tiles/stone_gold_alt.png",
    "tiles/stone_grass.png",
    "tiles/stone_iron_alt.png",
    "tiles/stone_sand.png",
    "tiles/stone_silver_alt.png",
    "tiles/stone_snow.png",
    "tiles/table.png",
    "tiles/glass.png",
    "tiles/glass_frame.png",
    "tiles/trunk_red_side.png",
    "tiles/trunk_red_top.png",
    "tiles/wood_white.png",
    "tiles/grass_brown.png",
    "tiles/grass_tan.png",
    "tiles/mushroom_brown.png",
    "tiles/mushroom_red.png",
    "tiles/mushroom_tan.png",
    "tiles/rock.png",
    "tiles/rock_moss.png",
];
const SKY: [&str; 8] = [
    "sky/skybox_bottom.png",
    "sky/skybox_side.png",
    "sky/skybox_sideClouds.png",
    "sky/skybox_sideHills.png",
    "sky/skybox_top.png",
    "sky/clouds.png",
    "sky/sun.png",
    "sky/moon.png",
];
const SIZE: u32 = 128;

struct Image {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
}

fn decode(bytes: &[u8]) -> Image {
    let image = image::decode(bytes).expect("Decode Kenney PNG");
    Image {
        pixels: image.pixels().to_vec(),
        width: image.width(),
        height: image.height(),
    }
}

fn resize_nearest(image: &Image, width: u32, height: u32) -> Vec<u8> {
    let mut output = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let source_x = x * image.width / width;
            let source_y = y * image.height / height;
            let index = ((source_y * image.width + source_x) * 4) as usize;
            output.extend_from_slice(&image.pixels[index..index + 4]);
        }
    }
    output
}

fn downsample(pixels: &[u8], size: u32) -> Vec<u8> {
    let next = size / 2;
    let mut output = Vec::with_capacity((next * next * 4) as usize);
    for y in 0..next {
        for x in 0..next {
            let mut color = [0.0f32; 3];
            let mut alpha = 0.0;
            for dy in 0..2 {
                for dx in 0..2 {
                    let index = (((y * 2 + dy) * size + x * 2 + dx) * 4) as usize;
                    let a = f32::from(pixels[index + 3]) / 255.0;
                    for channel in 0..3 {
                        color[channel] +=
                            (f32::from(pixels[index + channel]) / 255.0).powf(2.2) * a;
                    }
                    alpha += a;
                }
            }
            // Alpha-weighted filtering prevents black fringes around transparent sprites.
            for c in color {
                output.push(if alpha > 0.0 {
                    ((c / alpha).powf(1.0 / 2.2) * 255.0).round() as u8
                } else {
                    0
                });
            }
            output.push((alpha * 0.25 * 255.0).round() as u8);
        }
    }
    output
}

fn preserve_coverage(pixels: &mut [u8], coverage: f32) {
    if coverage == 1.0 || coverage == 0.0 {
        return;
    }
    let mut alphas: Vec<_> = pixels.as_chunks::<4>().0.iter().map(|p| p[3]).collect();
    alphas.sort_unstable_by(|a, b| b.cmp(a));
    let target = ((coverage * alphas.len() as f32).round() as usize).clamp(1, alphas.len());
    let threshold = f32::from(alphas[target - 1]);
    if threshold == 0.0 {
        return;
    }
    let scale = 128.0 / threshold;
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel[3] = (f32::from(pixel[3]) * scale).round().min(255.0) as u8;
    }
}

pub(crate) fn load(
    device: &bwebgpu::Device,
    queue: &bwebgpu::Queue,
) -> (bwebgpu::TextureView, bwebgpu::Sampler) {
    let texture = device.create_texture(&bwebgpu::TextureDescriptor {
        label: Some("Kenney voxel and sky textures"),
        size: bwebgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: (TILES.len() + SKY.len()) as u32,
        },
        mip_level_count: 8,
        sample_count: 1,
        dimension: bwebgpu::TextureDimension::D2,
        format: bwebgpu::TextureFormat::Rgba8UnormSrgb,
        usage: bwebgpu::TextureUsages::TEXTURE_BINDING | bwebgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (layer, path) in TILES.into_iter().chain(SKY).enumerate() {
        let file = Assets::get(path).expect("Embedded texture");
        let image = decode(&file.data);
        let mut pixels = if layer < TILES.len() {
            assert_eq!((image.width, image.height), (SIZE, SIZE));
            image.pixels
        } else {
            resize_nearest(&image, SIZE, SIZE)
        };
        let coverage = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] >= 128)
            .count() as f32
            / (SIZE * SIZE) as f32;
        let mut size = SIZE;
        for mip in 0..8 {
            queue.write_texture(
                bwebgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: mip,
                    origin: bwebgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: layer as u32,
                    },
                    aspect: bwebgpu::TextureAspect::All,
                },
                &pixels,
                bwebgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(size * 4),
                    rows_per_image: Some(size),
                },
                bwebgpu::Extent3d {
                    width: size,
                    height: size,
                    depth_or_array_layers: 1,
                },
            );
            if size > 1 {
                pixels = downsample(&pixels, size);
                preserve_coverage(&mut pixels, coverage);
                size /= 2;
            }
        }
    }

    let sampler = device.create_sampler(&bwebgpu::SamplerDescriptor {
        label: Some("Repeating block textures"),
        address_mode_u: bwebgpu::AddressMode::Repeat,
        address_mode_v: bwebgpu::AddressMode::Repeat,
        mag_filter: bwebgpu::FilterMode::Nearest,
        min_filter: bwebgpu::FilterMode::Linear,
        mipmap_filter: bwebgpu::MipmapFilterMode::Linear,
        ..Default::default()
    });
    let view = texture.create_view(&bwebgpu::TextureViewDescriptor {
        dimension: Some(bwebgpu::TextureViewDimension::D2Array),
    });
    (view, sampler)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_and_mips_preserve_opaque_and_cutout_materials() {
        for path in Assets::iter() {
            assert!(TILES.contains(&path.as_ref()) || SKY.contains(&path.as_ref()));
        }

        for tile in TILES {
            let file = Assets::get(tile).unwrap();
            let image = decode(&file.data);
            assert_eq!((image.width, image.height), (SIZE, SIZE));
            let mut pixels = image.pixels;
            let has_transparency = pixels.as_chunks::<4>().0.iter().any(|p| p[3] < 255);
            let coverage = pixels
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[3] >= 128)
                .count() as f32
                / (SIZE * SIZE) as f32;
            let mut size = SIZE;
            loop {
                assert_eq!(pixels.len(), (size * size * 4) as usize);
                if !has_transparency {
                    assert!(pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
                }
                assert!(pixels.as_chunks::<4>().0.iter().any(|p| p[3] >= 128));
                if size == 1 {
                    break;
                }
                pixels = downsample(&pixels, size);
                preserve_coverage(&mut pixels, coverage);
                size /= 2;
            }
        }
    }

    #[test]
    fn transparent_pixels_do_not_darken_visible_mip_colors() {
        let input = [255, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(downsample(&input, 2), vec![255, 0, 0, 64]);
    }

    #[test]
    fn sky_layers_resize_to_the_gpu_array_extent() {
        for path in SKY {
            let file = Assets::get(path).unwrap();
            let pixels = resize_nearest(&decode(&file.data), SIZE, SIZE);
            assert_eq!(pixels.len(), (SIZE * SIZE * 4) as usize);
        }
    }
}
