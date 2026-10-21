/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [maxminddb](https://crates.io/crates/maxminddb) crate.
//!
//! Reads MaxMind DB format files (`.mmdb`), including GeoIP2 and GeoLite2 databases.
//!
//! # Quick start
//!
//! ```no_run
//! use maxminddb::{Reader, geoip2};
//! use std::net::IpAddr;
//!
//! // Map the database so it is paged in on demand instead of copied into memory
//! let reader = Reader::open_mmap("/path/to/GeoLite2-City.mmdb").unwrap();
//! let ip: IpAddr = "89.160.20.128".parse().unwrap();
//! let result = reader.lookup(ip).unwrap();
//! if let Some(city) = result.decode::<geoip2::City>().unwrap() {
//!     println!("Country: {:?}", city.country.iso_code);
//! }
//! ```

pub use crate::error::MaxMindDbError;
pub use crate::metadata::Metadata;
pub use crate::mmap::Mmap;
pub use crate::reader::{LookupResult, Reader};

pub mod geoip2;

mod decoder;
mod error;
mod metadata;
mod mmap;
mod reader;

// MARK: Tests
#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use super::*;

    fn test_db_path() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/GeoLite2-City-Test.mmdb")
    }

    #[test]
    fn test_open_readfile() {
        let reader = Reader::open_readfile(test_db_path()).expect("failed to open database");
        assert!(reader.metadata().node_count > 0);
        assert_eq!(reader.metadata().ip_version, 6);
        assert_eq!(reader.metadata().record_size, 28);
        assert!(reader.metadata().database_type.contains("City"));
    }

    #[test]
    fn test_lookup_known_ipv4() {
        let reader = Reader::open_readfile(test_db_path()).expect("failed to open database");
        let ip: IpAddr = "89.160.20.128".parse().unwrap();
        let result = reader.lookup(ip).expect("lookup failed");
        assert!(result.has_data(), "expected data for 89.160.20.128");
        let city = result
            .decode::<geoip2::City>()
            .expect("decode failed")
            .expect("expected Some");
        assert_eq!(city.country.iso_code.as_deref(), Some("SE"));
    }

    #[test]
    fn test_lookup_unknown_ip() {
        let reader = Reader::open_readfile(test_db_path()).expect("failed to open database");
        // 192.0.2.0/24 is reserved documentation range, not in the DB.
        let ip: IpAddr = "192.0.2.1".parse().unwrap();
        let result = reader.lookup(ip).expect("lookup failed");
        assert!(!result.has_data());
        let decoded = result.decode::<geoip2::City>().expect("decode failed");
        assert!(decoded.is_none());
    }

    #[test]
    fn test_lookup_city_fields() {
        let reader = Reader::open_readfile(test_db_path()).expect("failed to open database");
        let ip: IpAddr = "81.2.69.142".parse().unwrap();
        let result = reader.lookup(ip).expect("lookup failed");
        if result.has_data() {
            let city = result
                .decode::<geoip2::City>()
                .expect("decode failed")
                .expect("expected Some");
            // Verify we can access nested fields without panic
            let _ = city.city.names.english;
            let _ = city.country.iso_code;
            let _ = city.location.latitude;
            let _ = city.location.longitude;
            let _ = city.location.time_zone;
            let _ = city.continent.code;
        }
    }

    #[test]
    fn test_open_mmap_matches_readfile() {
        let mapped = Reader::open_mmap(test_db_path()).expect("failed to map database");
        let read = Reader::open_readfile(test_db_path()).expect("failed to open database");
        assert_eq!(mapped.metadata().node_count, read.metadata().node_count);
        for ip in ["89.160.20.128", "81.2.69.142", "192.0.2.1"] {
            let ip: IpAddr = ip.parse().unwrap();
            let decode = |result: LookupResult| result.decode::<geoip2::City>().unwrap();
            assert_eq!(
                decode(mapped.lookup(ip).unwrap()).map(|city| city.country.iso_code),
                decode(read.lookup(ip).unwrap()).map(|city| city.country.iso_code)
            );
        }
    }

    #[test]
    fn test_from_source() {
        let buf = std::fs::read(test_db_path()).expect("failed to read file");
        let reader = Reader::from_source(buf).expect("failed to parse database");
        assert!(reader.metadata().node_count > 0);
    }

    #[test]
    fn test_metadata_build_epoch() {
        let reader = Reader::open_readfile(test_db_path()).expect("failed to open database");
        // Build epoch should be a reasonable Unix timestamp (after 2010).
        assert!(reader.metadata().build_epoch > 1_262_304_000);
    }
}
