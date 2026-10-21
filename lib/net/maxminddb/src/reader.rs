/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::net::IpAddr;
use std::path::Path;
use std::sync::OnceLock;

use serde::de::DeserializeOwned;

use crate::decoder::{Value, decode_value_at, from_value};
use crate::error::MaxMindDbError;
use crate::metadata::Metadata;
use crate::mmap::Mmap;

// MARK: Reader

/// A reader for the MaxMind DB format.
pub struct Reader<S: AsRef<[u8]>> {
    // Metadata about the database.
    metadata: Metadata,
    buf: S,
    data_offset: usize,
    ipv4_start: OnceLock<usize>,
}

impl Reader<Vec<u8>> {
    /// Open a MaxMind DB database file by loading it into memory.
    pub fn open_readfile(path: impl AsRef<Path>) -> Result<Self, MaxMindDbError> {
        let buf = std::fs::read(path)?;
        Self::from_source(buf)
    }
}

impl Reader<Mmap> {
    /// Open a MaxMind DB database file by memory mapping it, so it is paged in on demand.
    pub fn open_mmap(path: impl AsRef<Path>) -> Result<Self, MaxMindDbError> {
        Self::from_source(Mmap::open(path)?)
    }
}

impl<S: AsRef<[u8]>> Reader<S> {
    /// Open a MaxMind DB database from anything that implements `AsRef<[u8]>`.
    pub fn from_source(buf: S) -> Result<Self, MaxMindDbError> {
        let (metadata, data_section_start) = Metadata::parse(buf.as_ref())?;
        // The data section starts after the search tree and its 16-byte separator.
        let tree_size = ((metadata.record_size as usize * 2) / 8) * metadata.node_count as usize;
        let data_offset = tree_size + 16;

        if data_section_start < data_offset {
            return Err(MaxMindDbError::InvalidDatabase(
                "database too small for declared node count".to_string(),
            ));
        }

        Ok(Self {
            metadata,
            buf,
            data_offset,
            ipv4_start: OnceLock::new(),
        })
    }

    /// Returns database metadata.
    #[inline]
    #[allow(clippy::missing_const_for_fn)]
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// Look up an IP address in the database.
    pub fn lookup(&self, addr: IpAddr) -> Result<LookupResult, MaxMindDbError> {
        let data = self.buf.as_ref();
        let node_count = self.metadata.node_count as usize;

        // Convert IP address to bits.
        let (bits, bit_count) = match addr {
            IpAddr::V4(ipv4) => {
                let octets = ipv4.octets();
                let n = u32::from_be_bytes(octets);
                (n as u128, 32usize)
            }
            IpAddr::V6(ipv6) => {
                let octets = ipv6.octets();
                let n = u128::from_be_bytes(octets);
                // For IPv4-in-IPv6 (::ffff:x.x.x.x), use the IPv4 path if DB is IPv4-only.
                (n, 128)
            }
        };

        // For IPv4 lookup in an IPv6 database, we start at the node pointed to
        // by following 96 zero bits from node 0 (IPv4-in-IPv6 mapping).
        let (start_node, effective_bits, bit_value) =
            if self.metadata.ip_version == 6 && bit_count == 32 {
                let root = if let Some(&node) = self.ipv4_start.get() {
                    Some(node)
                } else {
                    let root = self.ipv4_start_node(data)?;
                    if let Some(node) = root {
                        _ = self.ipv4_start.set(node);
                    }
                    root
                };
                let Some(node) = root else {
                    return Ok(LookupResult { data_value: None });
                };
                (node, 32usize, bits)
            } else {
                (0, bit_count, bits)
            };

        // Walk the search tree.
        let mut node = start_node;
        for i in 0..effective_bits {
            let bit = (bit_value >> (effective_bits - 1 - i)) & 1;
            let record = self.read_record(data, node, bit != 0)?;
            if record == node_count {
                // No data for this IP.
                return Ok(LookupResult { data_value: None });
            }
            if record > node_count {
                // This is a pointer into the data section.
                // Pointer values within the data section are relative to the data section start.
                let data_record_offset = record - node_count - 16;
                let data_section = &data[self.data_offset..];
                let (value, _) = decode_value_at(data_section, data_record_offset)?;
                return Ok(LookupResult {
                    data_value: Some(value),
                });
            }
            node = record;
        }

        Ok(LookupResult { data_value: None })
    }

    fn ipv4_start_node(&self, data: &[u8]) -> Result<Option<usize>, MaxMindDbError> {
        let node_count = self.metadata.node_count as usize;
        let mut node = 0;
        for _ in 0..96 {
            let record = self.read_record(data, node, false)?;
            if record >= node_count {
                return Ok(None);
            }
            node = record;
        }
        Ok(Some(node))
    }

    fn read_record(&self, data: &[u8], node: usize, right: bool) -> Result<usize, MaxMindDbError> {
        let record_size = self.metadata.record_size as usize;
        let node_size = (record_size * 2) / 8;
        let node_offset = node * node_size;

        if node_offset + node_size > data.len() {
            return Err(MaxMindDbError::InvalidDatabase(
                "node offset out of bounds".to_string(),
            ));
        }

        let node_bytes = &data[node_offset..node_offset + node_size];

        match record_size {
            24 => {
                // Each node is 6 bytes: left record (3 bytes), right record (3 bytes).
                if right {
                    Ok((node_bytes[3] as usize) * 65536
                        + (node_bytes[4] as usize) * 256
                        + node_bytes[5] as usize)
                } else {
                    Ok((node_bytes[0] as usize) * 65536
                        + (node_bytes[1] as usize) * 256
                        + node_bytes[2] as usize)
                }
            }
            28 => {
                // Each node is 7 bytes.
                // Left:  node[0..3] with high nibble from node[3] high bits
                // Right: node[4..7] with high nibble from node[3] low bits
                if right {
                    let high = (node_bytes[3] & 0x0f) as usize;
                    Ok(high * 16_777_216
                        + (node_bytes[4] as usize) * 65536
                        + (node_bytes[5] as usize) * 256
                        + node_bytes[6] as usize)
                } else {
                    let high = ((node_bytes[3] & 0xf0) >> 4) as usize;
                    Ok(high * 16_777_216
                        + (node_bytes[0] as usize) * 65536
                        + (node_bytes[1] as usize) * 256
                        + node_bytes[2] as usize)
                }
            }
            32 => {
                // Each node is 8 bytes: left record (4 bytes), right record (4 bytes).
                if right {
                    Ok((node_bytes[4] as usize) * 16_777_216
                        + (node_bytes[5] as usize) * 65536
                        + (node_bytes[6] as usize) * 256
                        + node_bytes[7] as usize)
                } else {
                    Ok((node_bytes[0] as usize) * 16_777_216
                        + (node_bytes[1] as usize) * 65536
                        + (node_bytes[2] as usize) * 256
                        + node_bytes[3] as usize)
                }
            }
            _ => Err(MaxMindDbError::InvalidDatabase(format!(
                "unsupported record size: {record_size}"
            ))),
        }
    }
}

// MARK: LookupResult

/// The result of looking up an IP address in a MaxMind DB.
pub struct LookupResult {
    pub(crate) data_value: Option<Value>,
}

impl LookupResult {
    /// Returns `true` if the database contains data for the looked-up IP.
    pub const fn has_data(&self) -> bool {
        self.data_value.is_some()
    }

    /// Decode the record into a strongly-typed value.
    ///
    /// Returns `Ok(None)` if the IP address was not found in the database.
    pub fn decode<T: DeserializeOwned>(&self) -> Result<Option<T>, MaxMindDbError> {
        match &self.data_value {
            None => Ok(None),
            Some(v) => from_value::<T>(v).map(Some),
        }
    }
}
