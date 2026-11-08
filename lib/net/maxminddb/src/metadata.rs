/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::collections::HashMap;

use crate::MaxMindDbError;
use crate::decoder::decode_value_at;

/// Metadata about the MaxMind DB file.
#[derive(Debug)]
pub struct Metadata {
    /// Number of nodes in the search tree.
    pub node_count: u32,
    /// Number of bits in each search tree record.
    pub record_size: u16,
    /// IP version: 4 or 6.
    pub ip_version: u16,
    /// Type of data stored in the database.
    pub database_type: String,
    /// Locale codes for localized data.
    pub languages: Vec<String>,
    /// Major version of the binary format.
    pub binary_format_major_version: u16,
    /// Minor version of the binary format.
    pub binary_format_minor_version: u16,
    /// Database build timestamp as Unix epoch.
    pub build_epoch: u64,
    /// Descriptions keyed by locale code.
    pub description: HashMap<String, String>,
}

/// The binary marker that separates the data section from the metadata.
pub(crate) const METADATA_MARKER: &[u8] = b"\xab\xcd\xefMaxMind.com";

impl Metadata {
    /// Parse metadata from the raw database bytes.
    pub(crate) fn parse(data: &[u8]) -> Result<(Self, usize), MaxMindDbError> {
        // Find the last occurrence of the metadata marker.
        let marker_offset = data
            .windows(METADATA_MARKER.len())
            .rposition(|w| w == METADATA_MARKER)
            .ok_or_else(|| {
                MaxMindDbError::InvalidDatabase("metadata marker not found".to_string())
            })?;
        let metadata_start = marker_offset + METADATA_MARKER.len();

        // The metadata is itself encoded as a MaxMind DB map in the metadata section.
        let meta_section = &data[metadata_start..];
        let (value, _) = decode_value_at(meta_section, 0)?;

        let map = match value {
            crate::decoder::Value::Map(m) => m,
            _ => {
                return Err(MaxMindDbError::InvalidDatabase(
                    "metadata is not a map".to_string(),
                ));
            }
        };

        let node_count = get_u64(&map, "node_count")? as u32;
        let record_size = get_u64(&map, "record_size")? as u16;
        let ip_version = get_u64(&map, "ip_version")? as u16;
        let database_type = get_string(&map, "database_type")?;
        let binary_format_major_version = get_u64(&map, "binary_format_major_version")? as u16;
        let binary_format_minor_version = get_u64(&map, "binary_format_minor_version")? as u16;
        let build_epoch = get_u64(&map, "build_epoch")?;

        let languages = match map.get("languages") {
            Some(crate::decoder::Value::Array(arr)) => arr
                .iter()
                .filter_map(|v| {
                    if let crate::decoder::Value::Str(s) = v {
                        Some((*s).to_string())
                    } else {
                        None
                    }
                })
                .collect(),
            _ => Vec::new(),
        };

        let description = match map.get("description") {
            Some(crate::decoder::Value::Map(m)) => m
                .iter()
                .filter_map(|(k, v)| {
                    if let crate::decoder::Value::Str(s) = v {
                        Some(((*k).to_string(), (*s).to_string()))
                    } else {
                        None
                    }
                })
                .collect(),
            _ => HashMap::new(),
        };

        Ok((
            Self {
                node_count,
                record_size,
                ip_version,
                database_type,
                languages,
                binary_format_major_version,
                binary_format_minor_version,
                build_epoch,
                description,
            },
            marker_offset,
        ))
    }
}

fn get_u64(
    map: &HashMap<&str, crate::decoder::Value<'_>>,
    key: &str,
) -> Result<u64, MaxMindDbError> {
    match map.get(key) {
        Some(crate::decoder::Value::U64(n)) => Ok(*n),
        Some(crate::decoder::Value::U32(n)) => Ok(u64::from(*n)),
        Some(crate::decoder::Value::U16(n)) => Ok(u64::from(*n)),
        _ => Err(MaxMindDbError::InvalidDatabase(format!(
            "missing or invalid metadata field: {key}"
        ))),
    }
}

fn get_string(
    map: &HashMap<&str, crate::decoder::Value<'_>>,
    key: &str,
) -> Result<String, MaxMindDbError> {
    match map.get(key) {
        Some(crate::decoder::Value::Str(s)) => Ok((*s).to_string()),
        _ => Err(MaxMindDbError::InvalidDatabase(format!(
            "missing or invalid metadata field: {key}"
        ))),
    }
}
