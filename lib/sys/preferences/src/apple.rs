/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(unsafe_code)]

use std::ffi::{c_char, c_void};
use std::io;

use objc2::rc::{Allocated, Retained, autoreleasepool};
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2::{class, msg_send};

use crate::Value;

#[link(name = "Foundation", kind = "framework")]
unsafe extern "C" {}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFGetTypeID(value: *const c_void) -> usize;
    fn CFBooleanGetTypeID() -> usize;
}

pub(super) struct Store {
    defaults: Retained<AnyObject>,
    name: Retained<AnyObject>,
}

impl Store {
    pub(super) fn new(id: &str) -> io::Result<Self> {
        autoreleasepool(|_| {
            let name = string(id);
            // SAFETY: NSBundle and NSUserDefaults support these messages, and name is a
            // live NSString. The returned objects are retained by Store.
            let defaults: Option<Retained<AnyObject>> = unsafe {
                let bundle: Retained<AnyObject> = msg_send![class!(NSBundle), mainBundle];
                let bundle_id: Option<Retained<AnyObject>> = msg_send![&*bundle, bundleIdentifier];
                let is_bundle_id = bundle_id.is_some_and(|bundle_id| {
                    let equal: Bool = msg_send![&*bundle_id, isEqualToString: &*name];
                    equal.as_bool()
                });
                // A suite named after the app's own bundle ID is unsupported, the
                // standard defaults already use that domain.
                if is_bundle_id {
                    msg_send![class!(NSUserDefaults), standardUserDefaults]
                } else {
                    let allocated: Allocated<AnyObject> = msg_send![class!(NSUserDefaults), alloc];
                    msg_send![allocated, initWithSuiteName: &*name]
                }
            };
            Ok(Self {
                defaults: defaults
                    .ok_or_else(|| io::Error::other("could not open NSUserDefaults"))?,
                name,
            })
        })
    }

    pub(super) fn get(&self, key: &str) -> io::Result<Option<Value>> {
        autoreleasepool(|_| {
            let key = preference_key(key);
            // SAFETY: defaults is NSUserDefaults, name and key are NSStrings, and
            // persistentDomainForName: returns an NSDictionary. Class tests
            // precede type-specific messages. NSNumber bridges to CFNumber/CFBoolean.
            unsafe {
                let domain: Option<Retained<AnyObject>> =
                    msg_send![&*self.defaults, persistentDomainForName: &*self.name];
                let Some(domain) = domain else {
                    return Ok(None);
                };
                let object: Option<Retained<AnyObject>> = msg_send![&*domain, objectForKey: &*key];
                let Some(object) = object else {
                    return Ok(None);
                };
                let is_string: Bool =
                    msg_send![&*object, isKindOfClass: class!(NSString).cast::<AnyClass>()];
                if is_string.as_bool() {
                    return text(&object).map(|text| Some(Value::String(text)));
                }
                let is_data: Bool =
                    msg_send![&*object, isKindOfClass: class!(NSData).cast::<AnyClass>()];
                if is_data.as_bool() {
                    let length: usize = msg_send![&*object, length];
                    let bytes: *const c_void = msg_send![&*object, bytes];
                    // NSData owns this buffer for the retained object's lifetime.
                    let bytes = if length == 0 {
                        &[]
                    } else {
                        std::slice::from_raw_parts(bytes.cast::<u8>(), length)
                    };
                    return String::from_utf8(bytes.to_vec())
                        .map(|text| Some(Value::String(text)))
                        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error));
                }
                let is_number: Bool =
                    msg_send![&*object, isKindOfClass: class!(NSNumber).cast::<AnyClass>()];
                if is_number.as_bool() {
                    if CFGetTypeID((&*object as *const AnyObject).cast()) == CFBooleanGetTypeID() {
                        let value: Bool = msg_send![&*object, boolValue];
                        return Ok(Some(Value::Bool(value.as_bool())));
                    }
                    let encoding: *const c_char = msg_send![&*object, objCType];
                    if matches!(*encoding as u8, b'f' | b'd') {
                        let value: f32 = msg_send![&*object, floatValue];
                        if !value.is_finite() {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "float out of range",
                            ));
                        }
                        return Ok(Some(Value::Float(value)));
                    }
                    let value: i64 = msg_send![&*object, longLongValue];
                    return Ok(Some(Value::Integer(value)));
                }
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unsupported preference value",
                ))
            }
        })
    }

    pub(super) fn set(&self, key: &str, value: Value) -> io::Result<()> {
        autoreleasepool(|_| {
            let key = preference_key(key);
            // SAFETY: NSNumber constructors match their Rust argument types.
            // setObject:forKey: accepts these property-list objects and retains them.
            unsafe {
                let object: Retained<AnyObject> = match value {
                    Value::Bool(value) => {
                        msg_send![class!(NSNumber), numberWithBool: if value { Bool::YES } else { Bool::NO }]
                    }
                    Value::Integer(value) => msg_send![class!(NSNumber), numberWithLongLong: value],
                    Value::Float(value) => msg_send![class!(NSNumber), numberWithFloat: value],
                    // CFPreferences truncates embedded NULs in NSStrings on disk.
                    Value::String(value) if value.contains('\0') => {
                        msg_send![class!(NSData), dataWithBytes: value.as_ptr().cast::<c_void>(), length: value.len()]
                    }
                    Value::String(value) => string(&value),
                };
                let _: () = msg_send![&*self.defaults, setObject: &*object, forKey: &*key];
            }
            Ok(())
        })
    }

    pub(super) fn remove(&self, key: &str) -> io::Result<()> {
        autoreleasepool(|_| {
            let key = preference_key(key);
            // SAFETY: defaults is NSUserDefaults and key is a live NSString.
            let _: () = unsafe { msg_send![&*self.defaults, removeObjectForKey: &*key] };
            Ok(())
        })
    }
}

fn preference_key(value: &str) -> Retained<AnyObject> {
    const PREFIX: &str = "__preferences__";
    if value.contains('\0') || value.starts_with(PREFIX) {
        string(&format!(
            "{PREFIX}{}",
            value.replace('%', "%25").replace('\0', "%00")
        ))
    } else {
        string(value)
    }
}

fn string(value: &str) -> Retained<AnyObject> {
    // SAFETY: bytes and length describe valid UTF-8, encoding 4 is UTF-8.
    // initWithBytes copies the bytes and returns an owned NSString.
    unsafe {
        let allocated: Allocated<AnyObject> = msg_send![class!(NSString), alloc];
        msg_send![allocated, initWithBytes: value.as_ptr().cast::<c_void>(), length: value.len(), encoding: 4usize]
    }
}

fn text(object: &AnyObject) -> io::Result<String> {
    // SAFETY: callers check that object is NSString. UTF8String stays valid while
    // object lives; the explicit length preserves embedded NUL characters.
    unsafe {
        let length: usize = msg_send![object, lengthOfBytesUsingEncoding: 4usize];
        if length == 0 {
            return Ok(String::new());
        }
        let bytes: *const c_char = msg_send![object, UTF8String];
        if bytes.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid NSString",
            ));
        }
        String::from_utf8(std::slice::from_raw_parts(bytes.cast::<u8>(), length).to_vec())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_only_the_named_domain() {
        let id = format!(
            "nl.bplaat.preferences-tests.isolation.{}",
            std::process::id()
        );
        let prefs = crate::Preferences::new(&id).expect("open");
        let other = crate::Preferences::new(&format!("{id}.other")).expect("other store");
        let key = "isolated-key";
        autoreleasepool(|_| {
            let key = string(key);
            // SAFETY: these are live Foundation objects with matching argument types.
            unsafe {
                let value: Retained<AnyObject> =
                    msg_send![class!(NSNumber), numberWithBool: Bool::YES];
                let defaults: Retained<AnyObject> =
                    msg_send![class!(NSDictionary), dictionaryWithObject: &*value, forKey: &*key];
                let _: () = msg_send![&*prefs.store.defaults, registerDefaults: &*defaults];
                let inherited: Option<Retained<AnyObject>> =
                    msg_send![&*prefs.store.defaults, objectForKey: &*key];
                assert!(inherited.is_some());
            }
        });
        assert_eq!(prefs.get(key).expect("missing key"), None);
        prefs.set(key, false).expect("set");
        assert_eq!(
            prefs.get(key).expect("stored key"),
            Some(Value::Bool(false))
        );
        assert_eq!(other.get(key).expect("other domain"), None);
        prefs.remove(key).expect("remove");
        assert_eq!(prefs.get(key).expect("removed key"), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn values_survive_writer_process_exit() {
        const CHILD_ID: &str = "SHARED_PREFS_TEST_CHILD_ID";
        let values = [
            Value::Bool(true),
            Value::Integer(i64::MAX),
            Value::Float(1.25),
            Value::String("caf\u{e9}\0\u{1f980}".into()),
        ];
        if let Ok(id) = std::env::var(CHILD_ID) {
            let prefs = crate::Preferences::new(&id).expect("open in child");
            for (key, value) in process_keys().into_iter().zip(values) {
                prefs.set(key, value).expect("set in child");
            }
            // Production writes remain asynchronous. Flush here so this test
            // checks decoding persisted values without depending on daemon timing.
            // SAFETY: defaults is a live NSUserDefaults instance.
            let flushed: Bool = unsafe { msg_send![&*prefs.store.defaults, synchronize] };
            assert!(flushed.as_bool());
            return;
        }
        let id = format!("nl.bplaat.preferences-tests.process.{}", std::process::id());
        let status = std::process::Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "apple::tests::values_survive_writer_process_exit",
            ])
            .env(CHILD_ID, &id)
            .status()
            .expect("run writer process");
        assert!(status.success());
        let prefs = crate::Preferences::new(&id).expect("open after child exits");
        for (key, value) in process_keys().into_iter().zip(values) {
            assert_eq!(prefs.get(key).expect("persisted value"), Some(value));
            prefs.remove(key).expect("remove");
        }
    }

    #[cfg(target_os = "macos")]
    fn process_keys() -> [&'static str; 4] {
        [
            "nul\0key",
            "__preferences__nul%00key",
            "nul%00key",
            "caf\u{e9}\0\u{1f980}",
        ]
    }

    #[test]
    fn native_round_trip() {
        let id = format!("nl.bplaat.preferences-tests.{}", std::process::id());
        let prefs = crate::Preferences::new(&id).expect("open");
        let key = "round-trip";
        prefs.remove(key).expect("remove");
        assert_eq!(prefs.get(key).expect("get"), None);
        for value in [
            Value::Bool(true),
            Value::Bool(false),
            Value::Integer(i64::MIN),
            Value::Integer(i64::MAX),
            Value::Float(0.0),
            Value::Float(1.0),
            Value::Float(1.25),
            Value::Float(f32::MAX),
            Value::String("caf\u{e9}\0\u{1f980}".into()),
            Value::String(String::new()),
        ] {
            prefs.set(key, value.clone()).expect("set");
            let reopened = crate::Preferences::new(&id).expect("reopen");
            assert_eq!(reopened.get(key).expect("get"), Some(value));
        }
        assert!(prefs.set(key, f32::NAN).is_err());
        prefs.remove(key).expect("remove");
        assert_eq!(prefs.get(key).expect("get"), None);
    }
}
