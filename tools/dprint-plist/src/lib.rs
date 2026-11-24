/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Adds repository property-list style rules to the dprint markup formatter.

#![cfg(any(test, target_arch = "wasm32"))]

use std::borrow::Cow;
use std::ops::Range;

#[cfg(target_arch = "wasm32")]
use dprint_core::configuration::{
    ConfigKeyMap, GlobalConfiguration, get_nullable_value, get_unknown_property_diagnostics,
};
#[cfg(target_arch = "wasm32")]
use dprint_core::plugins::{
    CheckConfigUpdatesMessage, ConfigChange, FileMatchingInfo, FormatError, FormatResult,
    PluginInfo, PluginResolveConfigurationResult, SyncFormatRequest, SyncHostFormatRequest,
    SyncPluginHandler,
};
use xmlparser::{ElementEnd, Token, Tokenizer};

#[derive(Clone)]
#[cfg_attr(target_arch = "wasm32", derive(serde::Serialize))]
struct Configuration {
    unindented_root: Option<String>,
}

#[cfg(target_arch = "wasm32")]
struct PlistPlugin;

#[cfg(target_arch = "wasm32")]
impl SyncPluginHandler<Configuration> for PlistPlugin {
    fn resolve_config(
        &mut self,
        mut config: ConfigKeyMap,
        _: &GlobalConfiguration,
    ) -> PluginResolveConfigurationResult<Configuration> {
        let mut diagnostics = Vec::new();
        let unindented_root = get_nullable_value(&mut config, "unindentedRoot", &mut diagnostics);
        diagnostics.extend(get_unknown_property_diagnostics(config));
        PluginResolveConfigurationResult {
            config: Configuration { unindented_root },
            diagnostics,
            file_matching: FileMatchingInfo {
                file_extensions: ["plist", "entitlements"].map(str::to_owned).into(),
                file_names: Vec::new(),
                additive: false,
            },
        }
    }

    fn plugin_info(&mut self) -> PluginInfo {
        PluginInfo {
            name: env!("CARGO_PKG_NAME").into(),
            version: env!("CARGO_PKG_VERSION").into(),
            config_key: "plist".into(),
            help_url: concat!(
                env!("CARGO_PKG_REPOSITORY"),
                "/tree/master/tools/dprint-plist"
            )
            .into(),
            config_schema_url: String::new(),
            update_url: None,
            supports_range_formatting: true,
        }
    }

    fn license_text(&mut self) -> String {
        include_str!("../../../LICENSE").into()
    }

    fn check_config_updates(
        &self,
        _: CheckConfigUpdatesMessage,
    ) -> Result<Vec<ConfigChange>, FormatError> {
        Ok(Vec::new())
    }

    fn format(
        &mut self,
        request: SyncFormatRequest<Configuration>,
        mut format_with_host: impl FnMut(SyncHostFormatRequest) -> FormatResult,
    ) -> FormatResult {
        if request.file_bytes.starts_with(b"bplist00") {
            return Ok(None);
        }
        let original = String::from_utf8(request.file_bytes).map_err(FormatError::new)?;
        let markup_path = request.file_path.with_extension("dprint-markup.xml");
        let config = ConfigKeyMap::new();
        let markup_format = |text: &str| -> Result<String, FormatError> {
            let formatted = format_with_host(SyncHostFormatRequest {
                file_path: &markup_path,
                file_bytes: text.as_bytes(),
                range: None,
                override_config: &config,
            })?;
            formatted.map_or_else(
                || Ok(text.to_owned()),
                |bytes| String::from_utf8(bytes).map_err(FormatError::new),
            )
        };
        let formatted = format_plist(&original, request.config, markup_format)?;
        if let Some(range) = request.range
            && !(range.start == 0 && range.end >= original.len())
        {
            return Ok(apply_range(&original, &formatted, range).map(String::into_bytes));
        }
        Ok((formatted != original).then(|| formatted.into_bytes()))
    }
}

#[cfg(target_arch = "wasm32")]
dprint_core::generate_plugin_code!(PlistPlugin, PlistPlugin);

fn format_plist<E>(
    text: &str,
    config: &Configuration,
    mut format_markup: impl FnMut(&str) -> Result<String, E>,
) -> Result<String, E> {
    let mut formatted = if let Some(body) = config
        .unindented_root
        .as_deref()
        .and_then(|root| root_body(text, root))
        .filter(|body| !text[body.clone()].trim().is_empty())
    {
        let inner = format_markup(text[body.clone()].trim_matches(is_xml_whitespace))?;
        format!(
            "{}\n{}\n{}",
            &text[..body.start],
            inner.trim_end_matches(is_xml_whitespace),
            &text[body.end..],
        )
    } else {
        format_markup(text)?
    };

    // Some macOS command line tools reject `<true />`, so always write `<true/>`
    remove_self_closing_spaces(&mut formatted);
    Ok(formatted)
}

/// Aligns the original and formatted XML on non-whitespace bytes. The markup formatter
/// keeps their order and spelling, so each whitespace edit can be applied independently.
fn apply_range(original: &str, formatted: &str, range: Range<usize>) -> Option<String> {
    let end = range.end.min(original.len());
    let start = range.start.min(end);
    let range = if start == end {
        let bytes = original.as_bytes();
        let line_start = bytes[..start]
            .iter()
            .rposition(|&byte| byte == b'\n')
            .map_or(0, |index| index + 1);
        let line_end = bytes[start..]
            .iter()
            .position(|&byte| byte == b'\n')
            .map_or(bytes.len(), |index| start + index);
        line_start..line_end
    } else {
        start..end
    };
    let body = original.trim_start_matches('\u{feff}');
    let bom_len = original.len() - body.len();
    let mut before = body;
    let mut after = formatted.trim_start_matches('\u{feff}');
    let crlf = body
        .find('\n')
        .is_some_and(|index| body.as_bytes()[..index].ends_with(b"\r"));
    let mut result = String::with_capacity(original.len());
    result.push_str(&original[..bom_len]);
    loop {
        let old_content = before.trim_start_matches(is_xml_whitespace);
        let new_content = after.trim_start_matches(is_xml_whitespace);
        let old_gap = &before[..before.len() - old_content.len()];
        let new_gap = &after[..after.len() - new_content.len()];
        let new_gap = if old_gap == new_gap {
            Cow::Borrowed(new_gap)
        } else {
            let lf = if new_gap.contains("\r\n") {
                Cow::Owned(new_gap.replace("\r\n", "\n"))
            } else {
                Cow::Borrowed(new_gap)
            };
            if crlf && lf.contains('\n') {
                Cow::Owned(lf.replace('\n', "\r\n"))
            } else {
                lf
            }
        };
        let offset = original.len() - before.len();
        let common_start = old_gap
            .bytes()
            .zip(new_gap.bytes())
            .take_while(|(old, new)| old == new)
            .count();
        let common_end = old_gap.as_bytes()[common_start..]
            .iter()
            .rev()
            .zip(new_gap.as_bytes()[common_start..].iter().rev())
            .take_while(|(old, new)| old == new)
            .count();
        let edit_start = offset + common_start;
        let edit_end = offset + old_gap.len() - common_end;
        let before_range_end = edit_start < range.end || edit_start == original.len();
        if edit_start >= range.start && edit_end <= range.end && before_range_end {
            result.push_str(&new_gap);
        } else {
            result.push_str(old_gap);
        }

        if old_content.is_empty() || new_content.is_empty() {
            return (old_content == new_content && result != original).then_some(result);
        }
        let run_len = old_content
            .bytes()
            .zip(new_content.bytes())
            .take_while(|&(old, new)| old == new && !is_xml_whitespace(char::from(old)))
            .count();
        if run_len == 0
            || !old_content.is_char_boundary(run_len)
            || !new_content.is_char_boundary(run_len)
        {
            return None;
        }
        result.push_str(&old_content[..run_len]);
        before = &old_content[run_len..];
        after = &new_content[run_len..];
    }
}

fn root_body(text: &str, root: &str) -> Option<Range<usize>> {
    let mut tokens = Tokenizer::from(text);
    loop {
        if let Token::ElementStart { prefix, local, .. } = tokens.next()?.ok()? {
            if !prefix.as_str().is_empty() || local.as_str() != root {
                return None;
            }
            break;
        }
    }
    let start = loop {
        if let Token::ElementEnd { end, span } = tokens.next()?.ok()? {
            break matches!(end, ElementEnd::Open).then_some(span.end())?;
        }
    };
    let mut end = None;
    for token in tokens {
        if let Token::ElementEnd {
            end: ElementEnd::Close(prefix, local),
            span,
        } = token.ok()?
            && prefix.as_str().is_empty()
            && local.as_str() == root
        {
            end = Some(span.start());
        }
    }
    Some(start..end?)
}

fn remove_self_closing_spaces(text: &mut String) {
    let gaps = Tokenizer::from(text.as_str())
        .map_while(Result::ok)
        .filter_map(|token| {
            let Token::ElementEnd {
                end: ElementEnd::Empty,
                span,
            } = token
            else {
                return None;
            };
            let end = span.start();
            let start = text[..end].trim_end_matches(is_xml_whitespace).len();
            (start < end).then_some(start..end)
        })
        .collect::<Vec<_>>();
    for gap in gaps.into_iter().rev() {
        text.replace_range(gap, "");
    }
}

const fn is_xml_whitespace(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\n' | '\r')
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use super::*;

    #[test]
    fn range_formats_only_selected_xml_whitespace() {
        let original = "<plist>\r\n<array>\r\n<true />\r\n<false />\r\n</array>\r\n</plist>\r\n";
        let formatted = "<plist>\n<array>\n    <true/>\n    <false/>\n</array>\n</plist>\n";
        let start = original.find("<true").unwrap();
        let end = original.find("<false").unwrap();
        assert_eq!(
            apply_range(original, formatted, start..end).as_deref(),
            Some("<plist>\r\n<array>\r\n    <true/>\r\n<false />\r\n</array>\r\n</plist>\r\n"),
        );
    }

    #[test]
    fn range_aligns_insertions_between_previously_adjacent_tags() {
        let original = "<array><true /><string>caf\u{e9}</string></array>";
        let formatted = "<array>\n    <true/>\n    <string>caf\u{e9}</string>\n</array>\n";
        let start = original.find("<true").unwrap();
        let end = original.find("<string").unwrap();
        assert_eq!(
            apply_range(original, formatted, start..end).as_deref(),
            Some("<array>\n    <true/><string>caf\u{e9}</string></array>"),
        );
    }

    #[test]
    fn plist_root_value_is_formatted_as_a_standalone_element() -> Result<(), Infallible> {
        let original =
            "<?xml version=\"1.0\"?>\n<plist version=\"1.0\"><dict><true /></dict></plist>\n";
        let config = Configuration {
            unindented_root: Some("plist".to_owned()),
        };
        let formatted = format_plist(original, &config, |body| {
            assert_eq!(body, "<dict><true /></dict>");
            Ok("<dict>\n    <true />\n</dict>\n".to_owned())
        })?;
        assert_eq!(
            formatted,
            "<?xml version=\"1.0\"?>\n<plist version=\"1.0\">\n<dict>\n    <true/>\n</dict>\n</plist>\n"
        );
        Ok(())
    }

    #[test]
    fn self_closing_spaces_are_removed_only_from_tags() {
        let mut xml = r#"<plist><array><string><![CDATA[ />]]></string><!-- /> --><string demo=" />" /></array></plist>"#.to_owned();
        remove_self_closing_spaces(&mut xml);
        assert_eq!(
            xml,
            r#"<plist><array><string><![CDATA[ />]]></string><!-- /> --><string demo=" />"/></array></plist>"#
        );
    }
}
