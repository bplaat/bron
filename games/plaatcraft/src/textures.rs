/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use cargo_bundle_assets::Assets;
use plaatcraft_engine::tiles::{FACE_TILES, TILES};
use plaatcraft_engine::world;

#[derive(Assets)]
#[folder = "assets"]
struct Assets;

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
/// The first texture layer of the character parts, after the sky and the tiles.
pub(crate) const CHARACTER_LAYER: u32 = (SKY.len() + TILES.len()) as u32;

// The parts of the Kenney male character, see the engine's avatars.
fn character_paths() -> impl Iterator<Item = String> {
    plaatcraft_engine::avatars::PARTS
        .iter()
        .map(|part| format!("characters/male_{part}.png"))
}
// Materials have a 7-bit ID in the packed quad format.
const MATERIAL_COUNT: usize = 128;
const MATERIAL_BYTES: usize = 48;
/// The size of the material table at the start of the scene storage buffer.
pub(crate) const MATERIAL_TABLE_BYTES: usize = MATERIAL_COUNT * MATERIAL_BYTES;

/// The texture layer of a face of a material, faces 6 and 7 are the sprite planes.
pub(crate) fn layer(material: u8, face: usize) -> u32 {
    // The sky layers come first so they never move.
    (SKY.len() + usize::from(FACE_TILES[usize::from(material)][face])) as u32
}

/// Encode the per-material tint, flags and face layers the shaders look up by material ID.
pub(crate) fn material_table() -> Vec<u8> {
    let mut table = vec![0; MATERIAL_TABLE_BYTES];
    for (material, entry) in table
        .as_chunks_mut::<MATERIAL_BYTES>()
        .0
        .iter_mut()
        .enumerate()
    {
        let Some(look) = world::look(material as u8) else {
            continue;
        };
        let mut words = [0u32; MATERIAL_BYTES / 4];
        for (word, tint) in words.iter_mut().zip(look.tint) {
            *word = tint.to_bits();
        }
        words[3] = world::look_flags(material as u8);
        for (face, word) in words[4..].iter_mut().enumerate() {
            *word = layer(material as u8, face);
        }
        for (bytes, word) in entry.as_chunks_mut::<4>().0.iter_mut().zip(words) {
            *bytes = word.to_ne_bytes();
        }
    }
    table
}

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
            depth_or_array_layers: CHARACTER_LAYER + plaatcraft_engine::avatars::PARTS.len() as u32,
        },
        mip_level_count: 8,
        sample_count: 1,
        dimension: bwebgpu::TextureDimension::D2,
        format: bwebgpu::TextureFormat::Rgba8UnormSrgb,
        usage: bwebgpu::TextureUsages::TEXTURE_BINDING | bwebgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let paths = SKY
        .into_iter()
        .chain(TILES)
        .map(str::to_owned)
        .chain(character_paths());
    for (layer, path) in paths.enumerate() {
        let file = Assets::get(&path).expect("Texture asset");
        let image = decode(&file.data);
        // Character parts fill their layer, the boxes they cover keep their proportions.
        let mut pixels = if (SKY.len()..CHARACTER_LAYER as usize).contains(&layer) {
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
    fn every_material_resolves_its_tiles() {
        let table = material_table();
        assert_eq!(table.len(), MATERIAL_TABLE_BYTES);
        for &(material, name) in world::MATERIALS {
            assert!(world::look(material).is_some(), "{name} has no look");
        }
        let unique: std::collections::HashSet<_> = TILES.iter().collect();
        assert_eq!(unique.len(), TILES.len());
    }

    #[test]
    fn tiles_and_mips_preserve_opaque_and_cutout_materials() {
        let characters: Vec<_> = character_paths().collect();
        for path in Assets::iter() {
            assert!(
                TILES.contains(&path.as_ref())
                    || SKY.contains(&path.as_ref())
                    || characters.iter().any(|character| *character == path)
            );
        }
        for path in characters {
            assert!(Assets::get(&path).is_some(), "{path} is missing");
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
