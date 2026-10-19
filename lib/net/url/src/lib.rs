/*
 * Copyright (c) 2024-2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [url](https://crates.io/crates/url) crate.
//!
//! This parser supports RFC 3986-style absolute URLs with an authority and resolves relative
//! references against them. It intentionally does not implement the full WHATWG URL Standard or
//! internationalized domain names.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::error::Error;
use core::fmt::{self, Display, Formatter};
use core::str::FromStr;

// MARK: URL
/// Url
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Url {
    scheme: String,
    authority: Option<Authority>,
    path: String,
    query: Option<String>,
    fragment: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Authority {
    userinfo: Option<String>,
    host: String,
    port: Option<u16>,
    is_ipv6: bool,
}

impl Url {
    /// Parse a new URL from a string
    pub fn parse(s: &str) -> Result<Self, ParseError> {
        Self::from_str(s)
    }

    /// Get the URL scheme
    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    /// Get the URL authority
    pub fn userinfo(&self) -> Option<&str> {
        self.authority
            .as_ref()
            .and_then(|auth| auth.userinfo.as_deref())
    }

    /// Get the URL host
    pub fn host(&self) -> Option<&str> {
        self.authority.as_ref().map(|auth| auth.host.as_str())
    }

    /// Get the URL domain
    pub fn domain(&self) -> Option<&str> {
        self.authority.as_ref().and_then(|auth| {
            (!auth.is_ipv6 && auth.host.parse::<core::net::Ipv4Addr>().is_err())
                .then_some(auth.host.as_str())
        })
    }

    /// Get the URL port
    pub fn port(&self) -> Option<u16> {
        self.authority.as_ref().and_then(|auth| auth.port)
    }

    /// Get the URL path
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Get the URL query
    pub fn query(&self) -> Option<&str> {
        self.query.as_deref()
    }

    /// Get the URL fragment
    pub fn fragment(&self) -> Option<&str> {
        self.fragment.as_deref()
    }

    /// Resolve a URL reference against this URL (RFC 3986 section 5.2)
    pub fn join(&self, input: &str) -> Result<Self, ParseError> {
        let scheme_end = input.find([':', '/', '?', '#']);
        if scheme_end.is_some_and(|end| {
            input[end..].starts_with(':') && validate_scheme(&input[..end]).is_ok()
        }) {
            return input.parse();
        }
        if input.starts_with("//") {
            return format!("{}:{input}", self.scheme).parse();
        }

        let mut origin = format!("{}://", self.scheme);
        if let Some(authority) = &self.authority {
            origin.push_str(&authority.to_string());
        }
        let query = self
            .query
            .as_ref()
            .map_or(String::new(), |query| format!("?{query}"));
        let reference = match input.chars().next() {
            None => format!("{}{query}", self.path),
            Some('#') => format!("{}{query}{input}", self.path),
            Some('?') => format!("{}{input}", self.path),
            Some('/') => input.to_string(),
            Some(_) => {
                let directory = self.path.rsplit_once('/').map_or("", |(path, _)| path);
                format!("{directory}/{input}")
            }
        };
        let (path, rest) =
            reference.split_at(reference.find(['?', '#']).unwrap_or(reference.len()));
        format!("{origin}{}{rest}", remove_dot_segments(path)).parse()
    }
}

fn remove_dot_segments(path: &str) -> String {
    let mut segments = Vec::new();
    for segment in path.split('/').skip(1) {
        match segment {
            "." => {}
            ".." => _ = segments.pop(),
            segment => segments.push(segment),
        }
    }
    if path.ends_with("/.") || path.ends_with("/..") {
        segments.push("");
    }
    format!("/{}", segments.join("/"))
}

impl FromStr for Url {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (scheme, remainder) = s
            .split_once("://")
            .ok_or_else(|| ParseError("URL must contain an authority".to_string()))?;
        validate_scheme(scheme)?;

        let authority_end = remainder.find(['/', '?', '#']).unwrap_or(remainder.len());
        let authority = parse_authority(&remainder[..authority_end])?;
        let remainder = &remainder[authority_end..];

        // A fragment is parsed before a query because '?' is allowed inside a fragment, while '#'
        // terminates a query.
        let (path_and_query, fragment) = match remainder.split_once('#') {
            Some((before, value)) => (before, Some(value)),
            None => (remainder, None),
        };
        let (path, query) = match path_and_query.split_once('?') {
            Some((value, query)) => (value, Some(query)),
            None => (path_and_query, None),
        };
        let path = normalize_path(if path.is_empty() { "/" } else { path })?;
        let query = query
            .map(|value| normalize_query_or_fragment(value, "query"))
            .transpose()?;
        let fragment = fragment
            .map(|value| normalize_query_or_fragment(value, "fragment"))
            .transpose()?;

        Ok(Url {
            scheme: scheme.to_ascii_lowercase(),
            authority: Some(authority),
            path,
            query,
            fragment,
        })
    }
}

fn validate_scheme(scheme: &str) -> Result<(), ParseError> {
    let mut chars = scheme.chars();
    if !chars.next().is_some_and(|ch| ch.is_ascii_alphabetic())
        || !chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
    {
        return Err(ParseError("Invalid scheme".to_string()));
    }
    Ok(())
}

fn parse_authority(authority: &str) -> Result<Authority, ParseError> {
    if authority.is_empty() {
        return Err(ParseError("Host is empty".to_string()));
    }

    let (userinfo, host_and_port) = match authority.rsplit_once('@') {
        Some((userinfo, host_and_port)) => {
            let userinfo = normalize_component(userinfo, "userinfo", |byte| {
                is_unreserved(byte) || is_sub_delimiter(byte) || byte == b':'
            })?;
            (Some(userinfo), host_and_port)
        }
        None => (None, authority),
    };

    let (host, port, is_ipv6) = if let Some(address) = host_and_port.strip_prefix('[') {
        let closing = address
            .find(']')
            .ok_or_else(|| ParseError("IPv6 host is missing a closing bracket".to_string()))?;
        let host = &address[..closing];
        let suffix = &address[closing + 1..];
        let address = host
            .parse::<core::net::Ipv6Addr>()
            .map_err(|_| ParseError("Invalid IPv6 host".to_string()))?;
        let port = parse_port_suffix(suffix)?;
        (address.to_string(), port, true)
    } else {
        if host_and_port.contains(['[', ']']) {
            return Err(ParseError("Invalid host brackets".to_string()));
        }
        let (host, port) = match host_and_port.rsplit_once(':') {
            Some((host, port)) => {
                if host.contains(':') {
                    return Err(ParseError(
                        "IPv6 hosts must be enclosed in brackets".to_string(),
                    ));
                }
                (host, Some(parse_port(port)?))
            }
            None => (host_and_port, None),
        };
        validate_component(host, "host", |byte| {
            is_unreserved(byte) || is_sub_delimiter(byte)
        })?;
        (host.to_ascii_lowercase(), port, false)
    };

    if host.is_empty() {
        return Err(ParseError("Host is empty".to_string()));
    }
    Ok(Authority {
        userinfo,
        host,
        port,
        is_ipv6,
    })
}

fn parse_port_suffix(suffix: &str) -> Result<Option<u16>, ParseError> {
    if suffix.is_empty() {
        Ok(None)
    } else if let Some(port) = suffix.strip_prefix(':') {
        parse_port(port).map(Some)
    } else {
        Err(ParseError("Invalid characters after IPv6 host".to_string()))
    }
}

fn parse_port(port: &str) -> Result<u16, ParseError> {
    if port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ParseError("Invalid port".to_string()));
    }
    port.parse()
        .map_err(|_| ParseError("Port is out of range".to_string()))
}

fn normalize_path(path: &str) -> Result<String, ParseError> {
    if !path.starts_with('/') {
        return Err(ParseError(
            "Path after an authority must start with '/'".to_string(),
        ));
    }
    normalize_component(path, "path", |byte| is_path_character(byte) || byte == b'/')
}

fn normalize_query_or_fragment(value: &str, name: &str) -> Result<String, ParseError> {
    normalize_component(value, name, |byte| {
        is_path_character(byte) || matches!(byte, b'/' | b'?')
    })
}

fn normalize_component(
    value: &str,
    name: &str,
    is_allowed: impl Fn(u8) -> bool,
) -> Result<String, ParseError> {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";

    let bytes = value.as_bytes();
    let mut normalized = String::with_capacity(value.len());
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'%' {
            check_percent_escape(bytes, index, name)?;
            normalized.push_str(&value[index..index + 3]);
            index += 3;
        } else if byte.is_ascii() && is_allowed(byte) {
            normalized.push(byte as char);
            index += 1;
        } else if byte.is_ascii_control() || byte == b'\\' {
            return Err(ParseError(format!("Invalid character in {name}")));
        } else {
            normalized.push('%');
            normalized.push(HEX[(byte >> 4) as usize] as char);
            normalized.push(HEX[(byte & 0x0f) as usize] as char);
            index += 1;
        }
    }
    Ok(normalized)
}

fn validate_component(
    value: &str,
    name: &str,
    is_allowed: impl Fn(u8) -> bool,
) -> Result<(), ParseError> {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'%' {
            check_percent_escape(bytes, index, name)?;
            index += 3;
        } else if byte.is_ascii() && is_allowed(byte) {
            index += 1;
        } else {
            return Err(ParseError(format!("Invalid character in {name}")));
        }
    }
    Ok(())
}

/// Check that the `%` at `index` is followed by two hex digits
fn check_percent_escape(bytes: &[u8], index: usize, name: &str) -> Result<(), ParseError> {
    if bytes
        .get(index + 1..index + 3)
        .is_some_and(|hex| hex.iter().all(u8::is_ascii_hexdigit))
    {
        Ok(())
    } else {
        Err(ParseError(format!("Invalid percent escape in {name}")))
    }
}

const fn is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

const fn is_sub_delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'='
    )
}

const fn is_path_character(byte: u8) -> bool {
    is_unreserved(byte) || is_sub_delimiter(byte) || matches!(byte, b':' | b'@')
}

impl Display for Url {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}://", self.scheme)?;
        if let Some(authority) = &self.authority {
            write!(f, "{authority}")?;
        }
        write!(f, "{}", self.path)?;
        if let Some(query) = &self.query {
            write!(f, "?{query}")?;
        }
        if let Some(fragment) = &self.fragment {
            write!(f, "#{fragment}")?;
        }
        Ok(())
    }
}

impl Display for Authority {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if let Some(userinfo) = &self.userinfo {
            write!(f, "{userinfo}@")?;
        }
        if self.is_ipv6 {
            write!(f, "[{}]", self.host)?;
        } else {
            write!(f, "{}", self.host)?;
        }
        if let Some(port) = self.port {
            write!(f, ":{port}")?;
        }
        Ok(())
    }
}

// MARK: ParseError
/// Url parser error
#[derive(Debug)]
pub struct ParseError(String);

impl Display for ParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "URL parse error: {}", self.0)
    }
}

impl Error for ParseError {}

// MARK: Tests
#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parses_common_absolute_urls() {
        let urls = [
            "http://example.com",
            "http://example.com/",
            "http://example.com/?",
            "http://example.com/#",
            "http://example.com/?#",
            "http://example.com/path",
            "http://example.com/path/",
            "http://example.com/path/?",
            "http://example.com/path/#",
            "http://example.com/path/?#",
            "http://example.com/path?query",
            "http://example.com/path/?query",
            "http://example.com/path#fragment",
            "http://example.com/path/#fragment",
            "http://example.com/path?query#fragment",
            "http://example.com/path/?query#fragment",
            "http://user:pass@example.com",
            "http://user:pass@example.com/",
            "http://user:pass@example.com/path",
            "http://user:pass@example.com/path?query",
            "http://user:pass@example.com/path#fragment",
            "http://user:pass@example.com/path?query#fragment",
            "http://example.com:8080",
            "http://example.com:8080/",
            "http://example.com:8080/path",
            "http://example.com:8080/path?query",
            "http://example.com:8080/path#fragment",
            "http://example.com:8080/path?query#fragment",
            "ws://example.com/",
            "wss://example.com/",
            "ws://example.com:8080/",
            "wss://example.com:8080/",
            "ws://example.com/path",
            "wss://example.com/path",
            "ws://example.com/path?query",
            "wss://example.com/path?query",
            "ws://example.com/path#fragment",
            "wss://example.com/path#fragment",
        ];
        for input in urls {
            assert!(Url::from_str(input).is_ok(), "URL should parse: {input}");
        }
    }

    #[test]
    fn joins_relative_references() {
        // Examples from RFC 3986 section 5.4
        let base = Url::parse("http://a/b/c/d;p?q").unwrap();
        let cases = [
            ("g:h", None),
            ("https://example.com/x", Some("https://example.com/x")),
            ("g", Some("http://a/b/c/g")),
            ("./g", Some("http://a/b/c/g")),
            ("g/", Some("http://a/b/c/g/")),
            ("/g", Some("http://a/g")),
            ("//g", Some("http://g/")),
            ("?y", Some("http://a/b/c/d;p?y")),
            ("g?y", Some("http://a/b/c/g?y")),
            ("#s", Some("http://a/b/c/d;p?q#s")),
            ("g#s", Some("http://a/b/c/g#s")),
            ("", Some("http://a/b/c/d;p?q")),
            (".", Some("http://a/b/c/")),
            ("./", Some("http://a/b/c/")),
            ("..", Some("http://a/b/")),
            ("../g", Some("http://a/b/g")),
            ("../..", Some("http://a/")),
            ("../../../g", Some("http://a/g")),
            ("/./g", Some("http://a/g")),
            ("g/../h", Some("http://a/b/c/h")),
            ("g?y/./x", Some("http://a/b/c/g?y/./x")),
        ];
        for (input, expected) in cases {
            let joined = base.join(input).ok().map(|url| url.to_string());
            assert_eq!(joined.as_deref(), expected, "join: {input}");
        }
    }

    #[test]
    fn parses_components_without_a_path() {
        let url = Url::parse("HTTPS://user:pass@Example.COM:8443?search=test#results").unwrap();

        assert_eq!(url.scheme(), "https");
        assert_eq!(url.userinfo(), Some("user:pass"));
        assert_eq!(url.host(), Some("example.com"));
        assert_eq!(url.domain(), Some("example.com"));
        assert_eq!(url.port(), Some(8443));
        assert_eq!(url.path(), "/");
        assert_eq!(url.query(), Some("search=test"));
        assert_eq!(url.fragment(), Some("results"));
        assert_eq!(
            url.to_string(),
            "https://user:pass@example.com:8443/?search=test#results"
        );
    }

    #[test]
    fn separates_query_and_fragment_in_the_correct_order() {
        let url = Url::parse("https://example.com/path?one#two?three").unwrap();

        assert_eq!(url.path(), "/path");
        assert_eq!(url.query(), Some("one"));
        assert_eq!(url.fragment(), Some("two?three"));
    }

    #[test]
    fn parses_ipv4_and_ipv6_hosts() {
        let ipv4 = Url::parse("http://127.0.0.1:0/").unwrap();
        assert_eq!(ipv4.host(), Some("127.0.0.1"));
        assert_eq!(ipv4.domain(), None);
        assert_eq!(ipv4.port(), Some(0));

        let ipv6 = Url::parse("http://[2001:0db8::1]:8080/path").unwrap();
        assert_eq!(ipv6.host(), Some("2001:db8::1"));
        assert_eq!(ipv6.domain(), None);
        assert_eq!(ipv6.port(), Some(8080));
        assert_eq!(ipv6.to_string(), "http://[2001:db8::1]:8080/path");
    }

    #[test]
    fn preserves_empty_query_and_fragment() {
        let url = Url::parse("http://example.com?#").unwrap();

        assert_eq!(url.path(), "/");
        assert_eq!(url.query(), Some(""));
        assert_eq!(url.fragment(), Some(""));
        assert_eq!(url.to_string(), "http://example.com/?#");
    }

    #[test]
    fn accepts_valid_percent_encoded_components() {
        let url = Url::parse("https://user%40name@example.com/a%20path?q=a%2Fb#c%23d").unwrap();

        assert_eq!(url.userinfo(), Some("user%40name"));
        assert_eq!(url.path(), "/a%20path");
        assert_eq!(url.query(), Some("q=a%2Fb"));
        assert_eq!(url.fragment(), Some("c%23d"));
    }

    #[test]
    fn percent_encodes_non_url_characters() {
        let url = Url::parse("https://user@example.com/a path?q=\"hello world\"#café").unwrap();

        assert_eq!(url.path(), "/a%20path");
        assert_eq!(url.query(), Some("q=%22hello%20world%22"));
        assert_eq!(url.fragment(), Some("cafe%CC%81"));
    }

    #[test]
    fn rejects_invalid_urls() {
        let invalid_urls = [
            "http://",
            "://example.com",
            "1http://example.com",
            "ht*tp://example.com",
            "http:/example.com",
            "http://?query",
            "http://example.com:abc/",
            "http://example.com:999999/",
            "http://example.com:65536/",
            "http://example.com:-1/",
            "http://example.com:1a2b3c/",
            "http://example.com:/",
            "http://example.com:/path",
            "http://example.com:/path/",
            "http://example.com:/path?query",
            "http://example.com:/path#fragment",
            "http://example.com:/path?query#fragment",
            "http://example.com/%",
            "http://example.com/%2",
            "http://example.com/%zz",
            "http://example.com/\\windows",
            "http://[::1",
            "http://[::1]extra/",
            "http://::1/",
            "http://[not-ipv6]/",
        ];
        for input in invalid_urls {
            assert!(Url::from_str(input).is_err(), "URL should fail: {input}");
        }
    }

    #[test]
    fn displays_normalized_urls() {
        let urls = [
            ("http://example.com", "http://example.com/"),
            ("http://example.com/", "http://example.com/"),
            ("http://example.com/path", "http://example.com/path"),
            (
                "http://example.com/path?query",
                "http://example.com/path?query",
            ),
            (
                "http://example.com/path#fragment",
                "http://example.com/path#fragment",
            ),
            (
                "http://example.com/path?query#fragment",
                "http://example.com/path?query#fragment",
            ),
            (
                "http://user:pass@example.com",
                "http://user:pass@example.com/",
            ),
            (
                "http://user:pass@example.com/path",
                "http://user:pass@example.com/path",
            ),
            (
                "http://user:pass@example.com/path?query",
                "http://user:pass@example.com/path?query",
            ),
            (
                "http://user:pass@example.com/path#fragment",
                "http://user:pass@example.com/path#fragment",
            ),
            (
                "http://user:pass@example.com/path?query#fragment",
                "http://user:pass@example.com/path?query#fragment",
            ),
            ("http://example.com:8080", "http://example.com:8080/"),
            (
                "http://example.com:8080/path",
                "http://example.com:8080/path",
            ),
            (
                "http://example.com:8080/path?query",
                "http://example.com:8080/path?query",
            ),
            (
                "http://example.com:8080/path#fragment",
                "http://example.com:8080/path#fragment",
            ),
            (
                "http://example.com:8080/path?query#fragment",
                "http://example.com:8080/path?query#fragment",
            ),
            ("ws://example.com/", "ws://example.com/"),
            ("wss://example.com/", "wss://example.com/"),
            ("ws://example.com:8080/", "ws://example.com:8080/"),
            ("wss://example.com:8080/", "wss://example.com:8080/"),
            ("ws://example.com/path", "ws://example.com/path"),
            ("wss://example.com/path", "wss://example.com/path"),
            ("ws://example.com/path?query", "ws://example.com/path?query"),
            (
                "wss://example.com/path?query",
                "wss://example.com/path?query",
            ),
            (
                "ws://example.com/path#fragment",
                "ws://example.com/path#fragment",
            ),
            (
                "wss://example.com/path#fragment",
                "wss://example.com/path#fragment",
            ),
        ];
        for (input, expected) in urls {
            let url = Url::from_str(input).unwrap();
            assert_eq!(url.to_string(), expected);
        }
    }
}
