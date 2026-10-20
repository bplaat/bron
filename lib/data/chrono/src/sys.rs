/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Local time zone conversion through the C library on Unix and Win32 on Windows.

cfg_select! {
    unix => {
        use core::ffi::c_int;
        use core::mem::MaybeUninit;

        use crate::NaiveDate;
        use crate::consts::{SECS_IN_DAY, SECS_IN_HOUR, SECS_IN_MIN};
        use crate::utils::timestamp_to_ymd;

        // The C `struct tm`, including the UTC offset and zone name Unix C libraries add.
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
            tm_gmtoff: core::ffi::c_long,
            tm_zone: *const core::ffi::c_char,
        }

        // The BSDs use a 64-bit `time_t` on every architecture except 32-bit x86 FreeBSD.
        cfg_select! {
            any(
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
            // NetBSD versions the symbols of its 64-bit `time_t` functions.
            #[cfg_attr(target_os = "netbsd", link_name = "__mktime50")]
            fn mktime(tm: *mut Tm) -> TimeT;
            #[cfg_attr(target_os = "netbsd", link_name = "__localtime_r50")]
            fn localtime_r(time: *const TimeT, tm: *mut Tm) -> *mut Tm;
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
                tm_gmtoff: 0,
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
            if unsafe { localtime_r(&time, tm.as_mut_ptr()) }.is_null() {
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
    windows => {
        use core::mem::MaybeUninit;

        use crate::NaiveDate;
        use crate::consts::{SECS_IN_DAY, SECS_IN_HOUR, SECS_IN_MIN};
        use crate::utils::timestamp_to_ymd;

        #[repr(C)]
        #[derive(Default)]
        #[allow(dead_code, reason = "fields are read and written by Win32")]
        struct SystemTime {
            year: u16,
            month: u16,
            day_of_week: u16,
            day: u16,
            hour: u16,
            minute: u16,
            second: u16,
            milliseconds: u16,
        }

        #[repr(C)]
        #[allow(dead_code, reason = "fields are read and written by Win32")]
        struct DynamicTimeZoneInformation {
            bias: i32,
            standard_name: [u16; 32],
            standard_date: SystemTime,
            standard_bias: i32,
            daylight_name: [u16; 32],
            daylight_date: SystemTime,
            daylight_bias: i32,
            time_zone_key_name: [u16; 128],
            dynamic_daylight_time_disabled: u8,
        }

        const TIME_ZONE_ID_INVALID: u32 = u32::MAX;

        type ConvertFn = unsafe extern "system" fn(
            *const DynamicTimeZoneInformation,
            *const SystemTime,
            *mut SystemTime,
        ) -> i32;

        #[allow(unsafe_code)]
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetDynamicTimeZoneInformation(tz: *mut DynamicTimeZoneInformation) -> u32;
            fn SystemTimeToTzSpecificLocalTimeEx(
                tz: *const DynamicTimeZoneInformation,
                utc: *const SystemTime,
                local: *mut SystemTime,
            ) -> i32;
            fn TzSpecificLocalTimeToSystemTimeEx(
                tz: *const DynamicTimeZoneInformation,
                local: *const SystemTime,
                utc: *mut SystemTime,
            ) -> i32;
        }

        /// Converts seconds since 1970 between UTC and local time with the current time zone and
        /// its per-year daylight saving time rules.
        #[allow(unsafe_code)]
        fn convert(secs: i64, convert: ConvertFn) -> Option<i64> {
            let mut tz = MaybeUninit::<DynamicTimeZoneInformation>::uninit();
            // SAFETY: tz is a valid pointer; Win32 fills it on success.
            if unsafe { GetDynamicTimeZoneInformation(tz.as_mut_ptr()) } == TIME_ZONE_ID_INVALID {
                return None;
            }

            let (year, month, day) = timestamp_to_ymd(secs);
            let day_secs = secs.rem_euclid(SECS_IN_DAY);
            let input = SystemTime {
                year: u16::try_from(year).ok()?,
                month: month as u16,
                day: day as u16,
                hour: (day_secs / SECS_IN_HOUR) as u16,
                minute: (day_secs % SECS_IN_HOUR / SECS_IN_MIN) as u16,
                second: (day_secs % SECS_IN_MIN) as u16,
                ..Default::default()
            };
            let mut output = SystemTime::default();
            // SAFETY: Win32 initialized tz, and input and output are valid SYSTEMTIME structs.
            if unsafe { convert(tz.as_ptr(), &input, &mut output) } == 0 {
                return None;
            }

            let output = NaiveDate::from_ymd_opt(
                output.year.into(),
                output.month.into(),
                output.day.into(),
            )?
            .and_hms_opt(
                output.hour.into(),
                output.minute.into(),
                output.second.into(),
            )?;
            Some(output.and_utc().timestamp())
        }

        /// Converts a wall-clock time, given as seconds since 1970 as if it were UTC, to a Unix
        /// timestamp in the local time zone.
        pub(crate) fn local_to_timestamp(local: i64) -> Option<i64> {
            convert(local, TzSpecificLocalTimeToSystemTimeEx)
        }

        /// Returns the local time zone's offset from UTC in seconds at a Unix timestamp.
        pub(crate) fn utc_offset(timestamp: i64) -> Option<i64> {
            Some(convert(timestamp, SystemTimeToTzSpecificLocalTimeEx)? - timestamp)
        }
    }
    _ => {
        compile_error!("Unsupported platform");
    }
}
