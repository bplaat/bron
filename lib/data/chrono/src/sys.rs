/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Local time zone conversion through the C library.

cfg_select! {
    any(unix, windows) => {
        use core::ffi::c_int;
        use core::mem::MaybeUninit;

        use crate::NaiveDate;
        use crate::consts::{SECS_IN_DAY, SECS_IN_HOUR, SECS_IN_MIN};
        use crate::utils::timestamp_to_ymd;

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

        #[allow(unsafe_code)]
        unsafe extern "C" {
            // NetBSD versions the symbols of its 64-bit `time_t` functions; Windows names them.
            #[cfg_attr(target_os = "netbsd", link_name = "__mktime50")]
            #[cfg_attr(windows, link_name = "_mktime64")]
            fn mktime(tm: *mut Tm) -> TimeT;
            #[cfg(unix)]
            #[cfg_attr(target_os = "netbsd", link_name = "__localtime_r50")]
            fn localtime_r(time: *const TimeT, tm: *mut Tm) -> *mut Tm;
            #[cfg(windows)]
            fn _localtime64_s(tm: *mut Tm, time: *const TimeT) -> c_int;
        }

        /// Converts a wall-clock time, given as seconds since 1970 as if it were UTC, to a Unix
        /// timestamp in the local time zone.
        #[allow(unsafe_code)]
        pub(crate) fn local_to_timestamp(local: i64) -> Option<i64> {
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

        /// Returns the local time zone's offset from UTC in seconds at a Unix timestamp.
        #[allow(unsafe_code)]
        pub(crate) fn utc_offset(timestamp: i64) -> Option<i64> {
            #[allow(
                clippy::unnecessary_fallible_conversions,
                reason = "time_t is 32 bits on some systems"
            )]
            let time = TimeT::try_from(timestamp).ok()?;
            let mut tm = MaybeUninit::<Tm>::uninit();
            // SAFETY: time and tm are valid pointers; the C library fills tm on success.
            #[cfg(unix)]
            let success = unsafe { !localtime_r(&time, tm.as_mut_ptr()).is_null() };
            // SAFETY: time and tm are valid pointers; the C library fills tm on success.
            #[cfg(windows)]
            let success = unsafe { _localtime64_s(tm.as_mut_ptr(), &time) == 0 };
            if !success {
                return None;
            }
            // SAFETY: the C library initialized tm.
            let tm = unsafe { tm.assume_init() };
            let local = NaiveDate::from_ymd_opt(
                u32::try_from(tm.tm_year + 1900).ok()?,
                tm.tm_mon as u32 + 1,
                tm.tm_mday as u32,
            )?
            // Clamp a leap second, which some time zones report as second 60.
            .and_hms_opt(
                tm.tm_hour as u32,
                tm.tm_min as u32,
                tm.tm_sec.min(59) as u32,
            )?;
            Some(local.and_utc().timestamp() - timestamp)
        }
    }
    _ => {
        /// Without a C library the local time zone is UTC.
        pub(crate) const fn local_to_timestamp(local: i64) -> Option<i64> {
            Some(local)
        }

        /// Without a C library the local time zone is UTC.
        pub(crate) const fn utc_offset(_timestamp: i64) -> Option<i64> {
            Some(0)
        }
    }
}
