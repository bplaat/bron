/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jiff::civil::DateTime;
use jiff::tz::TimeZone;

use crate::{ZipError, read_u16_le, read_u32_le};

pub(crate) fn extra_timestamp(mut extra: &[u8]) -> Result<Option<SystemTime>, ZipError> {
    let mut timestamp = None;
    while !extra.is_empty() {
        if extra.len() < 4 {
            // Tools like aapt2 and zipalign pad the extra field with zeros for alignment.
            if extra.iter().all(|byte| *byte == 0) {
                break;
            }
            return Err(ZipError::InvalidZip("truncated extra field header"));
        }
        let id = read_u16_le(extra, 0);
        let size = usize::from(read_u16_le(extra, 2));
        let data = extra
            .get(4..4 + size)
            .ok_or(ZipError::InvalidZip("truncated extra field"))?;
        if id == 0x5455 {
            let flags = *data
                .first()
                .ok_or(ZipError::InvalidZip("empty timestamp field"))?;
            if flags & 1 != 0 {
                if data.len() < 5 {
                    return Err(ZipError::InvalidZip("truncated Unix timestamp"));
                }
                let seconds = i64::from(read_u32_le(data, 1) as i32);
                timestamp = unix_time(seconds);
            }
        } else if id == 0x000a {
            let mut tags = data
                .get(4..)
                .ok_or(ZipError::InvalidZip("truncated NTFS field"))?;
            while !tags.is_empty() {
                if tags.len() < 4 {
                    return Err(ZipError::InvalidZip("truncated NTFS tag"));
                }
                let tag = read_u16_le(tags, 0);
                let size = usize::from(read_u16_le(tags, 2));
                let value = tags
                    .get(4..4 + size)
                    .ok_or(ZipError::InvalidZip("truncated NTFS tag"))?;
                if tag == 1 {
                    if value.len() != 24 {
                        return Err(ZipError::InvalidZip("invalid NTFS timestamps"));
                    }
                    let ticks = u64::from_le_bytes(value[..8].try_into().expect("timestamp bytes"));
                    let epoch = 116_444_736_000_000_000u64;
                    let duration = Duration::new(
                        ticks.abs_diff(epoch) / 10_000_000,
                        (ticks.abs_diff(epoch) % 10_000_000) as u32 * 100,
                    );
                    let ntfs = if ticks >= epoch {
                        UNIX_EPOCH.checked_add(duration)
                    } else {
                        UNIX_EPOCH.checked_sub(duration)
                    };
                    if timestamp.is_none() {
                        timestamp = ntfs;
                    }
                }
                tags = &tags[4 + size..];
            }
        }
        extra = &extra[4 + size..];
    }
    Ok(timestamp)
}

pub(crate) fn unix_time(seconds: i64) -> Option<SystemTime> {
    let duration = Duration::from_secs(seconds.unsigned_abs());
    if seconds >= 0 {
        UNIX_EPOCH.checked_add(duration)
    } else {
        UNIX_EPOCH.checked_sub(duration)
    }
}

// DOS timestamps are local wall-clock times, without a stored time zone.
pub(crate) fn dos_timestamp(date: u16, time: u16) -> Option<SystemTime> {
    let local = DateTime::new(
        (date >> 9) as i16 + 1980,
        ((date >> 5) & 15) as i8,
        (date & 31) as i8,
        (time >> 11) as i8,
        ((time >> 5) & 63) as i8,
        (time & 31) as i8 * 2,
        0,
    )
    .ok()?;
    let timestamp = local
        .to_zoned_datetime(TimeZone::system())
        .ok()?
        .timestamp();
    unix_time(timestamp.as_second())
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_zero_alignment_padding_but_rejects_truncated_headers() {
        assert!(matches!(extra_timestamp(&[0]), Ok(None)));
        assert!(matches!(extra_timestamp(&[0, 0, 0]), Ok(None)));
        assert!(extra_timestamp(&[1, 0]).is_err());
    }
}
