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

        use crate::civil::DateTime;

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

        /// Converts a wall-clock date and time in the local time zone to a Unix timestamp.
        /// The system can normalize skipped times, so the caller validates the result.
        #[allow(unsafe_code)]
        pub(crate) fn local_to_timestamp(datetime: DateTime) -> Option<i64> {
            let date = datetime.date();
            let mut tm = Tm {
                tm_sec: datetime.second().into(),
                tm_min: datetime.minute().into(),
                tm_hour: datetime.hour().into(),
                tm_mday: date.day().into(),
                tm_mon: c_int::from(date.month()) - 1,
                tm_year: c_int::from(date.year()) - 1900,
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
            // An error returns -1, which the caller rejects as it doesn't match the wall-clock time.
            Some(i64::from(timestamp))
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
            #[allow(clippy::useless_conversion, reason = "long is 32 bits on some systems")]
            Some(i64::from(unsafe { tm.assume_init() }.tm_gmtoff))
        }
    }
    windows => {
        use core::mem::MaybeUninit;

        use crate::civil::DateTime;

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

        #[cfg(not(target_vendor = "winxp"))]
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
        /// Windows XP loads the Vista/7 time zone functions at runtime and falls back to its older
        /// ones, which omit the last two fields and use the current daylight saving rules.
        #[cfg(target_vendor = "winxp")]
        macro_rules! compat_fn {
            ($($name:ident($($arg:ident: $type:ty),*) -> $return:ty = $fallback:ident;)*) => {$(
                #[allow(unsafe_code, non_snake_case, reason = "Win32 function names")]
                unsafe extern "system" fn $name($($arg: $type),*) -> $return {
                    use core::ffi::c_void;

                    #[link(name = "kernel32")]
                    unsafe extern "system" {
                        fn $fallback($($arg: $type),*) -> $return;
                        fn GetModuleHandleA(name: *const u8) -> *mut c_void;
                        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *const c_void;
                    }

                    type Function = unsafe extern "system" fn($($type),*) -> $return;
                    static FUNCTION: std::sync::OnceLock<Function> = std::sync::OnceLock::new();
                    let function = FUNCTION.get_or_init(|| {
                        // SAFETY: kernel32 is loaded in every process, the export has this
                        // signature.
                        unsafe {
                            let address = GetProcAddress(
                                GetModuleHandleA(c"kernel32.dll".as_ptr().cast()),
                                concat!(stringify!($name), "\0").as_ptr(),
                            );
                            if address.is_null() {
                                $fallback
                            } else {
                                core::mem::transmute::<*const c_void, Function>(address)
                            }
                        }
                    });
                    // SAFETY: the caller passes the arguments of the function.
                    unsafe { function($($arg),*) }
                }
            )*};
        }
        #[cfg(target_vendor = "winxp")]
        compat_fn! {
            GetDynamicTimeZoneInformation(tz: *mut DynamicTimeZoneInformation) -> u32
                = GetTimeZoneInformation;
            SystemTimeToTzSpecificLocalTimeEx(
                tz: *const DynamicTimeZoneInformation,
                utc: *const SystemTime,
                local: *mut SystemTime
            ) -> i32 = SystemTimeToTzSpecificLocalTime;
            TzSpecificLocalTimeToSystemTimeEx(
                tz: *const DynamicTimeZoneInformation,
                local: *const SystemTime,
                utc: *mut SystemTime
            ) -> i32 = TzSpecificLocalTimeToSystemTime;
        }

        /// Converts a date and time between UTC and local time with the current time zone and its
        /// per-year daylight saving time rules.
        #[allow(unsafe_code)]
        fn convert(datetime: DateTime, convert: ConvertFn) -> Option<DateTime> {
            let mut tz = MaybeUninit::<DynamicTimeZoneInformation>::uninit();
            // SAFETY: tz is a valid pointer; Win32 fills it on success.
            if unsafe { GetDynamicTimeZoneInformation(tz.as_mut_ptr()) } == TIME_ZONE_ID_INVALID {
                return None;
            }

            let date = datetime.date();
            let input = SystemTime {
                year: u16::try_from(date.year()).ok()?,
                month: date.month() as u16,
                day: date.day() as u16,
                hour: datetime.hour() as u16,
                minute: datetime.minute() as u16,
                second: datetime.second() as u16,
                ..Default::default()
            };
            let mut output = SystemTime::default();
            // SAFETY: Win32 initialized tz, and input and output are valid SYSTEMTIME structs.
            if unsafe { convert(tz.as_ptr(), &input, &mut output) } == 0 {
                return None;
            }
            DateTime::new(
                i16::try_from(output.year).ok()?,
                output.month as i8,
                output.day as i8,
                output.hour as i8,
                output.minute as i8,
                output.second as i8,
                0,
            )
            .ok()
        }

        /// Converts a wall-clock date and time in the local time zone to a Unix timestamp.
        /// The system can normalize skipped times, so the caller validates the result.
        pub(crate) fn local_to_timestamp(datetime: DateTime) -> Option<i64> {
            Some(convert(datetime, TzSpecificLocalTimeToSystemTimeEx)?.to_second())
        }

        /// Returns the local time zone's offset from UTC in seconds at a Unix timestamp.
        pub(crate) fn utc_offset(timestamp: i64) -> Option<i64> {
            let local = convert(
                DateTime::from_second(timestamp),
                SystemTimeToTzSpecificLocalTimeEx,
            )?;
            Some(local.to_second() - timestamp)
        }
    }
    _ => {
        compile_error!("Unsupported platform");
    }
}
