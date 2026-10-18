/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Local time zone conversion through the C library.

use crate::utils::timestamp_to_ymd;

/// Converts a wall-clock time, given as seconds since 1970 as if it were UTC, to a Unix
/// timestamp in the local time zone.
#[cfg(any(unix, windows))]
#[allow(unsafe_code)]
pub(crate) fn local_to_timestamp(local: i64) -> Option<i64> {
    use core::ffi::c_int;

    use crate::consts::{SECS_IN_DAY, SECS_IN_HOUR, SECS_IN_MIN};

    // The C `struct tm`. Unix C libraries add a UTC offset and zone name after the
    // standard fields; a struct larger than a platform's own is harmless.
    #[repr(C)]
    #[allow(dead_code, reason = "fields are read and written by the C library")]
    struct Tm {
        tm_sec: c_int,
        tm_min: c_int,
        tm_hour: c_int,
        tm_mday: c_int,
        tm_mon: c_int,
        tm_year: c_int,
        tm_wday: c_int,
        tm_yday: c_int,
        tm_isdst: c_int,
        #[cfg(unix)]
        tm_gmtoff: core::ffi::c_long,
        #[cfg(unix)]
        tm_zone: *const core::ffi::c_char,
    }

    // The BSDs use a 64-bit `time_t` on every architecture except 32-bit x86 FreeBSD.
    cfg_select! {
        any(
            windows,
            target_os = "openbsd",
            target_os = "netbsd",
            target_os = "dragonfly",
            all(target_os = "freebsd", not(target_arch = "x86"))
        ) => {
            type TimeT = i64;
        }
        _ => {
            type TimeT = core::ffi::c_long;
        }
    }

    unsafe extern "C" {
        // NetBSD versions the symbol of its 64-bit `time_t` function; Windows names it.
        #[cfg_attr(target_os = "netbsd", link_name = "__mktime50")]
        #[cfg_attr(windows, link_name = "_mktime64")]
        fn mktime(tm: *mut Tm) -> TimeT;
    }

    let (year, month, day) = timestamp_to_ymd(local);
    let day_secs = local.rem_euclid(SECS_IN_DAY);
    let mut tm = Tm {
        tm_sec: (day_secs % SECS_IN_MIN) as c_int,
        tm_min: (day_secs % SECS_IN_HOUR / SECS_IN_MIN) as c_int,
        tm_hour: (day_secs / SECS_IN_HOUR) as c_int,
        tm_mday: day as c_int,
        tm_mon: month as c_int - 1,
        tm_year: c_int::try_from(year).ok()? - 1900,
        tm_wday: 0,
        tm_yday: 0,
        // Let the C library decide whether daylight saving time applies.
        tm_isdst: -1,
        #[cfg(unix)]
        tm_gmtoff: 0,
        #[cfg(unix)]
        tm_zone: core::ptr::null(),
    };
    // SAFETY: tm is a valid, initialized struct tm owned by this call.
    let timestamp = unsafe { mktime(&mut tm) };
    #[allow(
        clippy::useless_conversion,
        reason = "time_t is 32 bits on some systems"
    )]
    (timestamp != -1).then(|| i64::from(timestamp))
}

/// Without a C library the local time zone is UTC.
#[cfg(not(any(unix, windows)))]
pub(crate) const fn local_to_timestamp(local: i64) -> Option<i64> {
    Some(local)
}
