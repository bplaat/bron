/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use super::picture::Picture;

/// The outline of a selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// A rectangle.
    Rectangle,
    /// An ellipse inside a rectangle.
    Ellipse,
}

/// The part of a picture that edits change, with smooth edges.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Selection {
    /// The outline.
    pub shape: Shape,
    /// The left, top, right and bottom edges of the shape in pixels.
    pub edges: [f64; 4],
    /// Whether everything outside the shape is selected instead.
    pub inverted: bool,
}

impl Selection {
    /// The rectangle around the shape in whole pixels inside a picture: its position and size.
    pub fn bounds(&self, width: u32, height: u32) -> (u32, u32, u32, u32) {
        let [left, top, right, bottom] = self.edges;
        let clamp = |value: f64, limit: u32| value.clamp(0.0, f64::from(limit)) as u32;
        let (left, top) = (clamp(left.floor(), width), clamp(top.floor(), height));
        let (right, bottom) = (clamp(right.ceil(), width), clamp(bottom.ceil(), height));
        (
            left.min(width.saturating_sub(1)),
            top.min(height.saturating_sub(1)),
            (right - left).max(1),
            (bottom - top).max(1),
        )
    }

    // How much of the pixel with its center at a position is selected, with edges softened
    // over `feather` pixels.
    fn coverage(&self, x: f64, y: f64, feather: f64) -> f32 {
        let [left, top, right, bottom] = self.edges;
        // The distance inside the outline, negative outside.
        let inside = match self.shape {
            Shape::Rectangle => (x - left).min(right - x).min(y - top).min(bottom - y),
            Shape::Ellipse => {
                let (radius_x, radius_y) = ((right - left) / 2.0, (bottom - top) / 2.0);
                if radius_x <= 0.0 || radius_y <= 0.0 {
                    return if self.inverted { 1.0 } else { 0.0 };
                }
                let (dx, dy) = (x - (left + radius_x), y - (top + radius_y));
                // The implicit function divided by the length of its gradient.
                let value = (dx / radius_x).powi(2) + (dy / radius_y).powi(2) - 1.0;
                let gradient =
                    2.0 * (dx * dx / radius_x.powi(4) + dy * dy / radius_y.powi(4)).sqrt();
                if gradient == 0.0 {
                    f64::INFINITY
                } else {
                    -value / gradient
                }
            }
        };
        let coverage = (inside / feather.max(1.0) + 0.5).clamp(0.0, 1.0) as f32;
        if self.inverted {
            1.0 - coverage
        } else {
            coverage
        }
    }

    /// Mixes an edited picture into the original inside the selection.
    pub fn blend(&self, original: &Picture, edited: &Picture, feather: f64) -> Picture {
        let mut pixels = original.pixels.clone();
        let width = original.width as usize;
        for (index, (pixel, edited)) in pixels
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(edited.pixels.as_chunks::<4>().0)
            .enumerate()
        {
            let (x, y) = ((index % width) as f64 + 0.5, (index / width) as f64 + 0.5);
            let coverage = self.coverage(x, y, feather);
            if coverage == 0.0 {
                continue;
            }
            for (value, edited) in pixel.iter_mut().zip(edited) {
                let mixed = f32::from(*value) + (f32::from(*edited) - f32::from(*value)) * coverage;
                *value = mixed.round() as u8;
            }
        }
        Picture::new(original.width, original.height, pixels)
    }

    /// Mirrors the selected part of a picture inside the rectangle around it, left to right
    /// or top to bottom. An ellipse is mirrored onto itself.
    pub fn flip(&self, picture: &Picture, horizontal: bool) -> Picture {
        let (x, y, width, height) = self.bounds(picture.width, picture.height);
        let region = picture.crop(x, y, width, height);
        let region = if horizontal {
            region.flip_horizontal()
        } else {
            region.flip_vertical()
        };
        let mut flipped = picture.clone();
        flipped.paste(&region, x, y);
        self.blend(picture, &flipped, 0.0)
    }

    /// The selected part of a picture, cut out to the rectangle around it with everything
    /// outside the shape transparent.
    pub fn cut(&self, picture: &Picture) -> Picture {
        let (x, y, width, height) = self.bounds(picture.width, picture.height);
        let (x, y, width, height) = if self.inverted {
            (0, 0, picture.width, picture.height)
        } else {
            (x, y, width, height)
        };
        let mut cut = picture.crop(x, y, width, height);
        for (index, pixel) in cut.pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let column = (index % width as usize) as f64 + f64::from(x) + 0.5;
            let row = (index / width as usize) as f64 + f64::from(y) + 0.5;
            pixel[3] = (f32::from(pixel[3]) * self.coverage(column, row, 0.0)).round() as u8;
        }
        cut
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn white(width: u32, height: u32) -> Picture {
        Picture::new(width, height, vec![255; (width * height * 4) as usize])
    }

    fn black(width: u32, height: u32) -> Picture {
        Picture::new(
            width,
            height,
            [0, 0, 0, 255].repeat((width * height) as usize),
        )
    }

    #[test]
    fn edits_only_change_the_selection() {
        let selection = Selection {
            shape: Shape::Rectangle,
            edges: [2.0, 2.0, 6.0, 6.0],
            inverted: false,
        };
        let blended = selection.blend(&white(8, 8), &black(8, 8), 0.0);
        assert_eq!(blended.pixel(1, 1), [255; 4]);
        assert_eq!(blended.pixel(3, 3), [0, 0, 0, 255]);
        let inverted = Selection {
            inverted: true,
            ..selection
        };
        let blended = inverted.blend(&white(8, 8), &black(8, 8), 0.0);
        assert_eq!(blended.pixel(1, 1), [0, 0, 0, 255]);
        assert_eq!(blended.pixel(3, 3), [255; 4]);
    }

    #[test]
    fn ellipses_have_smooth_and_feathered_edges() {
        let selection = Selection {
            shape: Shape::Ellipse,
            edges: [0.0, 0.0, 10.0, 10.0],
            inverted: false,
        };
        let blended = selection.blend(&white(10, 10), &black(10, 10), 0.0);
        assert_eq!(blended.pixel(5, 5), [0, 0, 0, 255]);
        assert_eq!(blended.pixel(0, 0), [255; 4]);
        // An edge pixel is partly covered.
        let edge = blended.pixel(0, 5)[0];
        assert!(edge > 0 && edge < 255, "{edge}");
        let feathered = selection.blend(&white(10, 10), &black(10, 10), 4.0);
        assert!(feathered.pixel(1, 5)[0] > blended.pixel(1, 5)[0]);
    }

    #[test]
    fn flips_mirror_inside_the_selection() {
        // Columns of increasing gray in an 8x4 picture.
        let pixels = (0..32u8)
            .flat_map(|index| [index % 8 * 30, 0, 0, 255])
            .collect();
        let picture = Picture::new(8, 4, pixels);
        let selection = Selection {
            shape: Shape::Rectangle,
            edges: [2.0, 0.0, 6.0, 4.0],
            inverted: false,
        };
        let flipped = selection.flip(&picture, true);
        assert_eq!(flipped.pixel(2, 1)[0], picture.pixel(5, 1)[0]);
        assert_eq!(flipped.pixel(5, 1)[0], picture.pixel(2, 1)[0]);
        assert_eq!(flipped.pixel(0, 1), picture.pixel(0, 1));
        assert_eq!(flipped.pixel(7, 1), picture.pixel(7, 1));
        let vertical = selection.flip(&picture, false);
        assert_eq!(vertical, picture);
    }

    #[test]
    fn cuts_keep_the_shape() {
        let selection = Selection {
            shape: Shape::Ellipse,
            edges: [2.0, 2.0, 6.0, 8.0],
            inverted: false,
        };
        assert_eq!(selection.bounds(8, 8), (2, 2, 4, 6));
        let cut = selection.cut(&white(8, 8));
        assert_eq!((cut.width, cut.height), (4, 6));
        assert!(cut.pixel(0, 0)[3] < 128);
        assert_eq!(cut.pixel(2, 3)[3], 255);
    }
}
