//! An explicit no-tools request is a ceiling, including under managed overrides.

use codex_config::ConfigLayerSource;
use codex_config::ConfigLayerStack;
use codex_config::config_toml::ConfigToml;

pub(super) fn enabled(config: &ConfigToml, layers: &ConfigLayerStack) -> bool {
    config
        .tools
        .as_ref()
        .and_then(|tools| tools.enabled)
        .unwrap_or(true)
        && !layers.layers_low_to_high().any(|layer| {
            matches!(layer.name, ConfigLayerSource::SessionFlags)
                && layer
                    .config
                    .get("tools")
                    .and_then(|tools| tools.get("enabled"))
                    .and_then(toml::Value::as_bool)
                    == Some(false)
        })
}

#[cfg(test)]
#[path = "tools_ceiling_tests.rs"]
mod tests;
