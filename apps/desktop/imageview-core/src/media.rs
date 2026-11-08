/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::sync::Arc;
use std::time::Duration;

use bcanvas::premultiplied_bgra;
use image::LoopCount;
use vector::VectorImage;

use crate::Info;

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

/// The pages of an image: each size of an icon, or the image itself.
pub struct Pages {
    /// The pages in file order.
    pub pages: Vec<Media>,
    /// The index of the largest page, which is shown first.
    pub largest: usize,
    /// The format and metadata of the file.
    pub info: Info,
}

impl Pages {
    /// The one page of media without a file, like an edited picture.
    pub fn single(media: Media) -> Self {
        Self {
            pages: vec![media],
            largest: 0,
            info: Info::default(),
        }
    }
}

/// Decodes an image with the local decoders, the largest size of an icon.
pub fn decode_media(bytes: &[u8]) -> Result<Media, String> {
    let Pages {
        mut pages, largest, ..
    } = decode_pages(bytes)?;
    Ok(pages.swap_remove(largest))
}

/// Decodes every page of an image with the local decoders, like each size of an icon.
pub fn decode_pages(bytes: &[u8]) -> Result<Pages, String> {
    if let Some(format) = vector::vector_format(bytes) {
        return vector::decode_vector(bytes)
            .map(|vector| Pages {
                pages: vec![Media::Vector(vector)],
                largest: 0,
                info: Info {
                    format: match format {
                        vector::VectorFormat::Svg => "SVG",
                        vector::VectorFormat::Svgz => "Compressed SVG",
                        vector::VectorFormat::TinyVg => "TinyVG",
                    },
                    metadata: Vec::new(),
                },
            })
            .map_err(|error| error.to_string());
    }
    let image = image::decode(bytes).map_err(|error| error.to_string())?;
    let info = Info {
        format: image.format().name(),
        metadata: image.metadata().to_vec(),
    };
    // An animation is one page, the frames of a still image are the sizes of an icon.
    if image.is_animated() {
        return Ok(Pages {
            pages: vec![raster(image.frames(), image.loop_count())],
            largest: 0,
            info,
        });
    }
    let largest = image.largest_frame_index();
    Ok(Pages {
        pages: image
            .frames()
            .chunks(1)
            .map(|frame| raster(frame, image.loop_count()))
            .collect(),
        largest,
        info,
    })
}

// A bitmap of frames of the same size.
fn raster(frames: &[image::Frame], loop_count: LoopCount) -> Media {
    let bitmap = frames[0].bitmap();
    Media::Raster(Raster {
        width: bitmap.width(),
        height: bitmap.height(),
        frames: frames
            .iter()
            .map(|frame| Frame {
                pixels: premultiplied_bgra(frame.pixels()),
                delay: frame.delay(),
            })
            .collect(),
        loop_count,
    })
}

// MARK: Tests

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

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
    fn icons_decode_every_page_and_find_the_largest() {
        let bytes = std::fs::read(format!("{FIXTURES}/rgb.icns")).unwrap();
        let Pages {
            pages,
            largest,
            info,
        } = decode_pages(&bytes).expect("ICNS pages");
        assert_eq!(info.format, "ICNS");
        let sizes: Vec<_> = pages.iter().map(Media::size).collect();
        assert_eq!(
            sizes,
            [
                (512.0, 512.0),
                (32.0, 32.0),
                (256.0, 256.0),
                (16.0, 16.0),
                (128.0, 128.0)
            ]
        );
        assert_eq!(largest, 0);
        assert!(pages.iter().all(Media::is_still));
        let ico = include_bytes!("../../imageview/meta/windows/file-types/jpeg.ico");
        let Pages { pages, largest, .. } = decode_pages(ico).expect("ICO pages");
        assert_eq!(pages.len(), 10);
        assert_eq!(pages[largest].size(), (256.0, 256.0));
        assert_eq!(decode_media(ico).expect("ICO").size(), (256.0, 256.0));
        let animated = std::fs::read(format!("{FIXTURES}/animated.gif")).unwrap();
        assert_eq!(decode_pages(&animated).expect("GIF pages").pages.len(), 1);
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
