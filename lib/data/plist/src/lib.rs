/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [plist](https://crates.io/crates/plist) crate.
//! Supports writing binary plists and reading XML plists.

use std::fmt;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::prelude::*;
pub use dictionary::Dictionary;
use xmlparser::{ElementEnd, Token as XmlToken, Tokenizer};

pub mod dictionary;

// MARK: Value
/// A plist value.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Value {
    /// An ordered array of values.
    Array(Vec<Value>),
    /// A key-value dictionary.
    Dictionary(Dictionary),
    /// A boolean value.
    Boolean(bool),
    /// A raw bytes value.
    Data(Vec<u8>),
    /// A date value.
    Date(Date),
    /// A 64-bit floating-point value.
    Real(f64),
    /// An integer value.
    Integer(Integer),
    /// A UTF-8 string value.
    String(String),
    /// A keyed archiver object reference.
    Uid(Uid),
}

impl From<Vec<Value>> for Value {
    fn from(from: Vec<Value>) -> Self {
        Value::Array(from)
    }
}

impl From<Dictionary> for Value {
    fn from(from: Dictionary) -> Self {
        Value::Dictionary(from)
    }
}

impl From<bool> for Value {
    fn from(from: bool) -> Self {
        Value::Boolean(from)
    }
}

impl From<Date> for Value {
    fn from(from: Date) -> Self {
        Value::Date(from)
    }
}

impl From<f64> for Value {
    fn from(from: f64) -> Self {
        Value::Real(from)
    }
}

impl From<String> for Value {
    fn from(from: String) -> Self {
        Value::String(from)
    }
}

impl From<&str> for Value {
    fn from(from: &str) -> Self {
        Value::String(from.into())
    }
}

// MARK: Integer
/// An integer that can be represented by either an `i64` or a `u64`.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Integer {
    value: i128,
}

impl Integer {
    /// Returns the value as an `i64` if it can be represented by that type.
    pub fn as_signed(self) -> Option<i64> {
        i64::try_from(self.value).ok()
    }

    /// Returns the value as a `u64` if it can be represented by that type.
    pub fn as_unsigned(self) -> Option<u64> {
        u64::try_from(self.value).ok()
    }

    /// Parse an XML plist integer, preferring `i64` like Apple's implementation.
    fn parse(s: &str) -> Result<Self, std::num::ParseIntError> {
        Ok(match s.strip_prefix("0x") {
            Some(hex) => u64::from_str_radix(hex, 16)?.into(),
            None => match s.parse::<i64>() {
                Ok(value) => value.into(),
                Err(_) => s.parse::<u64>()?.into(),
            },
        })
    }
}

impl fmt::Debug for Integer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.value.fmt(f)
    }
}

impl fmt::Display for Integer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.value.fmt(f)
    }
}

macro_rules! impl_integer_from {
    ($($ty:ty),*) => {$(
        impl From<$ty> for Integer {
            fn from(value: $ty) -> Self {
                Integer {
                    value: value.into(),
                }
            }
        }

        impl From<$ty> for Value {
            fn from(from: $ty) -> Self {
                Value::Integer(from.into())
            }
        }
    )*};
}
impl_integer_from!(i64, i32, i16, i8, u64, u32, u16, u8);

// MARK: Date
/// A UTC timestamp used for serialization to and from the plist date type.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Date {
    inner: SystemTime,
}

impl Date {
    /// The plist epoch (2001-01-01T00:00:00Z) as a Unix timestamp.
    const PLIST_EPOCH_UNIX_TIMESTAMP: Duration = Duration::from_secs(978_307_200);

    fn as_seconds_since_plist_epoch(&self) -> f64 {
        let plist_epoch = UNIX_EPOCH + Self::PLIST_EPOCH_UNIX_TIMESTAMP;
        match self.inner.duration_since(plist_epoch) {
            Ok(duration) => duration.as_secs_f64(),
            Err(err) => -err.duration().as_secs_f64(),
        }
    }
}

impl From<SystemTime> for Date {
    fn from(date: SystemTime) -> Self {
        Date { inner: date }
    }
}

impl From<Date> for SystemTime {
    fn from(val: Date) -> Self {
        val.inner
    }
}

// MARK: Uid
/// A plist `uid` value, used by `NSKeyedArchiver` object references.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct Uid {
    value: u64,
}

impl Uid {
    /// Creates a new `Uid` containing the given value.
    #[allow(clippy::missing_const_for_fn)] // Upstream API is not const
    pub fn new(value: u64) -> Uid {
        Uid { value }
    }

    /// Returns the value as a `u64`.
    #[allow(clippy::missing_const_for_fn)] // Upstream API is not const
    pub fn get(self) -> u64 {
        self.value
    }
}

impl fmt::Debug for Uid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.value.fmt(f)
    }
}

// MARK: Error
/// Error type returned by plist operations.
#[derive(Debug)]
pub struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error(e.to_string())
    }
}

// MARK: Value methods
impl Value {
    /// Read a plist from a file. Supports XML format; binary plists are not supported.
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Value, Error> {
        let bytes = std::fs::read(path)?;
        if bytes.starts_with(b"bplist") {
            return Err(Error("Reading binary plist is not supported".to_string()));
        }
        let text = std::str::from_utf8(&bytes).map_err(|e| Error(e.to_string()))?;
        parse_value(&tokenize(text)?, &mut 0)
    }

    /// Write this value to a file as a binary plist.
    pub fn to_file_binary<P: AsRef<Path>>(&self, path: P) -> Result<(), Error> {
        let mut bytes = Vec::new();
        to_writer_binary(&mut bytes, self)?;
        std::fs::write(path, bytes)?;
        Ok(())
    }

    /// Returns a reference to the inner dictionary if this value is a `Dictionary`.
    pub const fn as_dictionary(&self) -> Option<&Dictionary> {
        match self {
            Value::Dictionary(d) => Some(d),
            _ => None,
        }
    }

    /// Converts this value into a `Dictionary`, returning `None` if it is not one.
    pub fn into_dictionary(self) -> Option<Dictionary> {
        match self {
            Value::Dictionary(d) => Some(d),
            _ => None,
        }
    }
}

// MARK: Binary plist writer
enum Obj {
    Bool(bool),
    Int(Integer),
    Date(Date),
    Uid(Uid),
    Real(f64),
    Str(String),
    Data(Vec<u8>),
    Array(Vec<usize>),
    Dict(Vec<usize>, Vec<usize>),
}

struct Collector {
    objects: Vec<Obj>,
}

impl Collector {
    fn collect(&mut self, value: &Value) -> usize {
        match value {
            Value::Boolean(b) => self.push(Obj::Bool(*b)),
            Value::Integer(n) => self.push(Obj::Int(*n)),
            Value::Date(d) => self.push(Obj::Date(*d)),
            Value::Uid(u) => self.push(Obj::Uid(*u)),
            Value::Real(f) => self.push(Obj::Real(*f)),
            Value::String(s) => self.push(Obj::Str(s.clone())),
            Value::Data(d) => self.push(Obj::Data(d.clone())),
            Value::Array(arr) => {
                let i = self.objects.len();
                self.objects.push(Obj::Array(vec![]));
                let children: Vec<usize> = arr.iter().map(|v| self.collect(v)).collect();
                self.objects[i] = Obj::Array(children);
                i
            }
            Value::Dictionary(dict) => {
                let i = self.objects.len();
                self.objects.push(Obj::Dict(vec![], vec![]));
                let mut keys = Vec::new();
                let mut vals = Vec::new();
                for (k, v) in dict {
                    let k_idx = self.objects.len();
                    self.objects.push(Obj::Str(k.clone()));
                    keys.push(k_idx);
                    vals.push(self.collect(v));
                }
                self.objects[i] = Obj::Dict(keys, vals);
                i
            }
        }
    }

    fn push(&mut self, obj: Obj) -> usize {
        let i = self.objects.len();
        self.objects.push(obj);
        i
    }
}

/// Write a value to a writer as a binary plist (bplist00 format).
pub fn to_writer_binary<W: Write>(mut writer: W, value: &Value) -> Result<(), Error> {
    let mut collector = Collector {
        objects: Vec::new(),
    };
    let root_idx = collector.collect(value);
    let objects = collector.objects;
    let num_objects = objects.len();

    let ref_size: usize = if num_objects <= 0xFF {
        1
    } else if num_objects <= 0xFFFF {
        2
    } else {
        4
    };

    writer.write_all(b"bplist00").map_err(Error::from)?;

    // Encode objects and track their byte offsets
    let mut offsets: Vec<u64> = Vec::with_capacity(num_objects);
    let mut pos: u64 = 8; // skip magic
    let mut encoded: Vec<Vec<u8>> = Vec::with_capacity(num_objects);
    for obj in &objects {
        let bytes = encode_obj(obj, ref_size);
        offsets.push(pos);
        pos += bytes.len() as u64;
        encoded.push(bytes);
    }

    for bytes in &encoded {
        writer.write_all(bytes).map_err(Error::from)?;
    }

    let offset_table_offset = pos;
    let offset_int_size: usize = if pos <= 0xFF {
        1
    } else if pos <= 0xFFFF {
        2
    } else if pos <= 0xFFFF_FFFF {
        4
    } else {
        8
    };

    for &offset in &offsets {
        writer
            .write_all(&encode_uint(offset, offset_int_size))
            .map_err(Error::from)?;
    }

    // 32-byte trailer
    writer.write_all(&[0u8; 5]).map_err(Error::from)?; // unused padding
    writer.write_all(&[0u8]).map_err(Error::from)?; // sort_version
    writer
        .write_all(&[offset_int_size as u8])
        .map_err(Error::from)?;
    writer.write_all(&[ref_size as u8]).map_err(Error::from)?;
    writer
        .write_all(&encode_uint(num_objects as u64, 8))
        .map_err(Error::from)?;
    writer
        .write_all(&encode_uint(root_idx as u64, 8))
        .map_err(Error::from)?;
    writer
        .write_all(&encode_uint(offset_table_offset, 8))
        .map_err(Error::from)?;

    Ok(())
}

fn encode_obj(obj: &Obj, ref_size: usize) -> Vec<u8> {
    match obj {
        Obj::Bool(false) => vec![0x08],
        Obj::Bool(true) => vec![0x09],
        Obj::Int(n) => match n.as_signed() {
            Some(n) => encode_int_obj(n),
            // Integers above `i64::MAX` are stored as signed 128-bit integers
            None => {
                let mut v = vec![0x14];
                v.extend_from_slice(&n.value.to_be_bytes());
                v
            }
        },
        Obj::Date(d) => {
            let mut v = vec![0x33];
            v.extend_from_slice(&d.as_seconds_since_plist_epoch().to_bits().to_be_bytes());
            v
        }
        Obj::Uid(u) => {
            let bytes = u.value.to_be_bytes();
            let size = match u.value {
                0..=0xFF => 1,
                0x100..=0xFFFF => 2,
                0x1_0000..=0xFFFF_FFFF => 4,
                _ => 8,
            };
            let mut v = vec![0x80 | (size - 1) as u8];
            v.extend_from_slice(&bytes[8 - size..]);
            v
        }
        Obj::Real(f) => {
            let mut v = vec![0x23]; // 64-bit double
            v.extend_from_slice(&f.to_bits().to_be_bytes());
            v
        }
        Obj::Str(s) => {
            if s.is_ascii() {
                let mut v = count_tag(0x50, s.len());
                v.extend_from_slice(s.as_bytes());
                v
            } else {
                let utf16: Vec<u16> = s.encode_utf16().collect();
                let mut v = count_tag(0x60, utf16.len());
                for ch in utf16 {
                    v.extend_from_slice(&ch.to_be_bytes());
                }
                v
            }
        }
        Obj::Data(d) => {
            let mut v = count_tag(0x40, d.len());
            v.extend_from_slice(d);
            v
        }
        Obj::Array(refs) => {
            let mut v = count_tag(0xA0, refs.len());
            for &r in refs {
                v.extend_from_slice(&encode_uint(r as u64, ref_size));
            }
            v
        }
        Obj::Dict(keys, vals) => {
            let mut v = count_tag(0xD0, keys.len());
            for &k in keys {
                v.extend_from_slice(&encode_uint(k as u64, ref_size));
            }
            for &val in vals {
                v.extend_from_slice(&encode_uint(val as u64, ref_size));
            }
            v
        }
    }
}

fn count_tag(tag: u8, count: usize) -> Vec<u8> {
    if count < 15 {
        vec![tag | count as u8]
    } else {
        let mut v = vec![tag | 0xF];
        v.extend_from_slice(&encode_int_obj(count as i64));
        v
    }
}

fn encode_int_obj(n: i64) -> Vec<u8> {
    if (0..=0xFF).contains(&n) {
        vec![0x10, n as u8]
    } else if (0..=0xFFFF).contains(&n) {
        vec![0x11, (n >> 8) as u8, n as u8]
    } else if (0..=0xFFFF_FFFF).contains(&n) {
        let mut v = vec![0x12];
        v.extend_from_slice(&(n as u32).to_be_bytes());
        v
    } else {
        let mut v = vec![0x13];
        v.extend_from_slice(&n.to_be_bytes());
        v
    }
}

fn encode_uint(n: u64, size: usize) -> Vec<u8> {
    match size {
        1 => vec![n as u8],
        2 => (n as u16).to_be_bytes().to_vec(),
        4 => (n as u32).to_be_bytes().to_vec(),
        _ => n.to_be_bytes().to_vec(),
    }
}

// MARK: XML plist reader
#[derive(Debug)]
enum Token {
    Open(String),
    Close(String),
    SelfClose(String),
    Text(String),
}

fn tokenize(input: &str) -> Result<Vec<Token>, Error> {
    let mut tokens = Vec::new();
    let mut name = "";
    let mut text = String::new();
    for token in Tokenizer::from(input) {
        let token = token.map_err(|e| Error(e.to_string()))?;
        // Flush text at element boundaries, dropping whitespace between elements
        if matches!(
            token,
            XmlToken::ElementStart { .. } | XmlToken::ElementEnd { .. }
        ) {
            let text = std::mem::take(&mut text);
            if !text.trim().is_empty() {
                tokens.push(Token::Text(text));
            }
        }
        match token {
            XmlToken::ElementStart { local, .. } => name = local.as_str(),
            XmlToken::ElementEnd { end, .. } => match end {
                ElementEnd::Open => tokens.push(Token::Open(name.to_string())),
                ElementEnd::Empty => tokens.push(Token::SelfClose(name.to_string())),
                ElementEnd::Close(_, local) => {
                    tokens.push(Token::Close(local.as_str().to_string()))
                }
            },
            XmlToken::Text { text: t } => text.push_str(&xml_unescape(t.as_str())?),
            XmlToken::Cdata { text: t, .. } => text.push_str(t.as_str()),
            _ => {}
        }
    }
    Ok(tokens)
}

fn parse_value(tokens: &[Token], pos: &mut usize) -> Result<Value, Error> {
    while *pos < tokens.len() {
        match &tokens[*pos] {
            Token::Open(name) => {
                let name = name.clone();
                *pos += 1;
                return match name.as_str() {
                    "plist" => parse_value(tokens, pos),
                    "dict" => parse_dict(tokens, pos),
                    "array" => parse_array(tokens, pos),
                    "string" => {
                        let text = read_text(tokens, pos);
                        consume_close(tokens, pos, "string")?;
                        Ok(Value::String(text))
                    }
                    "integer" => {
                        let text = read_text(tokens, pos);
                        consume_close(tokens, pos, "integer")?;
                        Integer::parse(text.trim())
                            .map(Value::Integer)
                            .map_err(|e| Error(e.to_string()))
                    }
                    "real" => {
                        let text = read_text(tokens, pos);
                        consume_close(tokens, pos, "real")?;
                        text.trim()
                            .parse::<f64>()
                            .map(Value::Real)
                            .map_err(|e| Error(e.to_string()))
                    }
                    "data" => {
                        let text = read_text(tokens, pos);
                        consume_close(tokens, pos, "data")?;
                        let clean: String =
                            text.chars().filter(|c| !c.is_ascii_whitespace()).collect();
                        BASE64_STANDARD
                            .decode(clean)
                            .map(Value::Data)
                            .map_err(|e| Error(e.to_string()))
                    }
                    other => Err(Error(format!("Unknown plist element: {other}"))),
                };
            }
            Token::SelfClose(name) => {
                let name = name.clone();
                *pos += 1;
                return match name.as_str() {
                    "true" => Ok(Value::Boolean(true)),
                    "false" => Ok(Value::Boolean(false)),
                    other => Err(Error(format!("Unknown self-closing element: {other}"))),
                };
            }
            Token::Close(_) => return Err(Error("Unexpected closing tag".to_string())),
            Token::Text(_) => {
                *pos += 1; // skip stray text nodes
            }
        }
    }
    Err(Error("Unexpected end of plist".to_string()))
}

fn parse_dict(tokens: &[Token], pos: &mut usize) -> Result<Value, Error> {
    let mut dict = Dictionary::new();
    loop {
        match tokens.get(*pos) {
            Some(Token::Close(name)) if name == "dict" => {
                *pos += 1;
                break;
            }
            Some(Token::Open(name)) if name == "key" => {
                *pos += 1;
                let key = read_text(tokens, pos);
                consume_close(tokens, pos, "key")?;
                let value = parse_value(tokens, pos)?;
                dict.insert(key, value);
            }
            None => return Err(Error("Unexpected end of plist".to_string())),
            _ => {
                *pos += 1;
            }
        }
    }
    Ok(Value::Dictionary(dict))
}

fn parse_array(tokens: &[Token], pos: &mut usize) -> Result<Value, Error> {
    let mut arr = Vec::new();
    loop {
        match tokens.get(*pos) {
            Some(Token::Close(name)) if name == "array" => {
                *pos += 1;
                break;
            }
            None => return Err(Error("Unexpected end of plist".to_string())),
            _ => arr.push(parse_value(tokens, pos)?),
        }
    }
    Ok(Value::Array(arr))
}

fn read_text(tokens: &[Token], pos: &mut usize) -> String {
    match tokens.get(*pos) {
        Some(Token::Text(t)) => {
            let s = t.clone();
            *pos += 1;
            s
        }
        _ => String::new(),
    }
}

fn consume_close(tokens: &[Token], pos: &mut usize, tag: &str) -> Result<(), Error> {
    match tokens.get(*pos) {
        Some(Token::Close(name)) if name == tag => {
            *pos += 1;
            Ok(())
        }
        _ => Err(Error(format!("Expected closing tag: {tag}"))),
    }
}

fn xml_unescape(s: &str) -> Result<String, Error> {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let semi = rest[amp..]
            .find(';')
            .ok_or_else(|| Error("Unterminated entity reference".to_string()))?;
        let entity = &rest[amp + 1..amp + semi];
        let c = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .map(|hex| u32::from_str_radix(hex, 16))
                .or_else(|| entity.strip_prefix('#').map(str::parse))
                .and_then(Result::ok)
                .and_then(char::from_u32),
        };
        out.push(c.ok_or_else(|| Error(format!("Unknown entity reference: &{entity};")))?);
        rest = &rest[amp + semi + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    fn parse(xml: &str) -> Result<Value, Error> {
        parse_value(&tokenize(xml)?, &mut 0)
    }

    fn write_binary(value: &Value) -> Vec<u8> {
        let mut bytes = Vec::new();
        to_writer_binary(&mut bytes, value).expect("write");
        bytes
    }

    // -- binary writer: structural checks (all platforms) --

    #[test]
    fn test_binary_magic() {
        assert!(write_binary(&Value::Boolean(true)).starts_with(b"bplist00"));
    }

    #[test]
    fn test_binary_trailer_size() {
        // magic(8) + bool(1) + offset_table(1) + trailer(32) = 42
        assert_eq!(write_binary(&Value::Boolean(true)).len(), 42);
    }

    #[test]
    fn test_binary_bool_bytes() {
        assert_eq!(write_binary(&Value::Boolean(true))[8], 0x09);
        assert_eq!(write_binary(&Value::Boolean(false))[8], 0x08);
    }

    #[test]
    fn test_binary_int_1_byte() {
        let b = write_binary(&Value::Integer(42.into()));
        assert_eq!(b[8], 0x10);
        assert_eq!(b[9], 42);
    }

    #[test]
    fn test_binary_int_2_bytes() {
        let b = write_binary(&Value::Integer(256.into()));
        assert_eq!(b[8], 0x11);
        assert_eq!(&b[9..11], &[0x01, 0x00]);
    }

    #[test]
    fn test_binary_int_4_bytes() {
        let b = write_binary(&Value::Integer(0x10000.into()));
        assert_eq!(b[8], 0x12);
        assert_eq!(&b[9..13], &[0x00, 0x01, 0x00, 0x00]);
    }

    #[test]
    fn test_binary_int_8_bytes_negative() {
        let b = write_binary(&Value::Integer((-1).into()));
        assert_eq!(b[8], 0x13);
        assert_eq!(&b[9..17], &[0xFF; 8]);
    }

    #[test]
    fn test_binary_real() {
        let b = write_binary(&Value::Real(1.5));
        assert_eq!(b[8], 0x23);
        let bits = u64::from_be_bytes(b[9..17].try_into().unwrap());
        assert_eq!(bits, 1.5f64.to_bits());
    }

    #[test]
    fn test_binary_ascii_string() {
        let b = write_binary(&Value::String("hello".to_string()));
        assert_eq!(b[8], 0x55); // 0x50 | 5
        assert_eq!(&b[9..14], b"hello");
    }

    #[test]
    fn test_binary_unicode_string() {
        // "cafe\u{301}" has 5 UTF-16 code units, non-ASCII -> 0x60 tag
        let b = write_binary(&Value::String("caf\u{00E9}".to_string()));
        assert_eq!(b[8] & 0xF0, 0x60);
    }

    #[test]
    fn test_binary_data() {
        let data = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let b = write_binary(&Value::Data(data.clone()));
        assert_eq!(b[8], 0x44); // 0x40 | 4
        assert_eq!(&b[9..13], &data[..]);
    }

    #[test]
    fn test_binary_empty_array() {
        assert_eq!(write_binary(&Value::Array(vec![]))[8], 0xA0);
    }

    #[test]
    fn test_binary_array() {
        let b = write_binary(&Value::Array(vec![
            Value::Boolean(true),
            Value::Boolean(false),
        ]));
        assert_eq!(b[8], 0xA2); // 0xA0 | 2
    }

    #[test]
    fn test_binary_dict_marker() {
        let mut dict = Dictionary::new();
        dict.insert("a".to_string(), Value::Boolean(true));
        dict.insert("b".to_string(), Value::Boolean(false));
        let b = write_binary(&Value::Dictionary(dict));
        assert_eq!(b[8], 0xD2); // 0xD0 | 2
    }

    // -- XML reader (all platforms) --

    #[test]
    fn test_xml_dict() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
    <key>Name</key><string>TestApp</string>
    <key>Version</key><string>1.2.3</string>
    <key>Flag</key><true/>
    <key>Count</key><integer>42</integer>
</dict></plist>"#;
        let Value::Dictionary(dict) = parse(xml).expect("parse") else {
            panic!("expected dict");
        };
        assert_eq!(dict["Name"], Value::String("TestApp".to_string()));
        assert_eq!(dict["Version"], Value::String("1.2.3".to_string()));
        assert_eq!(dict["Flag"], Value::Boolean(true));
        assert_eq!(dict["Count"], Value::Integer(42.into()));
    }

    #[test]
    fn test_xml_false() {
        assert_eq!(
            parse(r#"<plist><false/></plist>"#).expect("parse"),
            Value::Boolean(false)
        );
    }

    #[test]
    fn test_xml_real() {
        let xml = format!(r#"<plist><real>{}</real></plist>"#, std::f64::consts::PI);
        let Value::Real(f) = parse(&xml).expect("parse") else {
            panic!("expected real");
        };
        assert!((f - std::f64::consts::PI).abs() < 1e-10);
    }

    #[test]
    fn test_xml_array() {
        let xml = r#"<plist><array><string>a</string><integer>3</integer><false/></array></plist>"#;
        let Value::Array(arr) = parse(xml).expect("parse") else {
            panic!("expected array");
        };
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0], Value::String("a".to_string()));
        assert_eq!(arr[1], Value::Integer(3.into()));
        assert_eq!(arr[2], Value::Boolean(false));
    }

    #[test]
    fn test_xml_data() {
        // "hello" in base64 = "aGVsbG8="
        let Value::Data(d) = parse(r#"<plist><data>aGVsbG8=</data></plist>"#).expect("parse")
        else {
            panic!("expected data");
        };
        assert_eq!(d, b"hello");
    }

    #[test]
    fn test_xml_data_whitespace() {
        let xml = "<plist><data>\n\taGVs\n\tbG8=\n</data></plist>";
        let Value::Data(d) = parse(xml).expect("parse") else {
            panic!("expected data");
        };
        assert_eq!(d, b"hello");
    }

    #[test]
    fn test_xml_data_invalid() {
        assert!(parse("<plist><data>aGV!sbG8=</data></plist>").is_err());
    }

    #[test]
    fn test_xml_nested_dict() {
        let xml = r#"<plist><dict>
            <key>inner</key><dict><key>x</key><integer>42</integer></dict>
        </dict></plist>"#;
        let Value::Dictionary(outer) = parse(xml).expect("parse") else {
            panic!("expected dict");
        };
        let Value::Dictionary(inner) = &outer["inner"] else {
            panic!("expected inner dict");
        };
        assert_eq!(inner["x"], Value::Integer(42.into()));
    }

    #[test]
    fn test_xml_unescape() {
        assert_eq!(xml_unescape("a &amp; b").expect("unescape"), "a & b");
        assert_eq!(xml_unescape("&lt;tag&gt;").expect("unescape"), "<tag>");
        assert_eq!(xml_unescape("&quot;hi&quot;").expect("unescape"), "\"hi\"");
        assert_eq!(xml_unescape("&apos;x&apos;").expect("unescape"), "'x'");
        assert_eq!(
            xml_unescape("&#65;&#x42;&#x1F600;").expect("unescape"),
            "AB\u{1F600}"
        );
        assert!(xml_unescape("&bogus;").is_err());
        assert!(xml_unescape("a & b").is_err());
    }

    #[test]
    fn test_xml_text() {
        let xml = "<plist><array><string> a &amp; <![CDATA[<b>]]> </string><string></string></array></plist>";
        assert_eq!(
            parse(xml).expect("parse"),
            Value::Array(vec![
                Value::String(" a & <b> ".to_string()),
                Value::String(String::new()),
            ])
        );
    }

    #[test]
    fn test_xml_malformed() {
        assert!(parse("<plist><string>a</plist>").is_err());
        assert!(parse("<plist><string>a").is_err());
    }

    #[test]
    fn test_from_impls() {
        assert_eq!(Value::from(true), Value::Boolean(true));
        assert_eq!(Value::from(false), Value::Boolean(false));
        assert_eq!(Value::from(42i64), Value::Integer(42.into()));
        assert_eq!(Value::from(42u8), Value::Integer(42.into()));
        assert_eq!(
            Value::from(vec![Value::from(true)]),
            Value::Array(vec![Value::Boolean(true)])
        );
        assert_eq!(Value::from(1.0f64), Value::Real(1.0));
        assert_eq!(Value::from("hello"), Value::String("hello".to_string()));
        assert_eq!(
            Value::from("world".to_string()),
            Value::String("world".to_string())
        );
    }

    #[test]
    fn test_binary_int_unsigned_128() {
        let b = write_binary(&Value::from(u64::MAX));
        assert_eq!(b[8], 0x14);
        assert_eq!(&b[9..17], &[0; 8]);
        assert_eq!(&b[17..25], &[0xFF; 8]);
    }

    #[test]
    fn test_binary_date() {
        let date = Date::from(UNIX_EPOCH + Duration::from_secs(978_307_200 + 60));
        let b = write_binary(&Value::Date(date));
        assert_eq!(b[8], 0x33);
        assert_eq!(&b[9..17], &60f64.to_bits().to_be_bytes());
    }

    #[test]
    fn test_binary_uid() {
        assert_eq!(&write_binary(&Value::Uid(Uid::new(7)))[8..10], &[0x80, 7]);
        assert_eq!(
            &write_binary(&Value::Uid(Uid::new(0x1234)))[8..11],
            &[0x81, 0x12, 0x34]
        );
    }

    #[test]
    fn test_integer() {
        assert_eq!(Integer::from(-1i64).as_signed(), Some(-1));
        assert_eq!(Integer::from(-1i64).as_unsigned(), None);
        assert_eq!(Integer::from(u64::MAX).as_signed(), None);
        assert_eq!(Integer::from(u64::MAX).as_unsigned(), Some(u64::MAX));
        assert_eq!(Integer::parse("0x1F").map(Integer::as_signed), Ok(Some(31)));
        assert_eq!(
            Integer::parse("18446744073709551615").map(Integer::as_unsigned),
            Ok(Some(u64::MAX))
        );
        assert!(Integer::parse("abc").is_err());
    }

    #[test]
    fn test_dictionary_order() {
        let mut dict = [("b", 1), ("a", 2)].into_iter().collect::<Dictionary>();
        assert_eq!(dict.insert("b".into(), 3.into()), Some(Value::from(1)));
        dict.extend([("c".to_string(), Value::from(4))]);
        let keys = dict.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>();
        assert_eq!(keys, ["b", "a", "c"]);
        assert_eq!(dict.len(), 3);
        assert_eq!(dict["b"], Value::from(3));
        assert_eq!(
            dict,
            [("c", 4), ("a", 2), ("b", 3)]
                .into_iter()
                .collect::<Dictionary>()
        );
    }

    #[test]
    fn test_xml_dict_order() {
        let xml = "<plist><dict><key>z</key><true/><key>a</key><false/></dict></plist>";
        let Value::Dictionary(dict) = parse(xml).expect("parse") else {
            panic!("expected dict");
        };
        let keys = dict.into_iter().map(|(k, _)| k).collect::<Vec<_>>();
        assert_eq!(keys, ["z", "a"]);
    }

    // -- macOS plutil validation (optional, requires plutil) --

    #[cfg(target_os = "macos")]
    fn plutil_lint(bytes: &[u8]) -> bool {
        use std::process::Command;
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("plist_test_{}_{}.plist", std::process::id(), n));
        std::fs::write(&path, bytes).expect("write temp plist");
        let ok = Command::new("plutil")
            .arg("-lint")
            .arg(&path)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        std::fs::remove_file(&path).ok();
        ok
    }

    #[cfg(target_os = "macos")]
    fn plutil_roundtrip(bytes: &[u8]) -> Value {
        use std::process::Command;
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("plist_rt_{}_{}.plist", std::process::id(), n));
        std::fs::write(&path, bytes).expect("write temp plist");
        let output = Command::new("plutil")
            .args(["-convert", "xml1", "-o", "-"])
            .arg(&path)
            .output()
            .expect("plutil");
        std::fs::remove_file(&path).ok();
        assert!(output.status.success(), "plutil conversion failed");
        let xml = std::str::from_utf8(&output.stdout).expect("utf8");
        parse(xml).expect("parse xml")
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_plutil_primitives() {
        assert!(plutil_lint(&write_binary(&Value::Boolean(true))));
        assert!(plutil_lint(&write_binary(&Value::Boolean(false))));
        assert!(plutil_lint(&write_binary(&Value::Integer(0.into()))));
        assert!(plutil_lint(&write_binary(&Value::Integer(255.into()))));
        assert!(plutil_lint(&write_binary(&Value::Integer(65536.into()))));
        assert!(plutil_lint(&write_binary(&Value::Integer((-1).into()))));
        assert!(plutil_lint(&write_binary(&Value::Real(
            std::f64::consts::PI
        ))));
        assert!(plutil_lint(&write_binary(&Value::String(
            "hello".to_string()
        ))));
        assert!(plutil_lint(&write_binary(&Value::String(
            "caf\u{00E9}".to_string()
        ))));
        assert!(plutil_lint(&write_binary(&Value::Data(vec![1, 2, 3]))));
        assert!(plutil_lint(&write_binary(&Value::from(u64::MAX))));
        assert!(plutil_lint(&write_binary(&Value::Date(
            SystemTime::now().into()
        ))));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_plutil_array() {
        let arr = Value::Array(vec![
            Value::Integer(1.into()),
            Value::String("a".to_string()),
        ]);
        assert!(plutil_lint(&write_binary(&arr)));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_plutil_info_plist() {
        let mut dict = Dictionary::new();
        dict.insert("CFBundlePackageType".to_string(), "APPL".into());
        dict.insert("CFBundleName".to_string(), "TestApp".into());
        dict.insert(
            "CFBundleIdentifier".to_string(),
            "com.example.TestApp".into(),
        );
        dict.insert("CFBundleVersion".to_string(), "1.0.0".into());
        dict.insert("CFBundleShortVersionString".to_string(), "1.0.0".into());
        dict.insert("CFBundleExecutable".to_string(), "TestApp".into());
        dict.insert("LSMinimumSystemVersion".to_string(), "11.0".into());
        assert!(plutil_lint(&write_binary(&Value::Dictionary(dict))));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_plutil_roundtrip() {
        let mut dict = Dictionary::new();
        dict.insert("Name".to_string(), "MyApp".into());
        dict.insert("Count".to_string(), Value::Integer(42.into()));
        dict.insert("Flag".to_string(), Value::Boolean(true));
        let bytes = write_binary(&Value::Dictionary(dict));
        let Value::Dictionary(result) = plutil_roundtrip(&bytes) else {
            panic!("expected dict");
        };
        assert_eq!(result["Name"], Value::String("MyApp".to_string()));
        assert_eq!(result["Count"], Value::Integer(42.into()));
        assert_eq!(result["Flag"], Value::Boolean(true));
    }
}
