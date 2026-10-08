//! Runtime selection of legacy contributions after the native plugin loader resolves authority.

use super::*;

pub const NATIVE_MEMORY_PLUGIN_ID: &str = "claude-mem@claudex-memory";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LegacyPluginSelection {
    #[default]
    KeepAll,
    NativeMemory,
}

impl LegacyPluginSelection {
    /// IDs must come from active loaded plugins after config and thread exclusions.
    pub fn from_active_native_plugins<'a>(
        stack: &crate::ConfigLayerStack,
        active_plugin_ids: impl Iterator<Item = &'a str>,
    ) -> io::Result<Self> {
        let user_home = home().and_then(|path| path.parent().map(Path::to_path_buf));
        Self::for_user_home(stack, active_plugin_ids, user_home.as_deref())
    }

    fn for_user_home<'a>(
        stack: &crate::ConfigLayerStack,
        mut active_plugin_ids: impl Iterator<Item = &'a str>,
        user_home: Option<&Path>,
    ) -> io::Result<Self> {
        if !user_config_enabled(stack.layers_low_to_high())
            || !active_plugin_ids.any(|id| id == NATIVE_MEMORY_PLUGIN_ID)
        {
            return Ok(Self::KeepAll);
        }
        match user_home {
            Some(home) if native_memory::active_for_user(home)? => Ok(Self::NativeMemory),
            _ => Ok(Self::KeepAll),
        }
    }

    pub fn retains(self, plugin_id: &str) -> bool {
        self != Self::NativeMemory || plugin_id.split('@').next() != Some("claude-mem")
    }
}

/// Kept separately from explicit configuration so selection cannot remove a name collision.
#[derive(Debug, Clone, PartialEq)]
pub struct ClaudePluginMcpConfig {
    pub plugin_id: String,
    pub config: TomlValue,
}

pub fn plugin_mcp_contributions(
    directory: &Path,
    include_user: bool,
    warnings: &mut Vec<String>,
) -> io::Result<Vec<ClaudePluginMcpConfig>> {
    plugins_for_scope(directory, include_user)?
        .into_iter()
        .map(|(plugin_id, root)| {
            let servers = mcp_config(&root.join(".mcp.json"), Some(&root), warnings)?;
            Ok(ClaudePluginMcpConfig {
                plugin_id,
                config: TomlValue::try_from(serde_json::json!({"mcp_servers": servers}))
                    .map_err(io::Error::other)?,
            })
        })
        .collect()
}

pub(crate) fn layer_config_with_plugins(
    layer: &crate::ConfigLayerEntry,
    selection: LegacyPluginSelection,
) -> TomlValue {
    let mut result = TomlValue::Table(toml::map::Map::new());
    for contribution in &layer.claude_plugin_mcp_configs {
        if selection.retains(&contribution.plugin_id) {
            crate::merge_toml_values(&mut result, &contribution.config);
        }
    }
    crate::merge_toml_values(&mut result, &layer.config);
    result
}

pub fn selected_mcp_config(
    stack: &crate::ConfigLayerStack,
    selection: LegacyPluginSelection,
) -> TomlValue {
    let mut result = TomlValue::Table(toml::map::Map::new());
    for layer in stack.layers_low_to_high() {
        crate::merge_toml_values(&mut result, &layer_config_with_plugins(layer, selection));
    }
    result
        .get("mcp_servers")
        .cloned()
        .unwrap_or_else(|| TomlValue::Table(toml::map::Map::new()))
}

#[cfg(test)]
#[path = "claude_plugin_selection_tests.rs"]
mod tests;
