/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

/// A point in device-independent pixels
#[derive(Debug, Clone, Copy)]
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
#[derive(Debug, Clone, Copy)]
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
