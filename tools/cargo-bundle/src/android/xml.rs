/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// A minimal XML element tree for Android manifests: elements and attributes only,
// comments and the declaration are skipped.

use std::fmt::Write;

use crate::support::xml_escape;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Element {
    pub name: String,
    pub attributes: Vec<(String, String)>,
    pub children: Vec<Element>,
}

impl Element {
    pub(crate) fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            attributes: Vec::new(),
            children: Vec::new(),
        }
    }

    pub(crate) fn attr(mut self, name: &str, value: impl Into<String>) -> Self {
        self.set(name, value);
        self
    }

    pub(crate) fn child(mut self, child: Element) -> Self {
        self.children.push(child);
        self
    }

    pub(crate) fn get(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    pub(crate) fn set(&mut self, name: &str, value: impl Into<String>) {
        let value = value.into();
        match self.attributes.iter_mut().find(|(key, _)| key == name) {
            Some((_, existing)) => *existing = value,
            None => self.attributes.push((name.to_string(), value)),
        }
    }

    // MARK: Parse
    pub(crate) fn parse(source: &str) -> Result<Self, String> {
        let mut parser = Parser {
            source,
            position: 0,
        };
        parser.skip_misc()?;
        let root = parser.element()?;
        parser.skip_misc()?;
        if parser.position != source.len() {
            return Err(parser.error("unexpected content after the root element"));
        }
        Ok(root)
    }

    // MARK: Write
    pub(crate) fn to_document(&self) -> String {
        let mut output = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
        self.write(&mut output, 0);
        output
    }

    fn write(&self, output: &mut String, depth: usize) {
        let indent = "    ".repeat(depth);
        _ = write!(output, "{indent}<{}", self.name);
        for (name, value) in &self.attributes {
            _ = write!(output, " {name}=\"{}\"", xml_escape(value));
        }
        if self.children.is_empty() {
            output.push_str(" />\n");
            return;
        }
        output.push_str(">\n");
        for child in &self.children {
            child.write(output, depth + 1);
        }
        _ = writeln!(output, "{indent}</{}>", self.name);
    }
}

fn unescape(value: &str) -> Result<String, String> {
    let mut output = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find('&') {
        output.push_str(&rest[..start]);
        let end = rest[start..]
            .find(';')
            .ok_or_else(|| format!("unterminated entity in {value:?}"))?;
        let entity = &rest[start + 1..start + end];
        let character = match entity {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            _ => entity
                .strip_prefix("#x")
                .map(|hex| u32::from_str_radix(hex, 16))
                .or_else(|| entity.strip_prefix('#').map(str::parse))
                .and_then(|code| code.ok())
                .and_then(char::from_u32)
                .ok_or_else(|| format!("unknown entity &{entity};"))?,
        };
        output.push(character);
        rest = &rest[start + end + 1..];
    }
    output.push_str(rest);
    Ok(output)
}

struct Parser<'a> {
    source: &'a str,
    position: usize,
}

impl Parser<'_> {
    fn rest(&self) -> &str {
        &self.source[self.position..]
    }

    fn error(&self, message: &str) -> String {
        let line = self.source[..self.position].matches('\n').count() + 1;
        format!("{message} on line {line}")
    }

    fn skip_whitespace(&mut self) {
        let rest = self.rest();
        self.position += rest.len() - rest.trim_start().len();
    }

    fn skip_until(&mut self, end: &str) -> Result<(), String> {
        let offset = self
            .rest()
            .find(end)
            .ok_or_else(|| self.error(&format!("missing {end}")))?;
        self.position += offset + end.len();
        Ok(())
    }

    // Skip whitespace, comments, and processing instructions like the XML declaration.
    fn skip_misc(&mut self) -> Result<(), String> {
        loop {
            self.skip_whitespace();
            if self.rest().starts_with("<!--") {
                self.skip_until("-->")?;
            } else if self.rest().starts_with("<?") {
                self.skip_until("?>")?;
            } else {
                return Ok(());
            }
        }
    }

    fn name(&mut self) -> Result<String, String> {
        let length = self
            .rest()
            .find(|c: char| !(c.is_alphanumeric() || matches!(c, ':' | '_' | '-' | '.')))
            .unwrap_or(self.rest().len());
        if length == 0 {
            return Err(self.error("expected a name"));
        }
        let name = self.rest()[..length].to_string();
        self.position += length;
        Ok(name)
    }

    fn expect(&mut self, token: &str) -> Result<(), String> {
        if !self.rest().starts_with(token) {
            return Err(self.error(&format!("expected {token}")));
        }
        self.position += token.len();
        Ok(())
    }

    fn element(&mut self) -> Result<Element, String> {
        self.expect("<")?;
        let mut element = Element::new(&self.name()?);
        loop {
            self.skip_whitespace();
            if self.rest().starts_with("/>") {
                self.position += 2;
                return Ok(element);
            }
            if self.rest().starts_with('>') {
                self.position += 1;
                break;
            }
            let name = self.name()?;
            self.skip_whitespace();
            self.expect("=")?;
            self.skip_whitespace();
            let quote = self
                .rest()
                .chars()
                .next()
                .filter(|quote| matches!(quote, '"' | '\''))
                .ok_or_else(|| self.error("expected a quoted attribute value"))?;
            self.position += 1;
            let length = self
                .rest()
                .find(quote)
                .ok_or_else(|| self.error("unterminated attribute value"))?;
            let value = unescape(&self.rest()[..length])?;
            self.position += length + 1;
            if element.get(&name).is_some() {
                return Err(self.error(&format!("duplicate attribute {name}")));
            }
            element.attributes.push((name, value));
        }
        loop {
            self.skip_misc()?;
            if self.rest().starts_with("</") {
                self.position += 2;
                if self.name()? != element.name {
                    return Err(self.error(&format!("expected </{}>", element.name)));
                }
                self.skip_whitespace();
                self.expect(">")?;
                return Ok(element);
            }
            if !self.rest().starts_with('<') || self.rest().starts_with("<!") {
                return Err(self.error("text, CDATA, and DTD content is not supported"));
            }
            element.children.push(self.element()?);
        }
    }
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_elements_attributes_and_entities() {
        let root = Element::parse(
            r#"<?xml version="1.0" encoding="utf-8"?>
<!-- comment -->
<manifest xmlns:android='http://schemas.android.com/apk/res/android' label="a &amp; b &#x41;&#66;">
    <application android:name="App"><!-- inner --></application>
    <uses-feature />
</manifest>
"#,
        )
        .unwrap();
        assert_eq!(root.name, "manifest");
        assert_eq!(root.get("label"), Some("a & b AB"));
        assert_eq!(root.children.len(), 2);
        assert_eq!(root.children[0].get("android:name"), Some("App"));
        assert_eq!(root.children[1].name, "uses-feature");
    }

    #[test]
    fn rejects_malformed_documents() {
        for source in [
            "<a>",
            "<a></b>",
            "<a b=c />",
            "<a b=\"1\" b=\"2\" />",
            "<a>text</a>",
            "<a /><b />",
            "<a b=\"&unknown;\" />",
        ] {
            assert!(Element::parse(source).is_err(), "{source}");
        }
    }

    #[test]
    fn writes_escaped_indented_documents_that_parse_back() {
        let element = Element::new("manifest")
            .attr("package", "nl.bplaat.app")
            .child(Element::new("application").attr("android:label", "\"Tom & Jerry\" <3"));
        let document = element.to_document();
        assert_eq!(
            document,
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<manifest package=\"nl.bplaat.app\">\n    <application android:label=\"&quot;Tom &amp; Jerry&quot; &lt;3\" />\n</manifest>\n"
        );
        assert_eq!(Element::parse(&document).unwrap(), element);
    }
}
