/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::borrow::Cow;
use std::io::{self, Write};
use std::slice::{Iter, IterMut};

/// HeaderMap
#[derive(Default, Clone)]
pub struct HeaderMap(Vec<(String, String)>);

impl HeaderMap {
    /// Create new HeaderMap
    pub fn new() -> Self {
        Self::default()
    }

    /// Get header value
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Get all header values
    pub fn get_all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.0.iter().filter_map(move |(header_name, value)| {
            header_name
                .eq_ignore_ascii_case(name)
                .then_some(value.as_str())
        })
    }

    /// Insert or replace a header case-insensitively
    pub fn insert(&mut self, name: String, value: String) {
        self.remove(&name);
        self.append(name, value);
    }

    /// Append a header without replacing existing values
    pub fn append(&mut self, name: String, value: String) {
        self.0.push((name, value));
    }

    /// Remove all values for a header case-insensitively
    pub fn remove(&mut self, name: &str) -> Option<String> {
        let mut first_value = None;
        self.0.retain_mut(|(header_name, value)| {
            if header_name.eq_ignore_ascii_case(name) {
                if first_value.is_none() {
                    first_value = Some(std::mem::take(value));
                }
                false
            } else {
                true
            }
        });
        first_value
    }

    /// Check whether a header exists case-insensitively
    pub fn contains_key(&self, name: &str) -> bool {
        self.0
            .iter()
            .any(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
    }

    /// Get number of headers
    pub const fn len(&self) -> usize {
        self.0.len()
    }

    /// Is empty
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Get iterator
    pub fn iter(&self) -> Iter<'_, (String, String)> {
        self.0.iter()
    }

    /// Write all headers as lines ending in `line_end`, stripping CR and LF to prevent injection
    pub(crate) fn write_to(&self, stream: &mut dyn Write, line_end: &str) -> io::Result<()> {
        fn sanitize(text: &str) -> Cow<'_, str> {
            if text.contains(['\r', '\n']) {
                Cow::Owned(text.replace(['\r', '\n'], ""))
            } else {
                Cow::Borrowed(text)
            }
        }
        for (name, value) in &self.0 {
            write!(stream, "{}: {}{line_end}", sanitize(name), sanitize(value))?;
        }
        Ok(())
    }
}

impl IntoIterator for HeaderMap {
    type Item = (String, String);
    type IntoIter = std::vec::IntoIter<(String, String)>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a HeaderMap {
    type Item = &'a (String, String);
    type IntoIter = Iter<'a, (String, String)>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<'a> IntoIterator for &'a mut HeaderMap {
    type Item = &'a mut (String, String);
    type IntoIter = IterMut<'a, (String, String)>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_replaces_values_case_insensitively() {
        let mut headers = HeaderMap::new();
        headers.append("Content-Type".to_string(), "text/plain".to_string());
        headers.append("content-type".to_string(), "text/html".to_string());

        headers.insert("CONTENT-TYPE".to_string(), "application/json".to_string());

        assert_eq!(headers.get("content-type"), Some("application/json"));
        assert_eq!(
            headers.get_all("Content-Type").collect::<Vec<_>>(),
            ["application/json"]
        );
        assert_eq!(headers.len(), 1);
    }

    #[test]
    fn append_preserves_repeated_values() {
        let mut headers = HeaderMap::new();
        headers.append("Set-Cookie".to_string(), "one=1".to_string());
        headers.append("set-cookie".to_string(), "two=2".to_string());

        assert_eq!(
            headers.get_all("SET-COOKIE").collect::<Vec<_>>(),
            ["one=1", "two=2"]
        );
    }

    #[test]
    fn remove_and_contains_key_are_case_insensitive() {
        let mut headers = HeaderMap::new();
        headers.append("X-Test".to_string(), "first".to_string());
        headers.append("x-test".to_string(), "second".to_string());

        assert!(headers.contains_key("X-TEST"));
        assert_eq!(headers.remove("x-Test"), Some("first".to_string()));
        assert!(!headers.contains_key("X-Test"));
        assert!(headers.is_empty());
    }

    #[test]
    fn write_to_strips_line_breaks() {
        let mut headers = HeaderMap::new();
        headers.append("X-Te\r\nst".to_string(), "va\nlue".to_string());
        headers.append("Host".to_string(), "localhost".to_string());

        let mut output = Vec::new();
        headers.write_to(&mut output, "\r\n").unwrap();
        assert_eq!(output, b"X-Test: value\r\nHost: localhost\r\n");
    }
}
