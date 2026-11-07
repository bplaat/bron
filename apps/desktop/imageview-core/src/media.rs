/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::sync::Arc;
use std::time::Duration;

use image::LoopCount;
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

    /// Whether the media is a still bitmap, which can be edited.
    pub const fn is_still(&self) -> bool {
        matches!(self, Self::Raster(raster) if raster.frames.len() == 1)
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
    // A still image shows its largest frame, like the largest size of an icon.
    let frames = if decoded.is_animated() {
        decoded.frames()
    } else {
        core::slice::from_ref(decoded.largest_frame())
    };
    let frames = frames
        .iter()
        .map(|frame| Frame {
            pixels: premultiplied_bgra(frame.pixels()).into(),
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

// Converts straight-alpha RGBA8 pixels to premultiplied BGRA8 pixels.
pub(crate) fn premultiplied_bgra(pixels: &[u8]) -> Vec<u8> {
    let mut output = pixels.to_vec();
    for pixel in output.as_chunks_mut::<4>().0 {
        let [red, green, blue, alpha] = *pixel;
        // Opaque pixels, like all of a photo, only swap their channels.
        *pixel = if alpha == 255 {
            [blue, green, red, alpha]
        } else {
            let premultiply = |value: u8| ((u16::from(value) * u16::from(alpha) + 127) / 255) as u8;
            [
                premultiply(blue),
                premultiply(green),
                premultiply(red),
                alpha,
            ]
        };
    }
    output
}

// MARK: Tests

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

    #[test]
    fn pixels_become_premultiplied_bgra() {
        assert_eq!(
            premultiplied_bgra(&[255, 128, 0, 128, 10, 20, 30, 255]),
            [0, 64, 128, 128, 30, 20, 10, 255]
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

        // TGA has no signature, so it is detected from its header.
        let tga = decode_media(&std::fs::read(format!("{FIXTURES}/rgb.tga")).unwrap()).unwrap();
        let Media::Raster(tga) = tga else {
            panic!("expected a raster");
        };
        assert_eq!(tga.frames[0].pixels, still.frames[0].pixels);

        let animated =
            decode_media(&std::fs::read(format!("{FIXTURES}/animated.gif")).unwrap()).unwrap();
        let Media::Raster(animated) = animated else {
            panic!("expected a raster");
        };
        assert!(animated.frames.len() > 1);
    }

    #[test]
    fn icns_and_webp_decode_with_shared_codecs() {
        let bytes = include_bytes!("../tests/fixtures/rgb.icns");
        let Media::Raster(icon) = decode_media(bytes).expect("ICNS") else {
            panic!("expected icon raster");
        };
        assert_eq!((icon.width, icon.height), (512, 512));
        assert_eq!(icon.frames.len(), 1);
        assert!(
            icon.frames[0]
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == [96, 48, 16, 128])
        );
        for bytes in [
            include_bytes!("../../../../lib/graphics/image/tests/images/webp/lossless.webp")
                .as_slice(),
            include_bytes!("../../../../lib/graphics/image/tests/images/webp/lossy.webp")
                .as_slice(),
        ] {
            let media = decode_media(bytes).expect("WebP");
            assert_eq!(media.size(), (16.0, 16.0));
            assert!(media.is_still());
        }
    }

    #[test]
    fn jpeg2000_and_compressed_svg_decode() {
        for bytes in [
            include_bytes!("../../../../lib/graphics/image/tests/images/jpeg2000/rgb.jp2")
                .as_slice(),
            include_bytes!("../../../../lib/graphics/image/tests/images/jpeg2000/rgb.j2k")
                .as_slice(),
        ] {
            let media = decode_media(bytes).expect("JPEG 2000 raster");
            assert_eq!(media.size(), (32.0, 32.0));
            assert!(media.is_still());
        }
        let media =
            decode_media(include_bytes!("../../imageview/examples/tiger.svgz")).expect("SVGZ");
        let Media::Vector(image) = media else {
            panic!("expected SVGZ vector");
        };
        assert_eq!(image.format(), vector::VectorFormat::Svgz);
    }

    #[test]
    fn tiff_decodes_with_shared_codecs() {
        for bytes in [
            include_bytes!("../../../../lib/graphics/image/tests/images/tiff/rgba-lzw.tif")
                .as_slice(),
            include_bytes!("../../../../lib/graphics/image/tests/images/tiff/jpeg-ycbcr.tif")
                .as_slice(),
        ] {
            let media = decode_media(bytes).expect("TIFF");
            assert_eq!(media.size(), (37.0, 23.0));
            assert!(media.is_still());
        }
    }

    #[test]
    fn netpbm_decodes_to_display_pixels() {
        for bytes in [
            &b"P1\n1 1\n0"[..],
            &b"P2\n1 1\n15\n15"[..],
            &b"P3\n1 1\n255\n255 255 255"[..],
            &b"P4\n1 1\n\0"[..],
            &b"P5\n1 1\n65535\n\xff\xff"[..],
            &b"P6\n1 1\n255\n\xff\xff\xff"[..],
            &b"P7\nWIDTH 1\nHEIGHT 1\nDEPTH 3\nMAXVAL 255\nTUPLTYPE RGB\nENDHDR\n\xff\xff\xff"[..],
        ] {
            let media = decode_media(bytes).expect("Netpbm");
            assert_eq!(media.size(), (1.0, 1.0));
            assert!(media.is_still());
            let Media::Raster(raster) = media else {
                panic!("expected Netpbm raster");
            };
            assert_eq!(&*raster.frames[0].pixels, &[255, 255, 255, 255]);
        }
        let Media::Raster(raster) = decode_media(
            b"P7\nWIDTH 1\nHEIGHT 1\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n\xff\x80\0\x80",
        ).expect("PAM alpha") else {
            panic!("expected PAM raster");
        };
        assert_eq!(&*raster.frames[0].pixels, &[0, 64, 128, 128]);
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
