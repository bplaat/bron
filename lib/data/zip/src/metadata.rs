/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::{ZipError, read_u16_le, read_u32_le};

pub(crate) fn extra_timestamp(mut extra: &[u8]) -> Result<Option<SystemTime>, ZipError> {
    let mut timestamp = None;
    while !extra.is_empty() {
        if extra.len() < 4 {
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
#[cfg(any(unix, windows))]
#[allow(unsafe_code)]
pub(crate) fn dos_timestamp(date: u16, time: u16) -> Option<SystemTime> {
    let year = i32::from(date >> 9) + 1980;
    let month = i32::from((date >> 5) & 15);
    let day = i32::from(date & 31);
    let hour = i32::from(time >> 11);
    let minute = i32::from((time >> 5) & 63);
    let second = i32::from(time & 31) * 2;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => return None,
    };
    if !(1..=days).contains(&day) || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    // SAFETY: All-zero bytes form a valid C tm; every calendar input is set below.
    let mut calendar: libc::tm = unsafe { std::mem::zeroed() };
    calendar.tm_year = year - 1900;
    calendar.tm_mon = month - 1;
    calendar.tm_mday = day;
    calendar.tm_hour = hour;
    calendar.tm_min = minute;
    calendar.tm_sec = second;
    calendar.tm_isdst = -1;
    #[cfg(unix)]
    // SAFETY: calendar points to a valid, initialized tm owned by this call.
    let seconds = unsafe { libc::mktime(&mut calendar) } as i64;
    #[cfg(windows)]
    let seconds = {
        unsafe extern "C" {
            fn _mktime64(calendar: *mut libc::tm) -> i64;
        }
        // SAFETY: calendar points to a valid, initialized tm owned by this call.
        unsafe { _mktime64(&mut calendar) }
    };
    if seconds == -1 {
        None
    } else {
        unix_time(seconds)
    }
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn dos_timestamp(_date: u16, _time: u16) -> Option<SystemTime> {
    None
}
