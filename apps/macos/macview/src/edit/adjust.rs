/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Light and color adjustments, blur, sharpen and vignette effects and preset filters.

use super::{Canvas, Mask, Selection, blur, par_bands, smoothstep};

// Rec. 709 luma weights.
const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

// The blur standard deviation of the unsharp mask in full-resolution pixels, and its minimum.
const SHARPEN_SIGMA: f64 = 1.5;
const SHARPEN_MIN_SIGMA: f64 = 0.75;

// The unsharp mask gain at full sharpen strength.
const SHARPEN_GAIN: f32 = 2.0;

/// Light, color and effect settings; all zero/false is the identity.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Adjustments {
    /// -1..=1, maps to -2..+2 stops (in linear light).
    pub(crate) exposure: f32,
    /// -1..=1, midtone curve.
    pub(crate) brightness: f32,
    /// -1..=1.
    pub(crate) contrast: f32,
    /// -1..=1, affects bright tones (negative recovers highlights).
    pub(crate) highlights: f32,
    /// -1..=1, affects dark tones (positive lifts shadows).
    pub(crate) shadows: f32,
    /// -1..=1 (-1 = gray).
    pub(crate) saturation: f32,
    /// -1..=1 (weighted toward less saturated colors).
    pub(crate) vibrance: f32,
    /// -1..=1 (negative cooler/blue, positive warmer/orange).
    pub(crate) temperature: f32,
    /// -1..=1 (negative green, positive magenta).
    pub(crate) tint: f32,
    /// Converts to Rec. 709 luma.
    pub(crate) grayscale: bool,
    /// Inverts the colors.
    pub(crate) invert: bool,
    /// Gaussian radius in full-resolution pixels, 0..=100.
    pub(crate) blur: f32,
    /// Unsharp-mask strength 0..=1 (radius around 1.5 full-resolution pixels).
    pub(crate) sharpen: f32,
    /// -1..=1, positive darkens edges, negative lightens.
    pub(crate) vignette: f32,
}

impl Adjustments {
    /// Returns whether these settings leave an image unchanged.
    pub(crate) fn is_identity(&self) -> bool {
        *self == Self::default()
    }
}

/// Preset color filters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Preset {
    /// Punchy colors and contrast.
    Vivid,
    /// Warmer tones.
    Warm,
    /// Cooler tones.
    Cool,
    /// Faded, low contrast look.
    Fade,
    /// Neutral black and white.
    Mono,
    /// High contrast black and white.
    Noir,
    /// Brown toned black and white.
    Sepia,
    /// Warm, faded colors with dark corners.
    Vintage,
}

impl Preset {
    /// Every preset in display order.
    pub(crate) const ALL: [Preset; 8] = [
        Preset::Vivid,
        Preset::Warm,
        Preset::Cool,
        Preset::Fade,
        Preset::Mono,
        Preset::Noir,
        Preset::Sepia,
        Preset::Vintage,
    ];

    /// Returns the display name.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Preset::Vivid => "Vivid",
            Preset::Warm => "Warm",
            Preset::Cool => "Cool",
            Preset::Fade => "Fade",
            Preset::Mono => "Mono",
            Preset::Noir => "Noir",
            Preset::Sepia => "Sepia",
            Preset::Vintage => "Vintage",
        }
    }

    /// Returns the adjustments that make up this preset.
    pub(crate) fn adjustments(self) -> Adjustments {
        let base = Adjustments::default();
        match self {
            Preset::Vivid => Adjustments {
                contrast: 0.15,
                saturation: 0.25,
                vibrance: 0.3,
                ..base
            },
            Preset::Warm => Adjustments {
                temperature: 0.35,
                tint: 0.05,
                vibrance: 0.1,
                ..base
            },
            Preset::Cool => Adjustments {
                temperature: -0.35,
                brightness: 0.03,
                ..base
            },
            Preset::Fade => Adjustments {
                brightness: 0.05,
                contrast: -0.3,
                shadows: 0.3,
                saturation: -0.25,
                ..base
            },
            Preset::Mono => Adjustments {
                contrast: 0.1,
                grayscale: true,
                ..base
            },
            Preset::Noir => Adjustments {
                contrast: 0.5,
                highlights: -0.1,
                shadows: -0.2,
                grayscale: true,
                vignette: 0.3,
                ..base
            },
            Preset::Sepia => Adjustments {
                contrast: 0.05,
                grayscale: true,
                temperature: 0.45,
                tint: 0.1,
                ..base
            },
            Preset::Vintage => Adjustments {
                contrast: -0.15,
                shadows: 0.2,
                saturation: -0.3,
                temperature: 0.25,
                tint: 0.05,
                vignette: 0.35,
                ..base
            },
        }
    }
}

/// Applies adjustments to the whole canvas, or blended through the selection's soft mask.
/// `scale` is the canvas size relative to the full-resolution image (1.0 for the real image, < 1
/// for a preview proxy) and scales pixel radii (blur/sharpen) so previews match the final result.
/// The vignette is centered on the selection bounds when there is a selection, otherwise the image.
pub(crate) fn apply(
    canvas: &Canvas,
    adjustments: &Adjustments,
    selection: Option<&Selection>,
    scale: f64,
) -> Canvas {
    if adjustments.is_identity() {
        return canvas.clone();
    }
    let effects = Effects::new(adjustments, scale);
    let Some(selection) = selection else {
        let bounds = [0.0, 0.0, f64::from(canvas.width), f64::from(canvas.height)];
        return effects.process(canvas.clone(), bounds);
    };
    let Some(mask) = Mask::new(selection, canvas.width, canvas.height) else {
        return canvas.clone();
    };

    // Process the mask bounds plus the blur reach, then blend the result through the mask
    let margin = effects.margin();
    let left = mask.x.saturating_sub(margin);
    let top = mask.y.saturating_sub(margin);
    let right = (mask.x + mask.width)
        .saturating_add(margin)
        .min(canvas.width);
    let bottom = (mask.y + mask.height)
        .saturating_add(margin)
        .min(canvas.height);
    let bounds = [
        selection.x - f64::from(left),
        selection.y - f64::from(top),
        selection.width,
        selection.height,
    ];
    let processed = effects.process(canvas.region(left, top, right - left, bottom - top), bounds);

    let mut result = canvas.clone();
    let stride = canvas.width as usize * 4;
    let processed_stride = processed.width as usize * 4;
    for y in mask.y..mask.y + mask.height {
        let row = &mut result.pixels[y as usize * stride..][..stride];
        let new_row = &processed.pixels[(y - top) as usize * processed_stride..];
        for x in mask.x..mask.x + mask.width {
            let coverage = mask.at(x, y);
            if coverage <= 0.0 {
                continue;
            }
            let new = &new_row[(x - left) as usize * 4..][..4];
            for (old, &new) in row[x as usize * 4..][..4].iter_mut().zip(new) {
                *old =
                    (f32::from(*old) + (f32::from(new) - f32::from(*old)) * coverage).round() as u8;
            }
        }
    }
    result
}

// The resolved adjustment work for one canvas scale.
struct Effects {
    color: Option<ColorOps>,
    blur_sigma: f64,
    sharpen_sigma: f64,
    sharpen: f32,
    vignette: f32,
}

impl Effects {
    // Resolves adjustments, scaling pixel radii by the canvas scale.
    fn new(adjustments: &Adjustments, scale: f64) -> Effects {
        let identity = Adjustments::default();
        let color_only = Adjustments {
            blur: 0.0,
            sharpen: 0.0,
            vignette: 0.0,
            ..*adjustments
        };
        Effects {
            color: (color_only != identity).then(|| ColorOps::new(adjustments)),
            blur_sigma: f64::from(adjustments.blur) * scale,
            sharpen_sigma: (SHARPEN_SIGMA * scale).max(SHARPEN_MIN_SIGMA),
            sharpen: adjustments.sharpen,
            vignette: adjustments.vignette,
        }
    }

    // Returns how far outside a region the effects read pixels.
    fn margin(&self) -> u32 {
        let sharpen = if self.sharpen > 0.0 {
            blur::margin(self.sharpen_sigma)
        } else {
            0
        };
        blur::margin(self.blur_sigma) + sharpen
    }

    // Applies all effects to a canvas, centering the vignette on `bounds` (x, y, width, height).
    fn process(&self, mut canvas: Canvas, bounds: [f64; 4]) -> Canvas {
        if let Some(color) = &self.color {
            color.apply(&mut canvas);
        }
        if self.blur_sigma >= 0.5 || self.sharpen > 0.0 {
            canvas = self.filter(&canvas);
        }
        if self.vignette != 0.0 {
            vignette(&mut canvas, bounds, self.vignette);
        }
        canvas
    }

    // Applies blur and sharpen on premultiplied channels.
    fn filter(&self, canvas: &Canvas) -> Canvas {
        let (width, height) = (canvas.width as usize, canvas.height as usize);
        let mut plane = canvas.premultiplied();
        blur::gaussian(&mut plane, width, height, self.blur_sigma);
        if self.sharpen > 0.0 {
            let mut blurred = plane.clone();
            blur::gaussian(&mut blurred, width, height, self.sharpen_sigma);
            let gain = self.sharpen * SHARPEN_GAIN;
            for (pixel, soft) in plane
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(blurred.as_chunks::<4>().0.iter())
            {
                let sharp = |value: u16, soft: u16, max: f32| {
                    let value = f32::from(value);
                    (value + (value - f32::from(soft)) * gain)
                        .clamp(0.0, max)
                        .round() as u16
                };
                pixel[3] = sharp(pixel[3], soft[3], 65025.0);
                let alpha = f32::from(pixel[3]);
                for channel in 0..3 {
                    pixel[channel] = sharp(pixel[channel], soft[channel], alpha);
                }
            }
        }
        Canvas::from_premultiplied(canvas.width, canvas.height, &plane)
    }
}

// Darkens (positive) or lightens (negative) the color toward the corners of `bounds`.
fn vignette(canvas: &mut Canvas, bounds: [f64; 4], amount: f32) {
    let [x, y, width, height] = bounds;
    let (center_x, center_y) = (x + width / 2.0, y + height / 2.0);
    let (radius_x, radius_y) = (
        (width / 2.0).max(1.0) * 2f64.sqrt(),
        (height / 2.0).max(1.0) * 2f64.sqrt(),
    );
    let canvas_width = canvas.width as usize;
    par_bands(&mut canvas.pixels, canvas_width * 4, |first_row, band| {
        for (index, pixel) in band.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let px = (index % canvas_width) as f64 + 0.5;
            let py = (first_row + index / canvas_width) as f64 + 0.5;
            let distance = ((px - center_x) / radius_x).hypot((py - center_y) / radius_y);
            let falloff = smoothstep(0.35, 1.0, distance as f32) * amount.abs() * 0.85;
            for channel in &mut pixel[..3] {
                let value = f32::from(*channel);
                let target = if amount > 0.0 { 0.0 } else { 255.0 };
                *channel = (value + (target - value) * falloff).round() as u8;
            }
        }
    });
}

// Converts an sRGB encoded value to linear light.
fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

// Converts a linear light value to sRGB encoding.
fn linear_to_srgb(value: f32) -> f32 {
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

// Converts a 0..=1 value to a rounded, clamped byte.
fn to_byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

// The per-pixel color pipeline: tone curve LUT, cross-channel mixing, then channel gains.
struct ColorOps {
    curve: [f32; 256],
    highlights: f32,
    shadows: f32,
    saturation: f32,
    vibrance: f32,
    grayscale: bool,
    gains: [f32; 3],
    invert: bool,
}

impl ColorOps {
    // Builds the pipeline, baking exposure, brightness and contrast into a tone curve.
    fn new(adjustments: &Adjustments) -> ColorOps {
        let exposure = (2.0 * adjustments.exposure).exp2();
        let gamma = (-adjustments.brightness).exp2();
        let contrast = 4f32.powf(adjustments.contrast);
        ColorOps {
            curve: std::array::from_fn(|index| {
                let mut value = index as f32 / 255.0;
                if adjustments.exposure != 0.0 {
                    value = linear_to_srgb((srgb_to_linear(value) * exposure).min(1.0));
                }
                if adjustments.brightness != 0.0 {
                    value = value.powf(gamma);
                }
                if adjustments.contrast != 0.0 {
                    value = ((value - 0.5) * contrast + 0.5).clamp(0.0, 1.0);
                }
                value
            }),
            highlights: adjustments.highlights,
            shadows: adjustments.shadows,
            saturation: adjustments.saturation,
            vibrance: adjustments.vibrance,
            grayscale: adjustments.grayscale,
            gains: [
                1.0 + 0.25 * adjustments.temperature + 0.05 * adjustments.tint,
                1.0 - 0.2 * adjustments.tint,
                1.0 - 0.25 * adjustments.temperature + 0.05 * adjustments.tint,
            ],
            invert: adjustments.invert,
        }
    }

    // Returns whether any step mixes the color channels of a pixel.
    fn mixes(&self) -> bool {
        self.highlights != 0.0
            || self.shadows != 0.0
            || self.saturation != 0.0
            || self.vibrance != 0.0
            || self.grayscale
    }

    // Applies the channel gains and inversion and converts to a byte.
    fn finish(&self, channel: usize, value: f32) -> u8 {
        let value = (value * self.gains[channel]).clamp(0.0, 1.0);
        to_byte(if self.invert { 1.0 - value } else { value })
    }

    // Applies the tone-dependent and saturation steps to one pixel.
    fn mix(&self, mut rgb: [f32; 3]) -> [f32; 3] {
        let luma = |rgb: [f32; 3]| rgb.iter().zip(LUMA).map(|(c, w)| c * w).sum::<f32>();
        if self.highlights != 0.0 || self.shadows != 0.0 {
            let tone = luma(rgb);
            let delta = 0.3 * self.highlights * smoothstep(0.25, 1.0, tone)
                + 0.3 * self.shadows * (1.0 - smoothstep(0.0, 0.75, tone));
            rgb = rgb.map(|c| (c + delta).clamp(0.0, 1.0));
        }
        if self.saturation != 0.0 || self.vibrance != 0.0 {
            let gray = luma(rgb);
            let chroma =
                rgb.iter().fold(0f32, |a, &b| a.max(b)) - rgb.iter().fold(1f32, |a, &b| a.min(b));
            let factor = (1.0 + self.saturation) * (1.0 + self.vibrance * (1.0 - chroma));
            rgb = rgb.map(|c| (gray + (c - gray) * factor).clamp(0.0, 1.0));
        }
        if self.grayscale {
            rgb = [luma(rgb); 3];
        }
        rgb
    }

    // Applies the pipeline to every pixel, using byte LUTs when no channels are mixed.
    fn apply(&self, canvas: &mut Canvas) {
        let width = canvas.width as usize;
        if self.mixes() {
            par_bands(&mut canvas.pixels, width * 4, |_, band| {
                for pixel in band.as_chunks_mut::<4>().0 {
                    let rgb = self.mix([0, 1, 2].map(|c| self.curve[usize::from(pixel[c])]));
                    for (channel, value) in rgb.into_iter().enumerate() {
                        pixel[channel] = self.finish(channel, value);
                    }
                }
            });
        } else {
            let luts: [[u8; 256]; 3] = std::array::from_fn(|channel| {
                std::array::from_fn(|index| self.finish(channel, self.curve[index]))
            });
            par_bands(&mut canvas.pixels, width * 4, |_, band| {
                for pixel in band.as_chunks_mut::<4>().0 {
                    for (value, lut) in pixel.iter_mut().zip(&luts) {
                        *value = lut[usize::from(*value)];
                    }
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::SelectionShape;
    use crate::edit::tests::solid;

    // Builds a small canvas with varied colors and alpha.
    fn gradient() -> Canvas {
        let pixels = (0..16 * 12)
            .flat_map(|i| {
                [
                    (i * 7) as u8,
                    (i * 13) as u8,
                    (255 - i) as u8,
                    255 - (i % 3) as u8,
                ]
            })
            .collect();
        Canvas::new(16, 12, pixels)
    }

    #[test]
    fn identity_is_unchanged() {
        let canvas = gradient();
        assert_eq!(apply(&canvas, &Adjustments::default(), None, 1.0), canvas);
        let tiny = Adjustments {
            contrast: 1e-9,
            ..Adjustments::default()
        };
        assert_eq!(apply(&canvas, &tiny, None, 1.0), canvas);
    }

    #[test]
    fn invert_twice_is_identity() {
        let canvas = gradient();
        let invert = Adjustments {
            invert: true,
            ..Adjustments::default()
        };
        let once = apply(&canvas, &invert, None, 1.0);
        assert_ne!(once, canvas);
        assert_eq!(apply(&once, &invert, None, 1.0), canvas);
    }

    #[test]
    fn grayscale_is_gray() {
        let canvas = gradient();
        let gray = Adjustments {
            grayscale: true,
            saturation: 0.5,
            ..Adjustments::default()
        };
        let result = apply(&canvas, &gray, None, 1.0);
        for (pixel, original) in result
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .zip(canvas.pixels.as_chunks::<4>().0.iter())
        {
            assert!(pixel[0] == pixel[1] && pixel[1] == pixel[2]);
            assert_eq!(pixel[3], original[3]);
        }
    }

    #[test]
    fn presets_change_colors() {
        let canvas = gradient();
        for preset in Preset::ALL {
            assert!(!preset.name().is_empty());
            assert_ne!(apply(&canvas, &preset.adjustments(), None, 1.0), canvas);
        }
    }

    #[test]
    fn selection_limits_adjustment() {
        let canvas = solid(40, 40, [100, 150, 200, 255]);
        let adjustments = Adjustments {
            invert: true,
            blur: 3.0,
            vignette: 0.5,
            ..Adjustments::default()
        };
        let selection = Selection {
            shape: SelectionShape::Ellipse,
            x: 5.0,
            y: 5.0,
            width: 10.0,
            height: 10.0,
            feather: 2.0,
        };
        let result = apply(&canvas, &adjustments, Some(&selection), 1.0);
        let at = |x: usize, y: usize| &result.pixels[(y * 40 + x) * 4..][..4];
        assert_eq!(at(30, 30), [100, 150, 200, 255]);
        assert_eq!(at(0, 0), [100, 150, 200, 255]);
        assert_eq!(at(10, 10), [155, 105, 55, 255]);
    }

    #[test]
    fn blur_keeps_flat_and_alpha() {
        let canvas = solid(20, 10, [30, 60, 90, 255]);
        let blur = Adjustments {
            blur: 4.0,
            sharpen: 0.5,
            ..Adjustments::default()
        };
        assert_eq!(apply(&canvas, &blur, None, 1.0), canvas);

        let mut striped = gradient();
        for pixel in striped.pixels.as_chunks_mut::<4>().0 {
            pixel[3] = 255;
        }
        let result = apply(&striped, &blur, None, 0.5);
        assert!(result.is_opaque());
        assert_ne!(result, striped);
    }

    #[test]
    fn exposure_brightens() {
        let canvas = solid(4, 4, [64, 64, 64, 255]);
        let brighter = Adjustments {
            exposure: 0.5,
            ..Adjustments::default()
        };
        let result = apply(&canvas, &brighter, None, 1.0);
        assert!(result.pixels[0] > 64 && result.pixels[3] == 255);
    }
}
