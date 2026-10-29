/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::OnceCell;
use std::collections::HashMap;

use xmlparser::{ElementEnd, Token, Tokenizer};

use super::{MAX_ATTRIBUTES, MAX_DEPTH, MAX_ELEMENTS, SVG_NS, XML_NS, XMLNS_NS};
use crate::VectorDecodeError;

pub(super) type InlineDeclarations = Vec<(String, String, bool)>;

#[derive(Debug)]
pub(super) struct Element<'a> {
    pub(super) name: &'a str,
    pub(super) prefix: &'a str,
    pub(super) attributes: Vec<(&'a str, &'a str, &'a str)>,
    pub(super) children: Vec<usize>,
    pub(super) parent: Option<usize>,
    pub(super) is_svg: bool,
    pub(super) text: String,
    pub(super) inline_style: OnceCell<Result<InlineDeclarations, VectorDecodeError>>,
}

impl Element<'_> {
    pub(super) fn attr(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|&&(prefix, local, _)| prefix.is_empty() && local == name)
            .map(|&(_, _, value)| value)
    }

    pub(super) fn attr_prefixed(&self, wanted_prefix: &str, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|&&(prefix, local, _)| prefix == wanted_prefix && local == name)
            .map(|&(_, _, value)| value)
    }
}

pub(super) struct XmlDocument<'a> {
    pub(super) elements: Vec<Element<'a>>,
    pub(super) root: usize,
    pub(super) ids: HashMap<&'a str, usize>,
}

impl<'a> XmlDocument<'a> {
    pub(super) fn parse(source: &'a str) -> Result<Self, VectorDecodeError> {
        let mut elements = Vec::<Element<'a>>::new();
        let mut stack = Vec::<usize>::new();
        let mut pending = None;
        let mut root = None;
        let mut ids = HashMap::new();
        let mut attributes = 0usize;
        for token in Tokenizer::from(source) {
            match token.map_err(|_| VectorDecodeError::InvalidData)? {
                Token::ElementStart { prefix, local, .. } => {
                    if pending.is_some() {
                        return Err(VectorDecodeError::InvalidData);
                    }
                    if elements.len() >= MAX_ELEMENTS || stack.len() >= MAX_DEPTH {
                        return Err(VectorDecodeError::ResourceLimit);
                    }
                    let index = elements.len();
                    elements.push(Element {
                        name: local.as_str(),
                        prefix: prefix.as_str(),
                        attributes: Vec::new(),
                        children: Vec::new(),
                        parent: None,
                        is_svg: false,
                        text: String::new(),
                        inline_style: OnceCell::new(),
                    });
                    pending = Some(index);
                }
                Token::Attribute {
                    prefix,
                    local,
                    value,
                    ..
                } => {
                    let index = pending.ok_or(VectorDecodeError::InvalidData)?;
                    attributes = attributes
                        .checked_add(1)
                        .ok_or(VectorDecodeError::ResourceLimit)?;
                    if attributes > MAX_ATTRIBUTES {
                        return Err(VectorDecodeError::ResourceLimit);
                    }
                    let name = local.as_str();
                    let prefix = prefix.as_str();
                    if elements[index]
                        .attributes
                        .iter()
                        .any(|&(p, n, _)| p == prefix && n == name)
                    {
                        return Err(VectorDecodeError::InvalidData);
                    }
                    let value = value.as_str();
                    if name == "id"
                        && prefix.is_empty()
                        && (value.is_empty() || ids.insert(value, index).is_some())
                    {
                        return Err(VectorDecodeError::InvalidData);
                    }
                    elements[index].attributes.push((prefix, name, value));
                }
                Token::ElementEnd { end, .. } => match end {
                    ElementEnd::Open | ElementEnd::Empty => {
                        let index = pending.take().ok_or(VectorDecodeError::InvalidData)?;
                        if let Some(&parent) = stack.last() {
                            elements[parent].children.push(index);
                            elements[index].parent = Some(parent);
                        } else {
                            if root.replace(index).is_some() {
                                return Err(VectorDecodeError::InvalidData);
                            }
                        }
                        if matches!(end, ElementEnd::Open) {
                            stack.push(index);
                        }
                    }
                    ElementEnd::Close(prefix, local) => {
                        if pending.is_some() {
                            return Err(VectorDecodeError::InvalidData);
                        }
                        let index = stack.pop().ok_or(VectorDecodeError::InvalidData)?;
                        if elements[index].name != local.as_str()
                            || elements[index].prefix != prefix.as_str()
                        {
                            return Err(VectorDecodeError::InvalidData);
                        }
                    }
                },
                Token::Text { text } | Token::Cdata { text, .. } => {
                    if stack.is_empty() && !text.as_str().trim().is_empty() {
                        return Err(VectorDecodeError::InvalidData);
                    }
                    if let Some(index) = stack.last() {
                        elements[*index].text.push_str(text.as_str());
                    }
                }
                Token::DtdStart { .. }
                | Token::EmptyDtd { .. }
                | Token::EntityDeclaration { .. } => return Err(VectorDecodeError::InvalidData),
                Token::ProcessingInstruction { .. }
                | Token::Declaration { .. }
                | Token::Comment { .. }
                | Token::DtdEnd { .. } => {}
            }
        }
        if pending.is_some() || !stack.is_empty() {
            return Err(VectorDecodeError::InvalidData);
        }
        let root = root.ok_or(VectorDecodeError::InvalidData)?;
        if elements[root].name != "svg" {
            return Err(VectorDecodeError::InvalidData);
        }
        if Self::element_namespace(&elements, root)?.is_some_and(|namespace| namespace != SVG_NS) {
            return Err(VectorDecodeError::InvalidData);
        }
        for index in 0..elements.len() {
            let namespace = Self::element_namespace(&elements, index)?;
            elements[index].is_svg = namespace.is_none_or(|namespace| namespace == SVG_NS);
            Self::validate_namespace_declarations(&elements[index])?;
            for &(prefix, _, _) in &elements[index].attributes {
                if !prefix.is_empty()
                    && prefix != "xmlns"
                    && prefix != "xml"
                    && Self::resolve_namespace(&elements, index, prefix).is_none()
                {
                    return Err(VectorDecodeError::InvalidData);
                }
            }
            for (position, &(prefix, name, _)) in elements[index].attributes.iter().enumerate() {
                if prefix == "xmlns" || prefix.is_empty() && name == "xmlns" {
                    continue;
                }
                let namespace = if prefix.is_empty() {
                    None
                } else {
                    Self::resolve_namespace(&elements, index, prefix)
                };
                if elements[index].attributes[..position].iter().any(
                    |&(other_prefix, other_name, _)| {
                        other_name == name
                            && other_prefix != "xmlns"
                            && !(other_prefix.is_empty() && other_name == "xmlns")
                            && (if other_prefix.is_empty() {
                                None
                            } else {
                                Self::resolve_namespace(&elements, index, other_prefix)
                            }) == namespace
                    },
                ) {
                    return Err(VectorDecodeError::InvalidData);
                }
            }
        }
        Ok(Self {
            elements,
            root,
            ids,
        })
    }
    fn element_namespace(
        elements: &[Element<'a>],
        index: usize,
    ) -> Result<Option<&'a str>, VectorDecodeError> {
        let prefix = elements[index].prefix;
        if prefix.is_empty() {
            Ok(Self::resolve_namespace(elements, index, ""))
        } else {
            Self::resolve_namespace(elements, index, prefix)
                .map(Some)
                .ok_or(VectorDecodeError::InvalidData)
        }
    }

    fn resolve_namespace(
        elements: &[Element<'a>],
        mut index: usize,
        prefix: &str,
    ) -> Option<&'a str> {
        if prefix == "xml" {
            return Some(XML_NS);
        }
        loop {
            let declaration =
                elements[index]
                    .attributes
                    .iter()
                    .find(|&&(attribute_prefix, name, _)| {
                        if prefix.is_empty() {
                            attribute_prefix.is_empty() && name == "xmlns"
                        } else {
                            attribute_prefix == "xmlns" && name == prefix
                        }
                    });
            if let Some(&(_, _, namespace)) = declaration {
                return Some(namespace);
            }
            index = elements[index].parent?;
        }
    }

    fn validate_namespace_declarations(element: &Element<'_>) -> Result<(), VectorDecodeError> {
        for &(prefix, name, value) in &element.attributes {
            if prefix.is_empty() && name == "xmlns" {
                if matches!(value, XML_NS | XMLNS_NS) {
                    return Err(VectorDecodeError::InvalidData);
                }
            } else if prefix == "xmlns"
                && (value.is_empty()
                    || name == "xmlns"
                    || name == "xml" && value != XML_NS
                    || name != "xml" && matches!(value, XML_NS | XMLNS_NS))
            {
                return Err(VectorDecodeError::InvalidData);
            }
        }
        Ok(())
    }
}
