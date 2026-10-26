/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Map, Value as Json};

use crate::Value;

// Serialize read-modify-write operations across all handles in this process.
static LOCK: Mutex<()> = Mutex::new(());
static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

pub(super) struct Store {
    path: PathBuf,
}

impl Store {
    pub(super) fn new(id: &str) -> io::Result<Self> {
        let dirs = directories::ProjectDirs::from_path(PathBuf::from(id))
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "config directory not found"))?;
        Ok(Self {
            path: dirs.config_dir().join("preferences.json"),
        })
    }

    fn read(&self) -> io::Result<Map<String, Json>> {
        match fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(invalid_data),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Map::new()),
            Err(error) => Err(error),
        }
    }

    pub(super) fn get(&self, key: &str) -> io::Result<Option<Value>> {
        let _lock = LOCK
            .lock()
            .map_err(|_| io::Error::other("preferences lock poisoned"))?;
        self.read()?.get(key).map(decode).transpose()
    }

    pub(super) fn set(&self, key: &str, value: Value) -> io::Result<()> {
        let value = match value {
            Value::Bool(value) => Json::from(value),
            Value::Integer(value) => Json::from(value),
            Value::Float(value) => {
                if !value.is_finite() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "float must be finite",
                    ));
                }
                Json::from(value)
            }
            Value::String(value) => Json::from(value),
        };
        self.update(|values| {
            values.insert(key.to_owned(), value);
        })
    }

    pub(super) fn remove(&self, key: &str) -> io::Result<()> {
        self.update(|values| {
            values.remove(key);
        })
    }

    fn update(&self, change: impl FnOnce(&mut Map<String, Json>)) -> io::Result<()> {
        let _lock = LOCK
            .lock()
            .map_err(|_| io::Error::other("preferences lock poisoned"))?;
        let mut values = self.read()?;
        change(&mut values);
        let bytes = serde_json::to_vec_pretty(&values).map_err(invalid_data)?;
        let directory = self.path.parent().expect("preferences file has a parent");
        create_directory(directory)?;
        let (temporary, mut file) = loop {
            let sequence = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
            let path = directory.join(format!(
                ".preferences-{}-{sequence}.tmp",
                std::process::id()
            ));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => break (Temporary(path), file),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        };
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        replace(&temporary.0, &self.path)
    }
}

fn decode(value: &Json) -> io::Result<Value> {
    match value {
        Json::Bool(value) => Ok(Value::Bool(*value)),
        Json::String(value) => Ok(Value::String(value.clone())),
        Json::Number(value) if value.is_i64() => {
            Ok(Value::Integer(value.as_i64().expect("integer")))
        }
        Json::Number(value) if value.is_f64() => {
            let number = value.as_f64().expect("float") as f32;
            if number.is_finite() {
                Ok(Value::Float(number))
            } else {
                Err(invalid_data("float out of range"))
            }
        }
        _ => Err(invalid_data("unsupported preference value")),
    }
}

fn invalid_data(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

struct Temporary(PathBuf);

impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[cfg(not(windows))]
fn create_directory(directory: &Path) -> io::Result<()> {
    if directory.is_dir() {
        return Ok(());
    }
    let parent = directory
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    create_directory(parent)?;
    match fs::create_dir(directory) {
        // Persist each new directory entry, including on the first write.
        Ok(()) => fs::File::open(parent)?.sync_all(),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists && directory.is_dir() => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(windows)]
fn create_directory(directory: &Path) -> io::Result<()> {
    fs::create_dir_all(directory)
}

#[cfg(not(windows))]
fn replace(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)?;
    // Persist the rename as well as the temporary file's contents.
    fs::File::open(destination.parent().expect("preferences file has a parent"))?.sync_all()
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn replace(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(source: *const u16, destination: *const u16, flags: u32) -> i32;
    }
    let source = source
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // SAFETY: both paths are valid null-terminated wide strings. REPLACE_EXISTING
    // and WRITE_THROUGH replace the destination without a delete-then-write gap.
    if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 0x1 | 0x8) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestStore(Store);

    impl TestStore {
        fn new() -> Self {
            let sequence = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
            let directory =
                std::env::temp_dir().join(format!("preferences-{}-{sequence}", std::process::id()));
            Self(Store {
                path: directory.join("preferences.json"),
            })
        }
    }

    impl Drop for TestStore {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(self.0.path.parent().expect("parent"));
        }
    }

    #[test]
    fn round_trip_and_reopen() {
        let test = TestStore::new();
        assert_eq!(test.0.get("missing").expect("get"), None);
        for value in [
            Value::Bool(true),
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
                path: test.0.path.clone(),
            };
            assert_eq!(reopened.get("key").expect("get"), Some(value));
        }
        test.0.remove("key").expect("remove");
        test.0.remove("missing").expect("remove missing");
        assert_eq!(test.0.get("key").expect("get"), None);
    }

    #[test]
    fn preserves_changes_from_other_handles() {
        let test = TestStore::new();
        let other = Store {
            path: test.0.path.clone(),
        };
        test.0.set("first", Value::Bool(true)).expect("set");
        other.set("second", Value::Integer(42)).expect("set");
        assert_eq!(test.0.get("second").expect("get"), Some(Value::Integer(42)));
        assert_eq!(other.get("first").expect("get"), Some(Value::Bool(true)));
    }

    #[test]
    fn creates_nested_directories_and_replaces_existing_preferences() {
        let test = TestStore::new();
        let store = Store {
            path: test
                .0
                .path
                .parent()
                .expect("parent")
                .join("app/config/preferences.json"),
        };
        store.set("key", Value::Bool(true)).expect("first write");
        store.set("key", Value::Bool(false)).expect("replace");
        assert_eq!(store.get("key").expect("get"), Some(Value::Bool(false)));
        store.remove("key").expect("remove");
        assert_eq!(store.get("key").expect("removed key"), None);
        assert_eq!(
            fs::read_dir(store.path.parent().expect("parent"))
                .expect("entries")
                .count(),
            1
        );
    }

    #[test]
    fn corrupt_file_is_not_overwritten() {
        let test = TestStore::new();
        fs::create_dir_all(test.0.path.parent().expect("parent")).expect("mkdir");
        for bytes in [b"broken".as_slice(), b"[]", b"null"] {
            fs::write(&test.0.path, bytes).expect("write");
            assert!(test.0.get("key").is_err());
            assert!(test.0.set("key", Value::Bool(true)).is_err());
            assert_eq!(fs::read(&test.0.path).expect("read"), bytes);
        }
    }

    #[test]
    fn rejects_non_finite_floats() {
        let test = TestStore::new();
        for number in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(test.0.set("key", Value::Float(number)).is_err());
        }
        assert!(!test.0.path.exists());
    }

    #[test]
    fn resolves_application_config_directory() {
        let id = "nl.bplaat.preferences-tests";
        let store = Store::new(id).expect("store");
        let dirs = directories::ProjectDirs::from_path(id.into()).expect("directories");
        assert_eq!(store.path, dirs.config_dir().join("preferences.json"));
    }
}
