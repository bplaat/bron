/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(unsafe_code)]

use std::io;

use jni::objects::{Global, JObject, JString};
use jni::vm::JavaVM;
use jni::{JValue, jni_sig, jni_str};

use crate::Value;

pub(super) struct Store {
    preferences: Global<JObject<'static>>,
    vm: JavaVM,
}

impl Store {
    pub(super) fn new(id: &str) -> io::Result<Self> {
        let vm = JavaVM::singleton().map_err(io::Error::other)?;
        let preferences = vm
            .attach_current_thread(|env| -> jni::errors::Result<_> {
                // The NDK exposes no Context, so ask the framework for the running Application.
                // This non-SDK method is on the unsupported list, allowed for every target SDK.
                let activity_thread = env.find_class(jni_str!("android/app/ActivityThread"))?;
                let application = env
                    .call_static_method(
                        &activity_thread,
                        jni_str!("currentApplication"),
                        jni_sig!("()Landroid/app/Application;"),
                        &[],
                    )?
                    .l()?;
                if application.is_null() {
                    return Ok(None);
                }
                let name = env.new_string(id)?;
                let preferences = env
                    .call_method(
                        &application,
                        jni_str!("getSharedPreferences"),
                        jni_sig!("(Ljava/lang/String;I)Landroid/content/SharedPreferences;"),
                        &[JValue::Object(&name), JValue::Int(0)],
                    )?
                    .l()?;
                env.new_global_ref(preferences).map(Some)
            })
            .map_err(io::Error::other)?
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "no running Android application")
            })?;
        Ok(Self { preferences, vm })
    }

    pub(super) fn get(&self, key: &str) -> io::Result<Option<Value>> {
        self.vm
            .attach_current_thread(|env| -> jni::errors::Result<_> {
                let key = env.new_string(key)?;
                let values = env
                    .call_method(
                        &self.preferences,
                        jni_str!("getAll"),
                        jni_sig!("()Ljava/util/Map;"),
                        &[],
                    )?
                    .l()?;
                let object = env
                    .call_method(
                        &values,
                        jni_str!("get"),
                        jni_sig!("(Ljava/lang/Object;)Ljava/lang/Object;"),
                        &[JValue::Object(&key)],
                    )?
                    .l()?;
                if object.is_null() {
                    return Ok(Ok(None));
                }
                let class = env.get_object_class(&object)?;
                let name = env
                    .call_method(
                        &class,
                        jni_str!("getName"),
                        jni_sig!("()Ljava/lang/String;"),
                        &[],
                    )?
                    .l()?;
                // SAFETY: Class.getName returns a java.lang.String in this JNI frame.
                let name = unsafe { JString::from_raw(env, name.into_raw()) }.try_to_string(env)?;
                let value = match name.as_str() {
                    "java.lang.Boolean" => Value::Bool(
                        env.call_method(&object, jni_str!("booleanValue"), jni_sig!("()Z"), &[])?
                            .z()?,
                    ),
                    "java.lang.Integer" | "java.lang.Long" => Value::Integer(
                        env.call_method(&object, jni_str!("longValue"), jni_sig!("()J"), &[])?
                            .j()?,
                    ),
                    "java.lang.Float" => {
                        let value = env
                            .call_method(&object, jni_str!("floatValue"), jni_sig!("()F"), &[])?
                            .f()?;
                        if !value.is_finite() {
                            return Ok(Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "float out of range",
                            )));
                        }
                        Value::Float(value)
                    }
                    "java.lang.String" => {
                        // SAFETY: the runtime class was checked above; object is a live String.
                        let string = unsafe { JString::from_raw(env, object.into_raw()) };
                        Value::String(string.try_to_string(env)?)
                    }
                    _ => {
                        return Ok(Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "unsupported preference value",
                        )));
                    }
                };
                Ok(Ok(Some(value)))
            })
            .map_err(io::Error::other)?
    }

    pub(super) fn set(&self, key: &str, value: Value) -> io::Result<()> {
        self.edit(key, Some(value))
    }

    pub(super) fn remove(&self, key: &str) -> io::Result<()> {
        self.edit(key, None)
    }

    fn edit(&self, key: &str, value: Option<Value>) -> io::Result<()> {
        let success = self.vm.attach_current_thread(|env| -> jni::errors::Result<_> {
            let key = env.new_string(key)?;
            let editor = env.call_method(&self.preferences, jni_str!("edit"), jni_sig!("()Landroid/content/SharedPreferences$Editor;"), &[])?.l()?;
            match value {
                Some(Value::Bool(value)) => {
                    env.call_method(&editor, jni_str!("putBoolean"), jni_sig!("(Ljava/lang/String;Z)Landroid/content/SharedPreferences$Editor;"), &[JValue::Object(&key), JValue::Bool(value)])?;
                }
                Some(Value::Integer(value)) => {
                    env.call_method(&editor, jni_str!("putLong"), jni_sig!("(Ljava/lang/String;J)Landroid/content/SharedPreferences$Editor;"), &[JValue::Object(&key), JValue::Long(value)])?;
                }
                Some(Value::Float(value)) => {
                    env.call_method(&editor, jni_str!("putFloat"), jni_sig!("(Ljava/lang/String;F)Landroid/content/SharedPreferences$Editor;"), &[JValue::Object(&key), JValue::Float(value)])?;
                }
                Some(Value::String(value)) => {
                    let value = env.new_string(value)?;
                    env.call_method(&editor, jni_str!("putString"), jni_sig!("(Ljava/lang/String;Ljava/lang/String;)Landroid/content/SharedPreferences$Editor;"), &[JValue::Object(&key), JValue::Object(&value)])?;
                }
                None => {
                    env.call_method(&editor, jni_str!("remove"), jni_sig!("(Ljava/lang/String;)Landroid/content/SharedPreferences$Editor;"), &[JValue::Object(&key)])?;
                }
            }
            env.call_method(&editor, jni_str!("commit"), jni_sig!("()Z"), &[])?.z()
        }).map_err(io::Error::other)?;
        if success {
            Ok(())
        } else {
            Err(io::Error::other("SharedPreferences commit failed"))
        }
    }
}
