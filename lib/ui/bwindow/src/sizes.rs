/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

/// A point in device-independent pixels
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LogicalPoint {
    /// X coordinate
    pub x: f32,
    /// Y coordinate
    pub y: f32,
}

impl LogicalPoint {
    /// Create new logical point
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// A size in device-independent pixels
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogicalSize {
    /// Width
    pub width: f32,
    /// Height
    pub height: f32,
}

impl LogicalSize {
    /// Create new logical size
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

/// A rectangle in device-independent pixels
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LogicalRect {
    /// Left edge
    pub x: f32,
    /// Top edge
    pub y: f32,
    /// Width
    pub width: f32,
    /// Height
    pub height: f32,
}

impl LogicalRect {
    /// Create new logical rectangle
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// The size in physical pixels at a scale, rounded to whole pixels.
    pub(crate) fn physical_size(self, scale: f32) -> (u32, u32) {
        (
            (self.width * scale).round().max(0.0) as u32,
            (self.height * scale).round().max(0.0) as u32,
        )
    }
}

/// Distances from each window edge in device-independent pixels
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct LogicalInsets {
    /// Top inset
    pub top: f32,
    /// Right inset
    pub right: f32,
    /// Bottom inset
    pub bottom: f32,
    /// Left inset
    pub left: f32,
}

impl LogicalInsets {
    /// Create new logical insets
    pub const fn new(top: f32, right: f32, bottom: f32, left: f32) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }
}
