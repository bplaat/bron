/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(missing_docs)]

use std::hint::black_box;
use std::time::Duration;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use image::{Bitmap, Compression, EncodeOptions, Format, Frame, LoopCount};

// Builds a bottom-up 24-bit BMP gradient, avoiding a large checked-in fixture.
fn bmp(width: u32, height: u32) -> Vec<u8> {
    let stride = (width * 3).next_multiple_of(4);
    let offset = 54u32;
    let size = offset + stride * height;
    let mut out = Vec::with_capacity(size as usize);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&offset.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]);
    for y in 0..height {
        for x in 0..width {
            out.extend_from_slice(&[x as u8, y as u8, (x ^ y) as u8]);
        }
        out.resize(out.len() + (stride - width * 3) as usize, 0);
    }
    out
}

// Builds a bottom-up 24-bit TGA gradient, like the BMP above.
fn tga(width: u16, height: u16) -> Vec<u8> {
    let mut out = vec![0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&[24, 0]);
    for y in 0..height {
        for x in 0..width {
            out.extend_from_slice(&[x as u8, y as u8, (x ^ y) as u8]);
        }
    }
    out
}

// Encodes a JPEG with the smooth shapes and fine grain of a photo, which unlike flat fixtures
// leaves most DCT coefficients nonzero.
fn photo_jpeg(width: u32, height: u32) -> Vec<u8> {
    let mut state = 0x2545_f491u32;
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let wave = |scale: f32, phase: f32| {
                ((x as f32 * scale + phase).sin() + (y as f32 * scale * 0.7).cos()) * 50.0 + 128.0
            };
            let grain = (state & 31) as f32 - 16.0;
            pixels.extend_from_slice(&[
                (wave(0.03, 0.0) + grain) as u8,
                (wave(0.05, 1.0) + grain) as u8,
                (wave(0.02, 2.0) + grain) as u8,
                255,
            ]);
        }
    }
    let bitmap = Bitmap::new(width, height, pixels).expect("valid bitmap");
    image::encode(&bitmap, Format::Jpeg).expect("valid JPEG")
}

fn decode(c: &mut Criterion) {
    let bmp = bmp(800, 600);
    let tga = tga(800, 600);
    let photo = if cfg!(feature = "jpeg") {
        photo_jpeg(800, 600)
    } else {
        Vec::new()
    };
    let mut raster = c.benchmark_group("image_decode_raster");
    for (name, enabled, bytes) in [
        ("bmp", cfg!(feature = "bmp"), &bmp[..]),
        (
            "gif",
            cfg!(feature = "gif"),
            &include_bytes!("images/dice.gif")[..],
        ),
        (
            "gif_animated",
            cfg!(feature = "gif"),
            &include_bytes!("images/dice-animated.gif")[..],
        ),
        (
            "gif_interlaced",
            cfg!(feature = "gif"),
            &include_bytes!("../tests/images/gif/anim/interlaced.gif")[..],
        ),
        (
            "ico",
            cfg!(feature = "ico"),
            &include_bytes!("../tests/images/ico/png-32bpp-alpha.ico")[..],
        ),
        (
            "jpeg_baseline",
            cfg!(feature = "jpeg"),
            &include_bytes!("images/dice-baseline.jpg")[..],
        ),
        (
            "jpeg_progressive",
            cfg!(feature = "jpeg"),
            &include_bytes!("images/dice-progressive.jpg")[..],
        ),
        ("jpeg_photo", cfg!(feature = "jpeg"), &photo[..]),
        (
            "jpeg_restart_420",
            cfg!(feature = "jpeg"),
            &include_bytes!("../tests/images/jpeg/subsampled/restart-420.jpg")[..],
        ),
        (
            "png",
            cfg!(feature = "png"),
            &include_bytes!("images/dice.png")[..],
        ),
        (
            "png_interlaced",
            cfg!(feature = "png"),
            &include_bytes!("../tests/images/png/interlaced/basi2c08.png")[..],
        ),
        (
            "png_rgba16",
            cfg!(feature = "png"),
            &include_bytes!("../tests/images/png/16bpc/basn6a16.png")[..],
        ),
        (
            "apng",
            cfg!(feature = "png"),
            &include_bytes!("../tests/images/png/apng/wpt/fcTL-dispose-in-region-previous.png")[..],
        ),
        (
            "qoi",
            cfg!(feature = "qoi"),
            &include_bytes!("images/dice.qoi")[..],
        ),
        ("tga", cfg!(feature = "tga"), &tga[..]),
    ] {
        if !enabled {
            continue;
        }
        let decoded = image::decode(bytes).expect("valid raster image");
        // Icon frames each have their own size.
        let pixels = decoded
            .frames()
            .iter()
            .map(|frame| u64::from(frame.bitmap().width()) * u64::from(frame.bitmap().height()))
            .sum();
        raster.throughput(Throughput::Elements(pixels));
        raster.bench_function(name, |b| {
            b.iter(|| black_box(image::decode(black_box(bytes)).expect("valid raster image")));
        });
    }
    raster.finish();
}

fn encode(c: &mut Criterion) {
    let mut pixels = Vec::with_capacity(128 * 128 * 4);
    for y in 0..128u8 {
        for x in 0..128u8 {
            pixels.extend_from_slice(&[x, y, x ^ y, 255]);
        }
    }
    let bitmap = Bitmap::new(128, 128, pixels).expect("valid bitmap");
    let formats: &[(&str, Format)] = &[
        #[cfg(feature = "bmp")]
        ("bmp", Format::Bmp),
        #[cfg(feature = "gif")]
        ("gif", Format::Gif),
        #[cfg(feature = "jpeg")]
        ("jpeg", Format::Jpeg),
        #[cfg(feature = "png")]
        ("png", Format::Png),
        #[cfg(feature = "qoi")]
        ("qoi", Format::Qoi),
        #[cfg(feature = "tga")]
        ("tga", Format::Tga),
    ];
    let compressions = [("normal", Compression::Normal), ("max", Compression::Max)];
    let mut raster = c.benchmark_group("image_encode_raster");
    raster.throughput(Throughput::Elements(128 * 128));
    for &(name, format) in formats {
        for &(compression_name, compression) in &compressions {
            let bench_name = format!("{name}_{compression_name}");
            let options = EncodeOptions {
                compression,
                ..EncodeOptions::default()
            };
            raster.bench_function(&bench_name, |b| {
                b.iter(|| {
                    black_box(
                        image::encode_with_options(black_box(&bitmap), format, options)
                            .expect("encode bitmap"),
                    )
                });
            });
        }
    }
    let mut palette_pixels = Vec::with_capacity(128 * 128 * 4);
    for y in 0..128u8 {
        for x in 0..128u8 {
            palette_pixels.extend_from_slice(&match (x / 8 + y / 8) % 4 {
                0 => [255, 0, 0, 255],
                1 => [0, 255, 0, 255],
                2 => [0, 0, 255, 255],
                _ => [255, 255, 0, 255],
            });
        }
    }
    let palette_bitmap = Bitmap::new(128, 128, palette_pixels).expect("valid bitmap");
    let palette_formats: &[(&str, Format)] = &[
        #[cfg(feature = "bmp")]
        ("bmp_palette", Format::Bmp),
        #[cfg(feature = "gif")]
        ("gif_palette", Format::Gif),
        #[cfg(feature = "png")]
        ("png_palette", Format::Png),
    ];
    for &(name, format) in palette_formats {
        for &(compression_name, compression) in &compressions {
            let bench_name = format!("{name}_{compression_name}");
            let options = EncodeOptions {
                compression,
                ..EncodeOptions::default()
            };
            raster.bench_function(&bench_name, |b| {
                b.iter(|| {
                    black_box(
                        image::encode_with_options(black_box(&palette_bitmap), format, options)
                            .expect("encode bitmap"),
                    )
                });
            });
        }
    }
    #[cfg(feature = "png")]
    {
        let mut transparent_pixels = bitmap.data().to_vec();
        transparent_pixels[3] = 0;
        let transparent = Bitmap::new(128, 128, transparent_pixels).expect("valid bitmap");
        for &(compression_name, compression) in &compressions {
            let bench_name = format!("png_transparent_{compression_name}");
            let options = EncodeOptions {
                compression,
                ..EncodeOptions::default()
            };
            raster.bench_function(&bench_name, |b| {
                b.iter(|| {
                    black_box(
                        image::encode_with_options(black_box(&transparent), Format::Png, options)
                            .expect("encode bitmap"),
                    )
                });
            });
        }
    }
    raster.finish();

    let mut second = bitmap.data().to_vec();
    for y in 32..96 {
        for x in 32..96 {
            let offset = (y * 128 + x) * 4;
            second[offset..offset + 4].copy_from_slice(&[255, 0, 0, 255]);
        }
    }
    let frames = [
        Frame::new(
            Bitmap::new(128, 128, bitmap.data().to_vec()).expect("valid bitmap"),
            Duration::from_millis(100),
        ),
        Frame::new(
            Bitmap::new(128, 128, second).expect("valid bitmap"),
            Duration::from_millis(100),
        ),
    ];
    let animated_formats: &[(&str, Format)] = &[
        #[cfg(feature = "gif")]
        ("gif", Format::Gif),
        #[cfg(feature = "png")]
        ("apng", Format::Png),
    ];
    let mut animation = c.benchmark_group("image_encode_animation");
    animation.throughput(Throughput::Elements(128 * 128 * frames.len() as u64));
    for &(name, format) in animated_formats {
        for &(compression_name, compression) in &compressions {
            let bench_name = format!("{name}_{compression_name}");
            let options = EncodeOptions {
                compression,
                ..EncodeOptions::default()
            };
            animation.bench_function(&bench_name, |b| {
                b.iter(|| {
                    black_box(
                        image::encode_animation_with_options(
                            format,
                            black_box(&frames),
                            LoopCount::Infinite,
                            options,
                        )
                        .expect("encode animation"),
                    )
                });
            });
        }
    }
    animation.finish();
}

#[cfg_attr(not(feature = "tga"), allow(clippy::missing_const_for_fn))]
fn encode_tga(c: &mut Criterion) {
    #[cfg(feature = "tga")]
    {
        let mut group = c.benchmark_group("image_encode_tga");
        group.throughput(Throughput::Elements(256 * 256));
        for name in [
            "flat",
            "gray",
            "gray_alpha",
            "palette",
            "noise",
            "transparent",
        ] {
            let mut seed = 123456789u32;
            let mut pixels = Vec::with_capacity(256 * 256 * 4);
            for y in 0..256u32 {
                for x in 0..256u32 {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    let v = x as u8;
                    pixels.extend_from_slice(&match name {
                        "flat" => [30, 60, 90, 255],
                        "gray" => [v, v, v, 255],
                        "gray_alpha" => [v, v, v, y as u8],
                        "palette" => match (x / 8 + y / 8) % 4 {
                            0 => [255, 0, 0, 255],
                            1 => [0, 255, 0, 255],
                            2 => [0, 0, 255, 255],
                            _ => [255, 255, 0, 255],
                        },
                        "transparent" => [30, 60, 90, 0],
                        _ => [seed as u8, (seed >> 8) as u8, (seed >> 16) as u8, 255],
                    });
                }
            }
            let bitmap = Bitmap::new(256, 256, pixels).expect("valid bitmap");
            for (compression_name, compression) in
                [("none", Compression::None), ("normal", Compression::Normal)]
            {
                let options = EncodeOptions {
                    compression,
                    ..EncodeOptions::default()
                };
                let encoded = image::encode_with_options(&bitmap, Format::Tga, options)
                    .expect("encode bitmap");
                assert_eq!(
                    image::decode(&encoded).expect("decode bitmap").pixels(),
                    bitmap.data()
                );
                eprintln!("TGA {name}/{compression_name}: {} bytes", encoded.len());
                group.bench_function(&format!("{name}_{compression_name}"), |b| {
                    b.iter(|| {
                        black_box(
                            image::encode_with_options(black_box(&bitmap), Format::Tga, options)
                                .expect("encode bitmap"),
                        )
                    });
                });
            }
        }
        group.finish();
    }
    #[cfg(not(feature = "tga"))]
    let _ = c;
}

criterion_group!(benches, decode, encode, encode_tga);
criterion_main!(benches);
