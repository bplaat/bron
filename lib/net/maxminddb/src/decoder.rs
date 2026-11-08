/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::collections::HashMap;
use std::fmt::{self, Display, Formatter};

use serde::de::{self, DeserializeOwned, Deserializer, MapAccess, SeqAccess, Visitor};

use crate::MaxMindDbError;

// MARK: Value

/// A decoded MaxMind DB data value.
#[derive(Debug, Clone)]
pub(crate) enum Value<'a> {
    /// A UTF-8 string.
    Str(&'a str),
    /// Raw bytes.
    Bytes(&'a [u8]),
    /// A 64-bit float (double).
    F64(f64),
    /// A 32-bit float.
    F32(f32),
    /// A boolean.
    Bool(bool),
    /// A signed 32-bit integer.
    I32(i32),
    /// An unsigned 16-bit integer.
    U16(u16),
    /// An unsigned 32-bit integer.
    U32(u32),
    /// An unsigned 64-bit integer.
    U64(u64),
    /// An unsigned 128-bit integer.
    U128(u128),
    /// A map of string keys to values.
    Map(HashMap<&'a str, Value<'a>>),
    /// An ordered array of values.
    Array(Vec<Value<'a>>),
}

// MARK: Decoder

/// Decode a MaxMind DB value from `data` at `offset`.
/// Returns the decoded value and the new offset after the value.
pub(crate) fn decode_value_at(
    data: &[u8],
    offset: usize,
) -> Result<(Value<'_>, usize), MaxMindDbError> {
    if offset >= data.len() {
        return Err(MaxMindDbError::InvalidDatabase(
            "unexpected end of data".to_string(),
        ));
    }

    let ctrl = data[offset];
    let type_num = (ctrl >> 5) as u32;
    let mut offset = offset + 1;

    // Type 0 means extended type: next byte gives actual type + 7.
    let type_num = if type_num == 0 {
        if offset >= data.len() {
            return Err(MaxMindDbError::InvalidDatabase(
                "unexpected end of data".to_string(),
            ));
        }
        let ext = data[offset] as u32 + 7;
        offset += 1;
        ext
    } else {
        type_num
    };

    // Decode the payload size from the lower 5 bits of ctrl byte.
    let (payload_size, offset) = decode_size(ctrl & 0x1f, data, offset)?;

    match type_num {
        1 => decode_pointer(payload_size, ctrl, data, offset),
        2 => decode_string(payload_size, data, offset),
        3 => decode_f64(payload_size, data, offset),
        4 => decode_bytes(payload_size, data, offset),
        5 => decode_uint(payload_size, data, offset, 2),
        6 => decode_uint(payload_size, data, offset, 4),
        7 => decode_map(payload_size, data, offset),
        8 => decode_int32(payload_size, data, offset),
        9 => decode_uint(payload_size, data, offset, 8),
        10 => decode_uint128(payload_size, data, offset),
        11 => decode_array(payload_size, data, offset),
        14 => decode_bool(payload_size, offset),
        15 => decode_f32(payload_size, data, offset),
        _ => Err(MaxMindDbError::InvalidDatabase(format!(
            "unknown type: {type_num}"
        ))),
    }
}

fn decode_size(low5: u8, data: &[u8], offset: usize) -> Result<(usize, usize), MaxMindDbError> {
    match low5 {
        s if s < 29 => Ok((s as usize, offset)),
        29 => {
            check_bounds(data, offset, 1)?;
            Ok((29 + data[offset] as usize, offset + 1))
        }
        30 => {
            check_bounds(data, offset, 2)?;
            let size = 285 + (data[offset] as usize) * 256 + data[offset + 1] as usize;
            Ok((size, offset + 2))
        }
        31 => {
            check_bounds(data, offset, 3)?;
            let size = 65821
                + (data[offset] as usize) * 65536
                + (data[offset + 1] as usize) * 256
                + data[offset + 2] as usize;
            Ok((size, offset + 3))
        }
        _ => unreachable!(),
    }
}

fn check_bounds(data: &[u8], offset: usize, n: usize) -> Result<(), MaxMindDbError> {
    if offset + n > data.len() {
        Err(MaxMindDbError::InvalidDatabase(
            "data out of bounds".to_string(),
        ))
    } else {
        Ok(())
    }
}

// Pointers: the size field is actually pointer-specific.
fn decode_pointer(
    payload_size: usize,
    ctrl: u8,
    data: &[u8],
    offset: usize,
) -> Result<(Value<'_>, usize), MaxMindDbError> {
    // Pointer size is encoded in bits 4-3 of ctrl (after the 3 type bits).
    let pointer_size = ((ctrl >> 3) & 0x3) as usize;
    let (ptr_value, new_offset) = match pointer_size {
        0 => {
            check_bounds(data, offset, 1)?;
            let v = ((payload_size & 0x7) << 8) | data[offset] as usize;
            (v, offset + 1)
        }
        1 => {
            check_bounds(data, offset, 2)?;
            let v = 2048
                + ((payload_size & 0x7) << 16)
                + (data[offset] as usize) * 256
                + data[offset + 1] as usize;
            (v, offset + 2)
        }
        2 => {
            check_bounds(data, offset, 3)?;
            let v = 526_336
                + ((payload_size & 0x7) << 24)
                + (data[offset] as usize) * 65536
                + (data[offset + 1] as usize) * 256
                + data[offset + 2] as usize;
            (v, offset + 3)
        }
        3 => {
            check_bounds(data, offset, 4)?;
            let v = (data[offset] as usize) * 16_777_216
                + (data[offset + 1] as usize) * 65536
                + (data[offset + 2] as usize) * 256
                + data[offset + 3] as usize;
            (v, offset + 4)
        }
        _ => unreachable!(),
    };
    // Resolve the pointer: it points into the same data slice.
    let (value, _) = decode_value_at(data, ptr_value)?;
    Ok((value, new_offset))
}

fn decode_string(
    size: usize,
    data: &[u8],
    offset: usize,
) -> Result<(Value<'_>, usize), MaxMindDbError> {
    check_bounds(data, offset, size)?;
    let s = std::str::from_utf8(&data[offset..offset + size])
        .map_err(|_| MaxMindDbError::InvalidDatabase("invalid UTF-8 string".to_string()))?;
    Ok((Value::Str(s), offset + size))
}

fn decode_f64(
    size: usize,
    data: &[u8],
    offset: usize,
) -> Result<(Value<'_>, usize), MaxMindDbError> {
    if size != 8 {
        return Err(MaxMindDbError::InvalidDatabase(
            "double must be 8 bytes".to_string(),
        ));
    }
    check_bounds(data, offset, 8)?;
    let bytes: [u8; 8] = data[offset..offset + 8].try_into().map_err(|_| {
        MaxMindDbError::InvalidDatabase("double byte conversion failed".to_string())
    })?;
    Ok((Value::F64(f64::from_be_bytes(bytes)), offset + 8))
}

fn decode_f32(
    size: usize,
    data: &[u8],
    offset: usize,
) -> Result<(Value<'_>, usize), MaxMindDbError> {
    if size != 4 {
        return Err(MaxMindDbError::InvalidDatabase(
            "float must be 4 bytes".to_string(),
        ));
    }
    check_bounds(data, offset, 4)?;
    let bytes: [u8; 4] = data[offset..offset + 4]
        .try_into()
        .map_err(|_| MaxMindDbError::InvalidDatabase("float byte conversion failed".to_string()))?;
    Ok((Value::F32(f32::from_be_bytes(bytes)), offset + 4))
}

fn decode_bytes(
    size: usize,
    data: &[u8],
    offset: usize,
) -> Result<(Value<'_>, usize), MaxMindDbError> {
    check_bounds(data, offset, size)?;
    Ok((Value::Bytes(&data[offset..offset + size]), offset + size))
}

fn decode_uint(
    size: usize,
    data: &[u8],
    offset: usize,
    max_bytes: usize,
) -> Result<(Value<'_>, usize), MaxMindDbError> {
    if size > max_bytes {
        return Err(MaxMindDbError::InvalidDatabase(format!(
            "uint size {size} exceeds maximum {max_bytes}"
        )));
    }
    check_bounds(data, offset, size)?;
    let mut v: u64 = 0;
    for &b in &data[offset..offset + size] {
        v = (v << 8) | u64::from(b);
    }
    let value = if max_bytes <= 2 {
        Value::U16(v as u16)
    } else if max_bytes <= 4 {
        Value::U32(v as u32)
    } else {
        Value::U64(v)
    };
    Ok((value, offset + size))
}

fn decode_uint128(
    size: usize,
    data: &[u8],
    offset: usize,
) -> Result<(Value<'_>, usize), MaxMindDbError> {
    if size > 16 {
        return Err(MaxMindDbError::InvalidDatabase(
            "uint128 size exceeds 16".to_string(),
        ));
    }
    check_bounds(data, offset, size)?;
    let mut v: u128 = 0;
    for &b in &data[offset..offset + size] {
        v = (v << 8) | u128::from(b);
    }
    Ok((Value::U128(v), offset + size))
}

fn decode_int32(
    size: usize,
    data: &[u8],
    offset: usize,
) -> Result<(Value<'_>, usize), MaxMindDbError> {
    if size > 4 {
        return Err(MaxMindDbError::InvalidDatabase(
            "int32 size exceeds 4".to_string(),
        ));
    }
    check_bounds(data, offset, size)?;
    let mut v: i32 = 0;
    for &b in &data[offset..offset + size] {
        v = (v << 8) | i32::from(b);
    }
    Ok((Value::I32(v), offset + size))
}

fn decode_map(
    count: usize,
    data: &[u8],
    mut offset: usize,
) -> Result<(Value<'_>, usize), MaxMindDbError> {
    let mut map = HashMap::with_capacity(count);
    for _ in 0..count {
        let (key_val, new_offset) = decode_value_at(data, offset)?;
        offset = new_offset;
        let key = match key_val {
            Value::Str(s) => s,
            _ => {
                return Err(MaxMindDbError::InvalidDatabase(
                    "map key must be a string".to_string(),
                ));
            }
        };
        let (val, new_offset) = decode_value_at(data, offset)?;
        offset = new_offset;
        map.insert(key, val);
    }
    Ok((Value::Map(map), offset))
}

fn decode_array(
    count: usize,
    data: &[u8],
    mut offset: usize,
) -> Result<(Value<'_>, usize), MaxMindDbError> {
    let mut arr = Vec::with_capacity(count);
    for _ in 0..count {
        let (val, new_offset) = decode_value_at(data, offset)?;
        offset = new_offset;
        arr.push(val);
    }
    Ok((Value::Array(arr), offset))
}

const fn decode_bool(
    size: usize,
    offset: usize,
) -> Result<(Value<'static>, usize), MaxMindDbError> {
    Ok((Value::Bool(size != 0), offset))
}

// MARK: Serde Deserializer

/// Deserialize a MaxMind DB `Value` into any `T: DeserializeOwned`.
pub(crate) fn from_value<T: DeserializeOwned>(value: &Value<'_>) -> Result<T, MaxMindDbError> {
    T::deserialize(ValueDeserializer(value))
        .map_err(|e| MaxMindDbError::InvalidDatabase(e.to_string()))
}

struct ValueDeserializer<'a, 'data>(&'a Value<'data>);

#[derive(Debug)]
struct DeError(String);

impl Display for DeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl de::Error for DeError {
    fn custom<T: Display>(msg: T) -> Self {
        Self(msg.to_string())
    }
}

impl std::error::Error for DeError {}

macro_rules! forward_to_u64 {
    ($method:ident, $visitor_method:ident) => {
        fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
            match self.0 {
                Value::U64(n) => visitor.$visitor_method(*n as _),
                Value::U32(n) => visitor.$visitor_method(*n as _),
                Value::U16(n) => visitor.$visitor_method(*n as _),
                Value::I32(n) => visitor.$visitor_method(*n as _),
                _ => Err(DeError(format!("expected number, got {:?}", self.0))),
            }
        }
    };
}

impl<'de> Deserializer<'de> for ValueDeserializer<'_, '_> {
    type Error = DeError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self.0 {
            Value::Str(s) => visitor.visit_str(s),
            Value::Bytes(b) => visitor.visit_bytes(b),
            Value::F64(n) => visitor.visit_f64(*n),
            Value::F32(n) => visitor.visit_f32(*n),
            Value::Bool(b) => visitor.visit_bool(*b),
            Value::I32(n) => visitor.visit_i32(*n),
            Value::U16(n) => visitor.visit_u16(*n),
            Value::U32(n) => visitor.visit_u32(*n),
            Value::U64(n) => visitor.visit_u64(*n),
            Value::U128(n) => visitor.visit_u128(*n),
            Value::Map(m) => visitor.visit_map(MapDeserializer {
                iter: m.iter(),
                current_value: None,
            }),
            Value::Array(arr) => visitor.visit_seq(SeqDeserializer { iter: arr.iter() }),
        }
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self.0 {
            Value::Bool(b) => visitor.visit_bool(*b),
            _ => Err(DeError(format!("expected bool, got {:?}", self.0))),
        }
    }

    forward_to_u64!(deserialize_i8, visit_i8);
    forward_to_u64!(deserialize_i16, visit_i16);
    forward_to_u64!(deserialize_i32, visit_i32);
    forward_to_u64!(deserialize_i64, visit_i64);
    forward_to_u64!(deserialize_u8, visit_u8);
    forward_to_u64!(deserialize_u16, visit_u16);
    forward_to_u64!(deserialize_u32, visit_u32);
    forward_to_u64!(deserialize_u64, visit_u64);

    fn deserialize_u128<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self.0 {
            Value::U128(n) => visitor.visit_u128(*n),
            Value::U64(n) => visitor.visit_u128(u128::from(*n)),
            Value::U32(n) => visitor.visit_u128(u128::from(*n)),
            Value::U16(n) => visitor.visit_u128(u128::from(*n)),
            _ => Err(DeError(format!("expected number, got {:?}", self.0))),
        }
    }

    fn deserialize_f32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self.0 {
            Value::F32(n) => visitor.visit_f32(*n),
            Value::F64(n) => visitor.visit_f32(*n as f32),
            _ => Err(DeError(format!("expected float, got {:?}", self.0))),
        }
    }

    fn deserialize_f64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self.0 {
            Value::F64(n) => visitor.visit_f64(*n),
            Value::F32(n) => visitor.visit_f64(f64::from(*n)),
            _ => Err(DeError(format!("expected float, got {:?}", self.0))),
        }
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self.0 {
            Value::Str(s) => visitor.visit_str(s),
            _ => Err(DeError(format!("expected string, got {:?}", self.0))),
        }
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.deserialize_str(visitor)
    }

    fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self.0 {
            Value::Bytes(b) => visitor.visit_bytes(b),
            _ => Err(DeError(format!("expected bytes, got {:?}", self.0))),
        }
    }

    fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.deserialize_bytes(visitor)
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_some(self)
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        visitor.visit_unit()
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self.0 {
            Value::Array(arr) => visitor.visit_seq(SeqDeserializer { iter: arr.iter() }),
            _ => Err(DeError(format!("expected array, got {:?}", self.0))),
        }
    }

    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self.0 {
            Value::Map(m) => visitor.visit_map(MapDeserializer {
                iter: m.iter(),
                current_value: None,
            }),
            _ => Err(DeError(format!("expected map, got {:?}", self.0))),
        }
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        match self.0 {
            Value::Map(m) => visitor.visit_map(MapDeserializer {
                iter: m.iter(),
                current_value: None,
            }),
            _ => Err(DeError(format!(
                "expected map for struct, got {:?}",
                self.0
            ))),
        }
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value, Self::Error> {
        Err(DeError("enum deserialization not supported".to_string()))
    }

    fn deserialize_identifier<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.deserialize_str(visitor)
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.deserialize_any(visitor)
    }

    fn deserialize_char<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.deserialize_str(visitor)
    }
}

struct MapDeserializer<'a, 'data> {
    iter: std::collections::hash_map::Iter<'a, &'data str, Value<'data>>,
    current_value: Option<&'a Value<'data>>,
}

impl<'de> MapAccess<'de> for MapDeserializer<'_, '_> {
    type Error = DeError;

    fn next_key_seed<K: de::DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Self::Error> {
        if let Some((key, val)) = self.iter.next() {
            self.current_value = Some(val);
            seed.deserialize(de::value::StrDeserializer::new(key))
                .map(Some)
        } else {
            Ok(None)
        }
    }

    fn next_value_seed<V: de::DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, Self::Error> {
        let val = self
            .current_value
            .take()
            .ok_or_else(|| DeError("no current value".to_string()))?;
        seed.deserialize(ValueDeserializer(val))
    }
}

struct SeqDeserializer<'a, 'data> {
    iter: std::slice::Iter<'a, Value<'data>>,
}

impl<'de> SeqAccess<'de> for SeqDeserializer<'_, '_> {
    type Error = DeError;

    fn next_element_seed<T: de::DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Self::Error> {
        if let Some(val) = self.iter.next() {
            seed.deserialize(ValueDeserializer(val)).map(Some)
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoded_payloads_borrow_database_storage() {
        let data = [0x43, b'a', b'b', b'c', 0x83, 1, 2, 3];
        let (text, next) = decode_value_at(&data, 0).unwrap();
        let Value::Str(text) = text else {
            panic!("expected string");
        };
        assert_eq!(text, "abc");
        assert_eq!(text.as_ptr(), data[1..].as_ptr());
        let (bytes, end) = decode_value_at(&data, next).unwrap();
        let Value::Bytes(bytes) = bytes else {
            panic!("expected bytes");
        };
        assert_eq!(bytes, [1, 2, 3]);
        assert_eq!(bytes.as_ptr(), data[5..].as_ptr());
        assert_eq!(end, data.len());
    }
}
