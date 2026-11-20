/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Mapping the decoders' typed metadata to the Windows property schema.

#![allow(non_snake_case)]

use image::{ImageInfo, Metadata, MetadataDateTime, MetadataGroup, MetadataKey, MetadataValue};

use super::{BOOL, HRESULT, KEYS, PropertyKey, c_void, image_key};

#[repr(C)]
#[derive(Default)]
struct SystemTime {
    year: u16,
    month: u16,
    weekday: u16,
    day: u16,
    hour: u16,
    minute: u16,
    second: u16,
    milliseconds: u16,
}

#[link(name = "propsys")]
unsafe extern "system" {
    fn PSGetPropertyKeyFromName(name: *const u16, key: *mut PropertyKey) -> HRESULT;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn SystemTimeToFileTime(time: *const SystemTime, ticks: *mut u64) -> BOOL;
    fn TzSpecificLocalTimeToSystemTime(
        zone: *const c_void,
        local: *const SystemTime,
        utc: *mut SystemTime,
    ) -> BOOL;
}

pub(super) enum Value {
    Text(String),
    Texts(Vec<String>),
    Unsigned(u32),
    Short(u16),
    SignedShort(i16),
    Double(f64),
    FileTime(u64),
}

pub(super) fn named_key(name: &str) -> Option<PropertyKey> {
    let name: Vec<u16> = name.encode_utf16().chain([0]).collect();
    let mut key = image_key(0);
    // SAFETY: The name is terminated and the key is writable.
    (unsafe { PSGetPropertyKeyFromName(name.as_ptr(), &mut key) } >= 0).then_some(key)
}

fn file_time(date: MetadataDateTime) -> Option<u64> {
    if date.nanosecond >= 1_000_000_000 {
        return None;
    }
    let local = SystemTime {
        year: date.year,
        month: date.month,
        day: date.day,
        hour: date.hour,
        minute: date.minute,
        second: date.second,
        ..SystemTime::default()
    };
    let mut utc = SystemTime::default();
    let mut ticks = 0;
    // SAFETY: Time structures and output buffers are valid. Windows validates the date.
    unsafe {
        let time = if date.offset_minutes.is_some() {
            &local
        } else {
            if TzSpecificLocalTimeToSystemTime(std::ptr::null(), &local, &mut utc) == 0 {
                return None;
            }
            &utc
        };
        if SystemTimeToFileTime(time, &mut ticks) == 0 {
            return None;
        }
    }
    ticks
        .checked_add_signed(-i64::from(date.offset_minutes.unwrap_or(0)) * 60 * 10_000_000)?
        .checked_add(u64::from(date.nanosecond / 100))
}

pub(super) fn image_properties(info: ImageInfo) -> Vec<(PropertyKey, Value)> {
    use MetadataKey::*;
    let mut values = vec![
        (
            KEYS[0],
            Value::Text(format!("{} x {}", info.width, info.height)),
        ),
        (KEYS[1], Value::Unsigned(info.width)),
        (KEYS[2], Value::Unsigned(info.height)),
    ];
    let mut add = |name: &str, value: Value| {
        if let Some(key) = named_key(name)
            && !values.iter().any(|(existing, _)| *existing == key)
        {
            values.push((key, value));
        }
    };
    if let Some(depth) = info.bit_depth.filter(|depth| *depth > 0) {
        add("System.Image.BitDepth", Value::Unsigned(depth));
    }
    // Native EXIF takes precedence over text/XMP fallbacks and container defaults such as JFIF.
    let properties: Vec<_> = [
        MetadataGroup::Exif,
        MetadataGroup::Text,
        MetadataGroup::Xmp,
        MetadataGroup::Image,
    ]
    .into_iter()
    .flat_map(|group| {
        info.metadata
            .iter()
            .filter(move |entry| entry.group() == group)
            .flat_map(Metadata::properties)
    })
    .collect();
    let find = |key| {
        properties
            .iter()
            .find(|property| property.key == key)
            .map(|property| &property.value)
    };
    for (key, name) in [
        (CameraMake, "System.Photo.CameraManufacturer"),
        (CameraModel, "System.Photo.CameraModel"),
        (Copyright, "System.Copyright"),
        (Title, "System.Title"),
        (Description, "System.Comment"),
    ] {
        if let Some(MetadataValue::Text(value)) = find(key)
            .filter(|value| matches!(value, MetadataValue::Text(text) if !text.trim().is_empty()))
        {
            add(name, Value::Text(value.clone()));
        }
    }
    // An author is a string vector in the Windows schema, even for a single name.
    for group in [MetadataGroup::Exif, MetadataGroup::Text, MetadataGroup::Xmp] {
        let mut authors = Vec::new();
        for property in info
            .metadata
            .iter()
            .filter(|entry| entry.group() == group)
            .flat_map(Metadata::properties)
        {
            if property.key == Author
                && let MetadataValue::Text(author) = &property.value
                && !author.trim().is_empty()
                && !authors.contains(author)
            {
                authors.push(author.clone());
            }
        }
        if !authors.is_empty() {
            add("System.Author", Value::Texts(authors));
            break;
        }
    }
    if let Some(MetadataValue::DateTime(date)) = find(DateTaken)
        && let Some(ticks) = file_time(*date)
    {
        add("System.Photo.DateTaken", Value::FileTime(ticks));
    }
    for (key, name) in [
        (Iso, "System.Photo.ISOSpeed"),
        (Orientation, "System.Photo.Orientation"),
    ] {
        if let Some(MetadataValue::Integer(value)) = find(key)
            && let Ok(value) = u16::try_from(*value)
            && value > 0
            && (key != Orientation || value <= 8)
        {
            add(name, Value::Short(value));
        }
    }
    for (key, name) in [
        (ExposureTime, "System.Photo.ExposureTime"),
        (FNumber, "System.Photo.FNumber"),
        (Aperture, "System.Photo.Aperture"),
        (ExposureBias, "System.Photo.ExposureBias"),
        (FocalLength, "System.Photo.FocalLength"),
    ] {
        if let Some(value) = find(key).and_then(MetadataValue::number)
            && value.is_finite()
            && (matches!(key, ExposureBias | Aperture) || value > 0.0)
        {
            add(name, Value::Double(value));
        }
    }
    // Keep a resolution and its units together; an EXIF unit must not reinterpret JFIF DPI.
    for group in [
        MetadataGroup::Exif,
        MetadataGroup::Image,
        MetadataGroup::Xmp,
    ] {
        let properties: Vec<_> = info
            .metadata
            .iter()
            .filter(|entry| entry.group() == group)
            .flat_map(Metadata::properties)
            .collect();
        let number = |key| {
            properties
                .iter()
                .find(|property| property.key == key)
                .and_then(|property| property.value.number())
        };
        let unit = number(ResolutionUnit).unwrap_or(2.0);
        if unit != 2.0 && unit != 3.0 {
            continue;
        }
        let mut has_resolution = false;
        for (key, name) in [
            (HorizontalResolution, "System.Image.HorizontalResolution"),
            (VerticalResolution, "System.Image.VerticalResolution"),
        ] {
            if let Some(value) = number(key)
                && value.is_finite()
                && value > 0.0
            {
                add(
                    name,
                    Value::Double(value * if unit == 3.0 { 2.54 } else { 1.0 }),
                );
                has_resolution = true;
            }
        }
        if has_resolution {
            add("System.Image.ResolutionUnit", Value::SignedShort(2));
            break;
        }
    }
    values
}
