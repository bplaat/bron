/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Adds repository property-list style rules to the dprint markup formatter.

#![cfg(any(test, target_arch = "wasm32"))]

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
