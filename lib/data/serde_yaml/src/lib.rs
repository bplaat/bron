/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [serde_yaml](https://crates.io/crates/serde_yaml) crate

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use granit_parser::{Event, Parser, ScalarStyle, Tag};
use serde::de::{
    self, Deserialize, DeserializeSeed, EnumAccess, IntoDeserializer, MapAccess, SeqAccess,
    VariantAccess, Visitor,
};

/// An error produced while parsing or deserializing YAML.
#[derive(Debug)]
pub struct Error {
    message: String,
}

impl Error {
    fn custom(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

impl de::Error for Error {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self::custom(message.to_string())
    }
}

/// Deserialize one YAML document from a string.
pub fn from_str<'de, T>(input: &'de str) -> Result<T, Error>
where
    T: Deserialize<'de>,
{
    T::deserialize(parse(input)?)
}

#[derive(Clone)]
enum Node<'de> {
    Scalar(Scalar<'de>),
    Sequence(Rc<Vec<Node<'de>>>),
    Mapping(Rc<Vec<(Node<'de>, Node<'de>)>>),
}

#[derive(Clone)]
struct Scalar<'de> {
    value: Cow<'de, str>,
    plain: bool,
    tag: ScalarTag,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ScalarTag {
    Implicit,
    String,
    Bool,
    Integer,
    Float,
    Null,
}

enum Container<'de> {
    Sequence {
        values: Vec<Node<'de>>,
        anchor: usize,
    },
    Mapping {
        values: Vec<(Node<'de>, Node<'de>)>,
        key: Option<Node<'de>>,
        anchor: usize,
    },
}

fn scalar_tag(tag: Option<&Cow<'_, Tag>>) -> ScalarTag {
    match tag.map(|tag| tag.suffix()) {
        Some("str") => ScalarTag::String,
        Some("bool") => ScalarTag::Bool,
        Some("int") => ScalarTag::Integer,
        Some("float") => ScalarTag::Float,
        Some("null") => ScalarTag::Null,
        _ => ScalarTag::Implicit,
    }
}

fn parse(input: &str) -> Result<Node<'_>, Error> {
    let mut root = None;
    let mut stack = Vec::new();
    let mut anchors = HashMap::new();

    for next in Parser::new_from_str(input) {
        let (event, _) = next.map_err(|error| Error::custom(error.to_string()))?;
        match event {
            Event::StreamStart
            | Event::StreamEnd
            | Event::DocumentStart(_, _)
            | Event::DocumentEnd
            | Event::Comment(_, _) => {}
            Event::Scalar(value, style, anchor, tag) => {
                let node = Node::Scalar(Scalar {
                    value,
                    plain: style == ScalarStyle::Plain,
                    tag: scalar_tag(tag.as_ref()),
                });
                finish_node(node, anchor, &mut stack, &mut anchors, &mut root)?;
            }
            Event::SequenceStart(_, anchor, _) => {
                stack.push(Container::Sequence {
                    values: Vec::new(),
                    anchor,
                });
            }
            Event::SequenceEnd => {
                let Some(Container::Sequence { values, anchor }) = stack.pop() else {
                    return Err(Error::custom("unexpected YAML sequence end"));
                };
                finish_node(
                    Node::Sequence(Rc::new(values)),
                    anchor,
                    &mut stack,
                    &mut anchors,
                    &mut root,
                )?;
            }
            Event::MappingStart(_, anchor, _) => {
                stack.push(Container::Mapping {
                    values: Vec::new(),
                    key: None,
                    anchor,
                });
            }
            Event::MappingEnd => {
                let Some(Container::Mapping {
                    values,
                    key,
                    anchor,
                }) = stack.pop()
                else {
                    return Err(Error::custom("unexpected YAML mapping end"));
                };
                if key.is_some() {
                    return Err(Error::custom("YAML mapping key has no value"));
                }
                finish_node(
                    Node::Mapping(Rc::new(values)),
                    anchor,
                    &mut stack,
                    &mut anchors,
                    &mut root,
                )?;
            }
            Event::Alias(anchor) => {
                let node = anchors
                    .get(&anchor)
                    .cloned()
                    .ok_or_else(|| Error::custom(format!("unknown YAML anchor {anchor}")))?;
                finish_node(node, 0, &mut stack, &mut anchors, &mut root)?;
            }
            _ => return Err(Error::custom("unsupported YAML parser event")),
        }
    }

    if !stack.is_empty() {
        return Err(Error::custom("unfinished YAML collection"));
    }
    root.ok_or_else(|| Error::custom("YAML document is empty"))
}

fn finish_node<'de>(
    node: Node<'de>,
    anchor: usize,
    stack: &mut [Container<'de>],
    anchors: &mut HashMap<usize, Node<'de>>,
    root: &mut Option<Node<'de>>,
) -> Result<(), Error> {
    if anchor != 0 {
        anchors.insert(anchor, node.clone());
    }
    match stack.last_mut() {
        Some(Container::Sequence { values, .. }) => values.push(node),
        Some(Container::Mapping { values, key, .. }) => {
            if let Some(key) = key.take() {
                values.push((key, node));
            } else {
                *key = Some(node);
            }
        }
        None => {
            if root.replace(node).is_some() {
                return Err(Error::custom("multiple YAML documents are not supported"));
            }
        }
    }
    Ok(())
}

impl Scalar<'_> {
    fn is_null(&self) -> bool {
        self.tag == ScalarTag::Null
            || (self.tag == ScalarTag::Implicit
                && self.plain
                && matches!(self.value.as_ref(), "" | "~" | "null" | "Null" | "NULL"))
    }

    fn as_bool(&self) -> Option<bool> {
        match self.value.as_ref() {
            "true" | "True" | "TRUE" => Some(true),
            "false" | "False" | "FALSE" => Some(false),
            _ => None,
        }
    }
}

fn parse_signed(value: &str) -> Result<i128, Error> {
    let value = value.replace('_', "");
    let (negative, digits) = value
        .strip_prefix('-')
        .map_or((false, value.as_str()), |digits| (true, digits));
    let digits = digits.trim_prefix('+');
    let (radix, digits) = if let Some(digits) = digits.strip_prefix("0x") {
        (16, digits)
    } else if let Some(digits) = digits.strip_prefix("0o") {
        (8, digits)
    } else if let Some(digits) = digits.strip_prefix("0b") {
        (2, digits)
    } else {
        (10, digits)
    };
    let number = i128::from_str_radix(digits, radix)
        .map_err(|_| Error::custom(format!("invalid integer `{value}`")))?;
    Ok(if negative { -number } else { number })
}

fn parse_unsigned(value: &str) -> Result<u128, Error> {
    let value = value.replace('_', "");
    let digits = value.trim_prefix('+');
    let (radix, digits) = if let Some(digits) = digits.strip_prefix("0x") {
        (16, digits)
    } else if let Some(digits) = digits.strip_prefix("0o") {
        (8, digits)
    } else if let Some(digits) = digits.strip_prefix("0b") {
        (2, digits)
    } else {
        (10, digits)
    };
    u128::from_str_radix(digits, radix)
        .map_err(|_| Error::custom(format!("invalid unsigned integer `{value}`")))
}

fn visit_string<'de, V>(value: Cow<'de, str>, visitor: V) -> Result<V::Value, Error>
where
    V: Visitor<'de>,
{
    match value {
        Cow::Borrowed(value) => visitor.visit_borrowed_str(value),
        Cow::Owned(value) => visitor.visit_string(value),
    }
}

macro_rules! deserialize_signed {
    ($name:ident, $type:ty, $visit:ident) => {
        fn $name<V>(self, visitor: V) -> Result<V::Value, Error>
        where
            V: Visitor<'de>,
        {
            let Node::Scalar(scalar) = self else {
                return Err(Error::custom("expected a YAML integer"));
            };
            let value = parse_signed(&scalar.value)?;
            let value = <$type>::try_from(value)
                .map_err(|_| Error::custom(format!("integer `{value}` is out of range")))?;
            visitor.$visit(value)
        }
    };
}

macro_rules! deserialize_unsigned {
    ($name:ident, $type:ty, $visit:ident) => {
        fn $name<V>(self, visitor: V) -> Result<V::Value, Error>
        where
            V: Visitor<'de>,
        {
            let Node::Scalar(scalar) = self else {
                return Err(Error::custom("expected a YAML unsigned integer"));
            };
            let value = parse_unsigned(&scalar.value)?;
            let value = <$type>::try_from(value)
                .map_err(|_| Error::custom(format!("integer `{value}` is out of range")))?;
            visitor.$visit(value)
        }
    };
}

impl<'de> de::Deserializer<'de> for Node<'de> {
    type Error = Error;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        match self {
            Self::Sequence(values) => visitor.visit_seq(NodeSeqAccess {
                values: Rc::unwrap_or_clone(values).into_iter(),
            }),
            Self::Mapping(values) => visitor.visit_map(NodeMapAccess {
                values: Rc::unwrap_or_clone(values).into_iter(),
                value: None,
            }),
            Self::Scalar(scalar) => {
                if scalar.is_null() {
                    visitor.visit_unit()
                } else if scalar.tag == ScalarTag::Bool
                    || (scalar.tag == ScalarTag::Implicit
                        && scalar.plain
                        && scalar.as_bool().is_some())
                {
                    visitor.visit_bool(
                        scalar
                            .as_bool()
                            .ok_or_else(|| Error::custom("invalid YAML boolean"))?,
                    )
                } else if scalar.tag == ScalarTag::Integer
                    || (scalar.tag == ScalarTag::Implicit
                        && scalar.plain
                        && parse_signed(&scalar.value).is_ok())
                {
                    visitor.visit_i128(parse_signed(&scalar.value)?)
                } else if scalar.tag == ScalarTag::Float
                    || (scalar.tag == ScalarTag::Implicit
                        && scalar.plain
                        && scalar.value.parse::<f64>().is_ok())
                {
                    visitor.visit_f64(
                        scalar
                            .value
                            .parse()
                            .map_err(|_| Error::custom("invalid YAML float"))?,
                    )
                } else {
                    visit_string(scalar.value, visitor)
                }
            }
        }
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        let Self::Scalar(scalar) = self else {
            return Err(Error::custom("expected a YAML boolean"));
        };
        visitor.visit_bool(
            scalar
                .as_bool()
                .ok_or_else(|| Error::custom(format!("invalid boolean `{}`", scalar.value)))?,
        )
    }

    deserialize_signed!(deserialize_i8, i8, visit_i8);
    deserialize_signed!(deserialize_i16, i16, visit_i16);
    deserialize_signed!(deserialize_i32, i32, visit_i32);
    deserialize_signed!(deserialize_i64, i64, visit_i64);
    deserialize_signed!(deserialize_i128, i128, visit_i128);
    deserialize_unsigned!(deserialize_u8, u8, visit_u8);
    deserialize_unsigned!(deserialize_u16, u16, visit_u16);
    deserialize_unsigned!(deserialize_u32, u32, visit_u32);
    deserialize_unsigned!(deserialize_u64, u64, visit_u64);
    deserialize_unsigned!(deserialize_u128, u128, visit_u128);

    fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        let Self::Scalar(scalar) = self else {
            return Err(Error::custom("expected a YAML float"));
        };
        visitor.visit_f32(
            scalar
                .value
                .parse()
                .map_err(|_| Error::custom(format!("invalid float `{}`", scalar.value)))?,
        )
    }

    fn deserialize_f64<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        let Self::Scalar(scalar) = self else {
            return Err(Error::custom("expected a YAML float"));
        };
        visitor.visit_f64(
            scalar
                .value
                .parse()
                .map_err(|_| Error::custom(format!("invalid float `{}`", scalar.value)))?,
        )
    }

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        let Self::Scalar(scalar) = self else {
            return Err(Error::custom("expected a YAML character"));
        };
        let mut chars = scalar.value.chars();
        let character = chars
            .next()
            .ok_or_else(|| Error::custom("expected one character, found an empty string"))?;
        if chars.next().is_some() {
            return Err(Error::custom("expected one character"));
        }
        visitor.visit_char(character)
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        let Self::Scalar(scalar) = self else {
            return Err(Error::custom("expected a YAML string"));
        };
        visit_string(scalar.value, visitor)
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        let Self::Scalar(scalar) = self else {
            return Err(Error::custom("expected YAML bytes"));
        };
        match scalar.value {
            Cow::Borrowed(value) => visitor.visit_borrowed_bytes(value.as_bytes()),
            Cow::Owned(value) => visitor.visit_byte_buf(value.into_bytes()),
        }
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_bytes(visitor)
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        if matches!(&self, Self::Scalar(scalar) if scalar.is_null()) {
            visitor.visit_none()
        } else {
            visitor.visit_some(self)
        }
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        if matches!(&self, Self::Scalar(scalar) if scalar.is_null()) {
            visitor.visit_unit()
        } else {
            Err(Error::custom("expected YAML null"))
        }
    }

    fn deserialize_unit_struct<V>(self, _name: &'static str, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        let Self::Sequence(values) = self else {
            return Err(Error::custom("expected a YAML sequence"));
        };
        visitor.visit_seq(NodeSeqAccess {
            values: Rc::unwrap_or_clone(values).into_iter(),
        })
    }

    fn deserialize_tuple<V>(self, _length: usize, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        _length: usize,
        visitor: V,
    ) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        let Self::Mapping(values) = self else {
            return Err(Error::custom("expected a YAML mapping"));
        };
        visitor.visit_map(NodeMapAccess {
            values: Rc::unwrap_or_clone(values).into_iter(),
            value: None,
        })
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_map(visitor)
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        match self {
            Self::Scalar(scalar) => visitor.visit_enum(ScalarEnumAccess {
                variant: scalar.value,
            }),
            Self::Mapping(values) if values.len() == 1 => {
                let (variant, value) = Rc::unwrap_or_clone(values)
                    .pop()
                    .ok_or_else(|| Error::custom("expected one YAML enum variant"))?;
                let Self::Scalar(variant) = variant else {
                    return Err(Error::custom("YAML enum variant must be a string"));
                };
                visitor.visit_enum(NodeEnumAccess {
                    variant: variant.value,
                    value,
                })
            }
            _ => Err(Error::custom("expected a YAML enum")),
        }
    }

    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_unit()
    }
}

struct NodeSeqAccess<'de> {
    values: std::vec::IntoIter<Node<'de>>,
}

impl<'de> SeqAccess<'de> for NodeSeqAccess<'de> {
    type Error = Error;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Error>
    where
        T: DeserializeSeed<'de>,
    {
        self.values
            .next()
            .map(|value| seed.deserialize(value))
            .transpose()
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.values.len())
    }
}

struct NodeMapAccess<'de> {
    values: std::vec::IntoIter<(Node<'de>, Node<'de>)>,
    value: Option<Node<'de>>,
}

impl<'de> MapAccess<'de> for NodeMapAccess<'de> {
    type Error = Error;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Error>
    where
        K: DeserializeSeed<'de>,
    {
        let Some((key, value)) = self.values.next() else {
            return Ok(None);
        };
        self.value = Some(value);
        seed.deserialize(key).map(Some)
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Error>
    where
        V: DeserializeSeed<'de>,
    {
        seed.deserialize(
            self.value
                .take()
                .ok_or_else(|| Error::custom("YAML mapping value requested before its key"))?,
        )
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.values.len())
    }
}

struct ScalarEnumAccess<'de> {
    variant: Cow<'de, str>,
}

impl<'de> EnumAccess<'de> for ScalarEnumAccess<'de> {
    type Error = Error;
    type Variant = UnitVariantAccess;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), Error>
    where
        V: DeserializeSeed<'de>,
    {
        let variant = seed.deserialize(self.variant.into_deserializer())?;
        Ok((variant, UnitVariantAccess))
    }
}

struct UnitVariantAccess;

impl<'de> VariantAccess<'de> for UnitVariantAccess {
    type Error = Error;

    fn unit_variant(self) -> Result<(), Error> {
        Ok(())
    }

    fn newtype_variant_seed<T>(self, _seed: T) -> Result<T::Value, Error>
    where
        T: DeserializeSeed<'de>,
    {
        Err(Error::custom("expected a YAML unit enum variant"))
    }

    fn tuple_variant<V>(self, _length: usize, _visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        Err(Error::custom("expected a YAML unit enum variant"))
    }

    fn struct_variant<V>(
        self,
        _fields: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        Err(Error::custom("expected a YAML unit enum variant"))
    }
}

struct NodeEnumAccess<'de> {
    variant: Cow<'de, str>,
    value: Node<'de>,
}

impl<'de> EnumAccess<'de> for NodeEnumAccess<'de> {
    type Error = Error;
    type Variant = NodeVariantAccess<'de>;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), Error>
    where
        V: DeserializeSeed<'de>,
    {
        let variant = seed.deserialize(self.variant.into_deserializer())?;
        Ok((variant, NodeVariantAccess { value: self.value }))
    }
}

struct NodeVariantAccess<'de> {
    value: Node<'de>,
}

impl<'de> VariantAccess<'de> for NodeVariantAccess<'de> {
    type Error = Error;

    fn unit_variant(self) -> Result<(), Error> {
        Deserialize::deserialize(self.value)
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, Error>
    where
        T: DeserializeSeed<'de>,
    {
        seed.deserialize(self.value)
    }

    fn tuple_variant<V>(self, length: usize, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        de::Deserializer::deserialize_tuple(self.value, length, visitor)
    }

    fn struct_variant<V>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        de::Deserializer::deserialize_struct(self.value, "", fields, visitor)
    }
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde::Deserialize;

    #[derive(Debug, Deserialize, Eq, PartialEq)]
    struct Config {
        name: String,
        enabled: bool,
        ports: Vec<u16>,
        labels: BTreeMap<String, String>,
        optional: Option<String>,
    }

    #[test]
    fn deserialize_typed_document() {
        let config: Config = super::from_str(
            "name: demo\nenabled: true\nports: [80, 443]\nlabels:\n  env: test\noptional: null\n",
        )
        .expect("valid YAML");
        assert_eq!(
            config,
            Config {
                name: "demo".to_string(),
                enabled: true,
                ports: vec![80, 443],
                labels: BTreeMap::from([("env".to_string(), "test".to_string())]),
                optional: None,
            }
        );
    }

    #[test]
    fn deserialize_collection_aliases_with_owned_strings() {
        let values: BTreeMap<String, Vec<BTreeMap<String, String>>> =
            super::from_str("original: &rows [{name: \"line\\nnext\"}]\nalias: *rows\n")
                .expect("valid YAML");
        assert_eq!(values["original"], values["alias"]);
        assert_eq!(values["alias"][0]["name"], "line\nnext");
    }

    #[test]
    fn ignored_nested_aliases_do_not_expand_the_tree() {
        let mut input = String::from("a0: &a0 [value]\n");
        for index in 1..24 {
            let previous = index - 1;
            input.push_str(&format!(
                "a{index}: &a{index} [*a{previous}, *a{previous}]\n"
            ));
        }
        super::from_str::<serde::de::IgnoredAny>(&input).expect("valid YAML aliases");
    }

    #[test]
    fn deserialize_anchor_alias() {
        let values: Vec<String> = super::from_str("- &name demo\n- *name\n").expect("valid YAML");
        assert_eq!(values, ["demo", "demo"]);
    }
}
