/*
 * Copyright (c) 2021-2023 David Sherret
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A fork of the [dprint TOML plugin](https://github.com/dprint/dprint-plugin-toml) with the
//! repository's `Cargo.toml` conventions.

#![cfg(any(test, target_arch = "wasm32"))]

mod ast;
mod cargo;
mod configuration;
mod format_range;
mod format_text;
mod generation;
mod parser;
mod sorting;
#[cfg(test)]
mod tests;

#[cfg(target_arch = "wasm32")]
mod plugin {
    use dprint_core::configuration::{ConfigKeyMap, GlobalConfiguration};
    use dprint_core::plugins::{
        CheckConfigUpdatesMessage, ConfigChange, FileMatchingInfo, FormatError, FormatResult,
        PluginInfo, PluginResolveConfigurationResult, SyncFormatRequest, SyncHostFormatRequest,
        SyncPluginHandler,
    };

    use crate::configuration::{Configuration, resolve_config};
    use crate::format_range::format_text_range;
    use crate::format_text::format_text;

    struct TomlPlugin;

    impl SyncPluginHandler<Configuration> for TomlPlugin {
        fn resolve_config(
            &mut self,
            config: ConfigKeyMap,
            global_config: &GlobalConfiguration,
        ) -> PluginResolveConfigurationResult<Configuration> {
            let (config, diagnostics) = resolve_config(config, global_config);
            PluginResolveConfigurationResult {
                config,
                diagnostics,
                file_matching: FileMatchingInfo {
                    file_extensions: vec!["toml".into()],
                    file_names: Vec::new(),
                    additive: false,
                },
            }
        }

        fn plugin_info(&mut self) -> PluginInfo {
            PluginInfo {
                name: env!("CARGO_PKG_NAME").into(),
                version: env!("CARGO_PKG_VERSION").into(),
                config_key: "toml".into(),
                help_url: concat!(
                    env!("CARGO_PKG_REPOSITORY"),
                    "/tree/master/tools/dprint-toml"
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
            _: impl FnMut(SyncHostFormatRequest) -> FormatResult,
        ) -> FormatResult {
            let text = String::from_utf8(request.file_bytes)?;
            let formatted = match request.range {
                Some(range) => format_text_range(
                    request.arena_pool,
                    request.file_path,
                    &text,
                    range,
                    request.config,
                ),
                None => format_text(
                    &request.arena_pool.create_arena(),
                    request.file_path,
                    &text,
                    request.config,
                ),
            }
            .map_err(FormatError::new)?;
            Ok(formatted.map(String::into_bytes))
        }
    }

    dprint_core::generate_plugin_code!(TomlPlugin, TomlPlugin);
}
