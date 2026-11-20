/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Format-independent properties alongside readable image metadata.

use alloc::string::String;

/// The meaning of a machine-readable metadata value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MetadataKey {
    /// Camera manufacturer.
    CameraMake,
    /// Camera model.
    CameraModel,
    /// Original capture date and time.
    DateTaken,
    /// Author or artist.
    Author,
    /// Copyright notice.
    Copyright,
    /// Document title.
    Title,
    /// Image description.
    Description,
    /// Exposure duration in seconds.
    ExposureTime,
    /// Lens f-number.
    FNumber,
    /// Aperture in APEX units.
    Aperture,
    /// Exposure compensation in EV.
    ExposureBias,
    /// Focal length in millimetres.
    FocalLength,
    /// ISO sensitivity.
    Iso,
    /// Horizontal pixels per resolution unit.
    HorizontalResolution,
    /// Vertical pixels per resolution unit.
    VerticalResolution,
    /// TIFF resolution unit: 1 means unspecified, 2 inches, 3 centimetres.
    ResolutionUnit,
    /// EXIF orientation, from 1 through 8.
    Orientation,
}

/// A capture date, retaining the camera's wall clock and optional UTC offset.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MetadataDateTime {
    /// Gregorian year.
    pub year: u16,
    /// Month from 1 through 12.
    pub month: u16,
    /// Day of the month.
    pub day: u16,
    /// Hour from 0 through 23.
    pub hour: u16,
    /// Minute from 0 through 59.
    pub minute: u16,
    /// Second from 0 through 59.
    pub second: u16,
    /// Fractional second in nanoseconds, from 0 through 999,999,999.
    pub nanosecond: u32,
    /// Minutes east of UTC, when the file specifies an offset.
    pub offset_minutes: Option<i16>,
}

/// A metadata value in its original units and precision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MetadataValue {
    /// A text value.
    Text(String),
    /// A signed or unsigned integer fitting in 64 bits.
    Integer(i64),
    /// An exact fraction with a nonzero, positive denominator.
    Rational {
        /// Signed numerator.
        numerator: i64,
        /// Positive denominator.
        denominator: u32,
    },
    /// A capture date and time.
    DateTime(MetadataDateTime),
}

impl MetadataValue {
    /// Returns a numeric value, preserving fractions until this conversion.
    pub fn number(&self) -> Option<f64> {
        match self {
            Self::Integer(value) => Some(*value as f64),
            Self::Rational {
                numerator,
                denominator,
            } if *denominator != 0 => Some(*numerator as f64 / f64::from(*denominator)),
            _ => None,
        }
    }
}

/// A typed property that can be consumed by an operating system or another application.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetadataProperty {
    /// The property meaning.
    pub key: MetadataKey,
    /// The original typed value.
    pub value: MetadataValue,
}
