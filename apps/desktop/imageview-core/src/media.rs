/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::sync::Arc;
use std::time::Duration;

use image::{ColorSpace, LoopCount};
use vector::VectorImage;

/// Decoded media, ready to draw with a canvas.
#[derive(Clone)]
pub enum Media {
    /// A still or animated bitmap.
    Raster(Raster),
    /// A vector display list.
    Vector(VectorImage),
}

impl Media {
    /// Returns the natural size of the media in display units.
    pub fn size(&self) -> (f64, f64) {
        match self {
            Self::Raster(raster) => (f64::from(raster.width), f64::from(raster.height)),
            Self::Vector(vector) => (vector.size().width, vector.size().height),
        }
    }

    /// Returns the frame shown after `elapsed` of playback, and how long until the next frame
    /// is shown when the media is still playing.
    pub fn frame_at(&self, elapsed: Duration) -> (usize, Option<Duration>) {
        let Self::Raster(raster) = self else {
            return (0, None);
        };
        if raster.frames.len() < 2 {
            return (0, None);
        }
        let delays: Vec<Duration> = raster.frames.iter().map(Frame::shown_for).collect();
        let period: Duration = delays.iter().sum();
        let plays = match raster.loop_count {
            LoopCount::Infinite => None,
            LoopCount::Finite(plays) => Some(plays.max(1)),
        };
        if plays.is_some_and(|plays| elapsed >= period * plays) {
            return (raster.frames.len() - 1, None);
        }
        let mut time = Duration::from_nanos((elapsed.as_nanos() % period.as_nanos()) as u64);
        for (index, delay) in delays.iter().enumerate() {
            if time < *delay {
                return (index, Some(*delay - time));
            }
            time -= *delay;
        }
        (0, Some(delays[0]))
    }
}

/// A still or animated bitmap with premultiplied BGRA8 frames in sRGB.
#[derive(Clone)]
pub struct Raster {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// One or more fully composited frames.
    pub frames: Vec<Frame>,
    /// The total number of animation plays.
    pub loop_count: LoopCount,
}

/// One frame of a bitmap.
#[derive(Clone)]
pub struct Frame {
    /// Row-major premultiplied BGRA8 pixels, which canvases draw directly.
    pub pixels: Arc<[u8]>,
    /// How long the frame is shown.
    pub delay: Duration,
}

// Browsers show frames with nearly no delay for longer, which many animations expect.
const MINIMUM_DELAY: Duration = Duration::from_millis(20);
const DEFAULT_DELAY: Duration = Duration::from_millis(100);

impl Frame {
    /// How long the frame is shown during playback.
    pub fn shown_for(&self) -> Duration {
        if self.delay < MINIMUM_DELAY {
            DEFAULT_DELAY
        } else {
            self.delay
        }
    }
}

/// Decodes an image with the local decoders.
pub fn decode_media(bytes: &[u8]) -> Result<Media, String> {
    if vector::vector_format(bytes).is_some() {
        return vector::decode_vector(bytes)
            .map(Media::Vector)
            .map_err(|error| error.to_string());
    }
    let decoded = image::decode(bytes).map_err(|error| error.to_string())?;
    let linear = decoded.color_space() == ColorSpace::LinearSrgb;
    let frames = decoded
        .frames()
        .iter()
        .map(|frame| Frame {
            pixels: premultiplied_bgra(frame.pixels(), linear).into(),
            delay: frame.delay(),
        })
        .collect();
    Ok(Media::Raster(Raster {
        width: decoded.width(),
        height: decoded.height(),
        frames,
        loop_count: decoded.loop_count(),
    }))
}

// Converts straight-alpha RGBA8 pixels to premultiplied BGRA8 pixels in sRGB.
fn premultiplied_bgra(pixels: &[u8], linear: bool) -> Vec<u8> {
    let encode: [u8; 256] = std::array::from_fn(|value| {
        if linear {
            linear_to_srgb(value as u8)
        } else {
            value as u8
        }
    });
    let mut output = Vec::with_capacity(pixels.len());
    for pixel in pixels.as_chunks::<4>().0 {
        let alpha = u16::from(pixel[3]);
        let premultiply =
            |value: u8| ((u16::from(encode[usize::from(value)]) * alpha + 127) / 255) as u8;
        output.extend_from_slice(&[
            premultiply(pixel[2]),
            premultiply(pixel[1]),
            premultiply(pixel[0]),
            pixel[3],
        ]);
    }
    output
}

fn linear_to_srgb(value: u8) -> u8 {
    let linear = f64::from(value) / 255.0;
    let srgb = if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    (srgb * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

    #[test]
    fn pixels_become_premultiplied_bgra() {
        assert_eq!(
            premultiplied_bgra(&[255, 128, 0, 128, 10, 20, 30, 255], false),
            [0, 64, 128, 128, 30, 20, 10, 255]
        );
    }

    #[test]
    fn linear_pixels_are_encoded_as_srgb() {
        assert_eq!(
            premultiplied_bgra(&[0, 255, 55, 255], true),
            [128, 255, 0, 255]
        );
    }

    #[test]
    fn rasters_and_animations_decode() {
        let still = decode_media(&std::fs::read(format!("{FIXTURES}/rgb.qoi")).unwrap()).unwrap();
        let Media::Raster(still) = still else {
            panic!("expected a raster");
        };
        assert_eq!(still.frames.len(), 1);
        assert_eq!(
            still.frames[0].pixels.len(),
            (still.width * still.height * 4) as usize
        );

        let animated =
            decode_media(&std::fs::read(format!("{FIXTURES}/animated.gif")).unwrap()).unwrap();
        let Media::Raster(animated) = animated else {
            panic!("expected a raster");
        };
        assert!(animated.frames.len() > 1);
    }

    #[test]
    fn high_bit_depth_images_decode() {
        for name in ["16bit.png", "16bit-apng.png", "animated.png", "dib.ico"] {
            let bytes = std::fs::read(format!("{FIXTURES}/{name}")).unwrap();
            decode_media(&bytes).unwrap_or_else(|error| panic!("{name}: {error}"));
        }
    }

    fn animation(delays: &[u64], loop_count: LoopCount) -> Media {
        Media::Raster(Raster {
            width: 1,
            height: 1,
            frames: delays
                .iter()
                .map(|delay| Frame {
                    pixels: Arc::from([0; 4]),
                    delay: Duration::from_millis(*delay),
                })
                .collect(),
            loop_count,
        })
    }

    #[test]
    fn animations_step_through_their_frames_and_loop() {
        let media = animation(&[100, 200], LoopCount::Infinite);
        let at = |millis| media.frame_at(Duration::from_millis(millis));
        assert_eq!(at(0), (0, Some(Duration::from_millis(100))));
        assert_eq!(at(150), (1, Some(Duration::from_millis(150))));
        assert_eq!(at(320), (0, Some(Duration::from_millis(80))));
    }

    #[test]
    fn finite_animations_stop_on_their_last_frame() {
        let media = animation(&[100, 100], LoopCount::Finite(2));
        assert_eq!(media.frame_at(Duration::from_millis(399)).0, 1);
        assert_eq!(media.frame_at(Duration::from_millis(400)), (1, None));
    }

    #[test]
    fn frames_without_delay_use_the_browser_default() {
        let media = animation(&[0, 10], LoopCount::Infinite);
        assert_eq!(media.frame_at(Duration::from_millis(150)).0, 1);
        assert_eq!(
            animation(&[50], LoopCount::Infinite).frame_at(Duration::ZERO),
            (0, None)
        );
    }

    #[test]
    fn vectors_decode() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"/>"#;
        let media = decode_media(svg).unwrap();
        assert!(matches!(media, Media::Vector(_)));
        assert_eq!(media.size(), (20.0, 10.0));
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(decode_media(b"not an image").is_err());
    }
}
