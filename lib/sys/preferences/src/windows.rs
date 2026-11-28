/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(unsafe_code)]

//! Registry values under `HKEY_CURRENT_USER\Software`. Vista and newer link RegGetValueW,
//! RegSetKeyValueW and RegDeleteKeyValueW; Windows XP builds load them at runtime and fall back
//! to the open key API when they are missing.

use std::ffi::c_void;
use std::io;
use std::ptr::{null, null_mut};

use crate::Value;

#[allow(clippy::upper_case_acronyms, reason = "Win32 name")]
type HKEY = *mut c_void;
const HKEY_CURRENT_USER: HKEY = 0x8000_0001_usize as HKEY;
const KEY_QUERY_VALUE: u32 = 0x0001;
const KEY_SET_VALUE: u32 = 0x0002;
const REG_SZ: u32 = 1;
const REG_BINARY: u32 = 3;
const REG_DWORD: u32 = 4;
const REG_QWORD: u32 = 11;
// The value types the store writes, never expanded, like the open key fallback
const RRF_RT_PREFERENCES: u32 = 0x0002 | 0x0008 | 0x0010 | 0x0040;
const RRF_NOEXPAND: u32 = 0x1000_0000;
const ERROR_SUCCESS: i32 = 0;
const ERROR_FILE_NOT_FOUND: i32 = 2;
const ERROR_MORE_DATA: i32 = 234;
const ERROR_UNSUPPORTED_TYPE: i32 = 1630;

#[link(name = "advapi32")]
unsafe extern "system" {
    fn RegCreateKeyExW(
        key: HKEY,
        sub_key: *const u16,
        reserved: u32,
        class: *const u16,
        options: u32,
        access: u32,
        security: *const c_void,
        result: *mut HKEY,
        disposition: *mut u32,
    ) -> i32;
    fn RegOpenKeyExW(
        key: HKEY,
        sub_key: *const u16,
        options: u32,
        access: u32,
        result: *mut HKEY,
    ) -> i32;
    fn RegQueryValueExW(
        key: HKEY,
        name: *const u16,
        reserved: *mut u32,
        kind: *mut u32,
        data: *mut u8,
        size: *mut u32,
    ) -> i32;
    fn RegSetValueExW(
        key: HKEY,
        name: *const u16,
        reserved: u32,
        kind: u32,
        data: *const u8,
        size: u32,
    ) -> i32;
    fn RegDeleteValueW(key: HKEY, name: *const u16) -> i32;
    fn RegCloseKey(key: HKEY) -> i32;
}

/// Imports an advapi32 function of Windows Vista: linked directly, or loaded at runtime on
/// Windows XP builds, `None` when Windows XP lacks it.
macro_rules! vista_proc {
    ($name:ident($($arg:ty),* $(,)?) -> $ret:ty) => {{
        type Function = unsafe extern "system" fn($($arg),*) -> $ret;
        #[cfg(not(target_vendor = "winxp"))]
        let function: Option<Function> = {
            #[link(name = "advapi32")]
            unsafe extern "system" {
                fn $name($(_: $arg),*) -> $ret;
            }
            Some($name)
        };
        #[cfg(target_vendor = "winxp")]
        let function: Option<Function> = {
            static FUNCTION: std::sync::OnceLock<Option<Function>> = std::sync::OnceLock::new();
            *FUNCTION.get_or_init(|| {
                let address = load_advapi32_proc(concat!(stringify!($name), "\0"));
                (!address.is_null()).then(|| {
                    // SAFETY: the exported function has this signature.
                    unsafe { std::mem::transmute::<*const c_void, Function>(address) }
                })
            })
        };
        function
    }};
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

/// Encode a string literal as a null-terminated UTF-16 string at compile time.
#[cfg(any(target_vendor = "winxp", test))]
macro_rules! w {
    ($text:literal) => {{
        const TEXT: &str = $text;
        const LEN: usize = {
            let bytes = TEXT.as_bytes();
            let mut len = 1;
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] & 0xc0 != 0x80 {
                    len += if bytes[i] >= 0xf0 { 2 } else { 1 };
                }
                i += 1;
            }
            len
        };
        const WIDE: &[u16] = &{
            let bytes = TEXT.as_bytes();
            let mut result = [0; LEN];
            let mut i = 0;
            let mut j = 0;
            while i < bytes.len() {
                let first = bytes[i];
                let (mut code, extra) = match first {
                    0..=0x7f => (first as u32, 0),
                    0xc0..=0xdf => ((first & 0x1f) as u32, 1),
                    0xe0..=0xef => ((first & 0x0f) as u32, 2),
                    _ => ((first & 0x07) as u32, 3),
                };
                let end = i + extra + 1;
                i += 1;
                while i < end {
                    code = (code << 6) | (bytes[i] & 0x3f) as u32;
                    i += 1;
                }
                assert!(code != 0, "wide string literal contains a null character");
                if code > 0xffff {
                    code -= 0x10000;
                    result[j] = 0xd800 | (code >> 10) as u16;
                    j += 1;
                    result[j] = 0xdc00 | (code & 0x3ff) as u16;
                } else {
                    result[j] = code as u16;
                }
                j += 1;
            }
            result
        };
        WIDE
    }};
}

/// Finds a function of advapi32.dll, which is loaded as the fallback functions link it
#[cfg(target_vendor = "winxp")]
fn load_advapi32_proc(symbol: &str) -> *const c_void {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *const c_void;
    }
    // SAFETY: both names are null-terminated.
    unsafe {
        let module = GetModuleHandleW(w!("advapi32.dll").as_ptr());
        if module.is_null() {
            return null();
        }
        GetProcAddress(module, symbol.as_ptr())
    }
}

pub(super) struct Store {
    sub_key: Vec<u16>,
}

impl Store {
    pub(super) fn new(id: &str) -> io::Result<Self> {
        Ok(Self {
            sub_key: wide(&registry_path(id)),
        })
    }

    pub(super) fn get(&self, key: &str) -> io::Result<Option<Value>> {
        let name = value_name(key)?;
        let value = if let Some(get_value) = vista_proc!(
            RegGetValueW(HKEY, *const u16, *const u16, u32, *mut u32, *mut c_void, *mut u32) -> i32
        ) {
            // SAFETY: the sub key and name are null-terminated and data holds size bytes.
            read(|kind, data, size| unsafe {
                get_value(
                    HKEY_CURRENT_USER,
                    self.sub_key.as_ptr(),
                    name.as_ptr(),
                    RRF_RT_PREFERENCES | RRF_NOEXPAND,
                    kind,
                    data.cast(),
                    size,
                )
            })?
        } else {
            let Some(registry) = self.open(KEY_QUERY_VALUE)? else {
                return Ok(None);
            };
            // SAFETY: the key is open, the name is null-terminated and data holds size bytes.
            read(|kind, data, size| unsafe {
                RegQueryValueExW(registry.0, name.as_ptr(), null_mut(), kind, data, size)
            })?
        };
        value.map(|(kind, data)| decode(kind, &data)).transpose()
    }

    pub(super) fn set(&self, key: &str, value: Value) -> io::Result<()> {
        let name = value_name(key)?;
        let (kind, data) = encode(value);
        if let Some(set_value) = vista_proc!(
            RegSetKeyValueW(HKEY, *const u16, *const u16, u32, *const c_void, u32) -> i32
        ) {
            // SAFETY: the sub key and name are null-terminated and data holds its length.
            return check(unsafe {
                set_value(
                    HKEY_CURRENT_USER,
                    self.sub_key.as_ptr(),
                    name.as_ptr(),
                    kind,
                    data.as_ptr().cast(),
                    data.len() as u32,
                )
            });
        }
        let mut registry = Key(null_mut());
        // SAFETY: the sub key is null-terminated and the result points to a key handle.
        check(unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                self.sub_key.as_ptr(),
                0,
                null(),
                0,
                KEY_SET_VALUE,
                null(),
                &mut registry.0,
                null_mut(),
            )
        })?;
        // SAFETY: the key is open, the name is null-terminated and data holds its length.
        check(unsafe {
            RegSetValueExW(
                registry.0,
                name.as_ptr(),
                0,
                kind,
                data.as_ptr(),
                data.len() as u32,
            )
        })
    }

    pub(super) fn remove(&self, key: &str) -> io::Result<()> {
        let name = value_name(key)?;
        let status = if let Some(delete_value) =
            vista_proc!(RegDeleteKeyValueW(HKEY, *const u16, *const u16) -> i32)
        {
            // SAFETY: the sub key and name are null-terminated.
            unsafe { delete_value(HKEY_CURRENT_USER, self.sub_key.as_ptr(), name.as_ptr()) }
        } else {
            let Some(registry) = self.open(KEY_SET_VALUE)? else {
                return Ok(());
            };
            // SAFETY: the key is open and the name is null-terminated.
            unsafe { RegDeleteValueW(registry.0, name.as_ptr()) }
        };
        // A missing key or value is already removed
        match status {
            ERROR_FILE_NOT_FOUND => Ok(()),
            status => check(status),
        }
    }

    fn open(&self, access: u32) -> io::Result<Option<Key>> {
        let mut registry = Key(null_mut());
        // SAFETY: the sub key is null-terminated and the result points to a key handle.
        match unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                self.sub_key.as_ptr(),
                0,
                access,
                &mut registry.0,
            )
        } {
            ERROR_FILE_NOT_FOUND => Ok(None),
            status => check(status).map(|()| Some(registry)),
        }
    }
}

/// Reads a value with a query that asks its size for a null data pointer, returns its type and
/// data or `None` when it does not exist.
fn read(
    mut query: impl FnMut(*mut u32, *mut u8, *mut u32) -> i32,
) -> io::Result<Option<(u32, Vec<u8>)>> {
    let (mut kind, mut size) = (0, 0);
    let mut data = Vec::new();
    loop {
        data.resize(size as usize, 0);
        let pointer = if data.is_empty() {
            null_mut()
        } else {
            data.as_mut_ptr()
        };
        match query(&mut kind, pointer, &mut size) {
            ERROR_FILE_NOT_FOUND => return Ok(None),
            ERROR_UNSUPPORTED_TYPE => return Err(invalid_data("unsupported preference value")),
            // The value can grow between asking its size and reading it
            ERROR_MORE_DATA => continue,
            ERROR_SUCCESS if size as usize <= data.len() => break,
            ERROR_SUCCESS => continue,
            status => return Err(io::Error::from_raw_os_error(status)),
        }
    }
    data.truncate(size as usize);
    Ok(Some((kind, data)))
}

/// An open registry key, closed on drop.
struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the handle was opened by the registry and is closed once.
            unsafe { RegCloseKey(self.0) };
        }
    }
}

/// `Software\<organization>\<application>` for reverse-DNS IDs, like `nl.bplaat.Reversi` to
/// `Software\bplaat\Reversi`, otherwise `Software\<id>`.
fn registry_path(id: &str) -> String {
    match id.splitn(3, '.').collect::<Vec<_>>()[..] {
        [_, organization, application] if !organization.is_empty() && !application.is_empty() => {
            format!("Software\\{organization}\\{application}")
        }
        _ => format!("Software\\{id}"),
    }
}

fn value_name(key: &str) -> io::Result<Vec<u16>> {
    if key.contains('\0') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "registry value names cannot contain NUL",
        ));
    }
    Ok(wide(key))
}

/// Booleans are DWORDs, integers QWORDs, floats 4 byte binaries and strings REG_SZ, which keeps
/// embedded NUL characters as the stored size includes them. Decoding checks the type and size,
/// as RegQueryValueExW neither checks types nor terminates strings.
fn encode(value: Value) -> (u32, Vec<u8>) {
    match value {
        Value::Bool(value) => (REG_DWORD, u32::from(value).to_le_bytes().to_vec()),
        Value::Integer(value) => (REG_QWORD, value.to_le_bytes().to_vec()),
        Value::Float(value) => (REG_BINARY, value.to_le_bytes().to_vec()),
        Value::String(value) => (
            REG_SZ,
            wide(&value)
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect(),
        ),
    }
}

fn decode(kind: u32, data: &[u8]) -> io::Result<Value> {
    match (kind, data.len()) {
        (REG_DWORD, 4) => match u32::from_le_bytes(data.try_into().expect("4 bytes")) {
            0 => Ok(Value::Bool(false)),
            1 => Ok(Value::Bool(true)),
            _ => Err(invalid_data("DWORD is not a boolean")),
        },
        (REG_QWORD, 8) => Ok(Value::Integer(i64::from_le_bytes(
            data.try_into().expect("8 bytes"),
        ))),
        (REG_BINARY, 4) => {
            let number = f32::from_le_bytes(data.try_into().expect("4 bytes"));
            if number.is_finite() {
                Ok(Value::Float(number))
            } else {
                Err(invalid_data("float is not finite"))
            }
        }
        (REG_SZ, length) if length % 2 == 0 => {
            let mut units = data
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&unit| u16::from_le_bytes(unit))
                .collect::<Vec<_>>();
            // Other writers may leave out the terminating NUL
            if units.last() == Some(&0) {
                units.pop();
            }
            String::from_utf16(&units)
                .map(Value::String)
                .map_err(invalid_data)
        }
        _ => Err(invalid_data("unsupported preference value")),
    }
}

fn check(status: i32) -> io::Result<()> {
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(status))
    }
}

fn invalid_data(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegDeleteKeyW(key: HKEY, sub_key: *const u16) -> i32;
    }

    /// A store in its own registry key, deleted on drop.
    struct TestStore(Store);

    impl TestStore {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = format!(
                "Software\\preferences-tests-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            );
            Self(Store {
                sub_key: wide(&path),
            })
        }
    }

    impl Drop for TestStore {
        fn drop(&mut self) {
            // SAFETY: the sub key is null-terminated.
            unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, self.0.sub_key.as_ptr()) };
        }
    }

    #[test]
    fn round_trip_and_reopen() {
        let test = TestStore::new();
        assert_eq!(test.0.get("missing").expect("get"), None);
        for value in [
            Value::Bool(true),
            Value::Bool(false),
            Value::Integer(i64::MIN),
            Value::Integer(i64::MAX),
            Value::Float(1.0),
            Value::Float(-0.0),
            Value::Float(f32::MAX),
            Value::Float(f32::MIN_POSITIVE),
            Value::String("caf\u{e9}\0\u{1f980}".into()),
            Value::String(String::new()),
        ] {
            test.0.set("key", value.clone()).expect("set");
            let reopened = Store {
                sub_key: test.0.sub_key.clone(),
            };
            assert_eq!(reopened.get("key").expect("get"), Some(value));
        }
        test.0.remove("key").expect("remove");
        test.0.remove("missing").expect("remove missing");
        assert_eq!(test.0.get("key").expect("get"), None);
    }

    #[test]
    fn value_names_are_case_insensitive_and_without_nul() {
        let test = TestStore::new();
        test.0.set("Theme", Value::Bool(true)).expect("set");
        assert_eq!(test.0.get("theme").expect("get"), Some(Value::Bool(true)));
        test.0.remove("THEME").expect("remove");
        assert!(test.0.set("a\0b", Value::Bool(true)).is_err());
        assert!(test.0.get("a\0b").is_err());
    }

    #[test]
    fn missing_key_is_not_created_by_reads() {
        let test = TestStore::new();
        assert_eq!(test.0.get("key").expect("get"), None);
        test.0.remove("key").expect("remove");
        assert!(test.0.open(KEY_QUERY_VALUE).expect("open").is_none());
    }

    #[test]
    fn expandable_strings_are_unsupported() {
        const REG_EXPAND_SZ: u32 = 2;
        let test = TestStore::new();
        test.0.set("other", Value::Bool(true)).expect("create key");
        let registry = test.0.open(KEY_SET_VALUE).expect("open").expect("key");
        let data = w!("%APPDATA%")
            .iter()
            .copied()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        // SAFETY: the key is open, the name is null-terminated and data holds its length.
        let status = unsafe {
            RegSetValueExW(
                registry.0,
                w!("path").as_ptr(),
                0,
                REG_EXPAND_SZ,
                data.as_ptr(),
                data.len() as u32,
            )
        };
        assert_eq!(status, ERROR_SUCCESS);
        let error = test.0.get("path").expect_err("unsupported");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn registry_path_uses_organization_and_application() {
        assert_eq!(
            registry_path("nl.bplaat.Reversi"),
            "Software\\bplaat\\Reversi"
        );
        assert_eq!(
            registry_path("com.example.my.app"),
            "Software\\example\\my.app"
        );
        assert_eq!(registry_path("example"), "Software\\example");
        assert_eq!(registry_path("a..b"), "Software\\a..b");
    }

    #[test]
    fn unsupported_values_are_errors() {
        assert!(decode(REG_DWORD, &2u32.to_le_bytes()).is_err());
        assert!(decode(REG_BINARY, &f32::NAN.to_le_bytes()).is_err());
        assert!(decode(REG_BINARY, &[0; 8]).is_err());
        assert!(decode(7, &[]).is_err());
    }
}
