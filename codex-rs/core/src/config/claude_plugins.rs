//! Derive legacy compatibility from native authority and retain runtime MCP overrides.

use super::*;
use codex_config::claude::LegacyPluginSelection;

impl Config {
    /// Resolve legacy contributions from the native loader's active, exclusion-filtered outcome.
    pub fn claude_plugin_selection(
        &self,
        loaded_plugins: &PluginLoadOutcome,
    ) -> LegacyPluginSelection {
        let active_ids = loaded_plugins
            .plugins()
            .iter()
            .filter(|plugin| plugin.is_active())
            .map(|plugin| plugin.config_name.as_str());
        LegacyPluginSelection::from_active_native_plugins(&self.config_layer_stack, active_ids)
            .unwrap_or_else(|error| {
                tracing::warn!(%error, "Claudex: native memory activation rejected; retaining legacy contributions");
                LegacyPluginSelection::KeepAll
            })
    }

    pub(super) fn mcp_servers_with_legacy_selection(
        &self,
        selection: LegacyPluginSelection,
    ) -> HashMap<String, McpServerConfig> {
        if selection == LegacyPluginSelection::KeepAll {
            return self.mcp_servers.get().clone();
        }
        let baseline = self.layer_mcp_servers(LegacyPluginSelection::KeepAll);
        let mut selected = self.layer_mcp_servers(selection);
        // Config is also refreshed by dependency installation and runtime MCP settings.
        // Preserve those changes relative to the layer projection, including removals.
        for name in baseline.keys() {
            if !self.mcp_servers.get().contains_key(name) {
                selected.remove(name);
            }
        }
        for (name, server) in self.mcp_servers.get() {
            if baseline.get(name) != Some(server) {
                selected.insert(name.clone(), server.clone());
            }
        }
        filter_mcp_servers_by_requirements(
            &mut selected,
            self.config_layer_stack.requirements().mcp_servers.as_ref(),
        );
        selected
    }

    fn layer_mcp_servers(
        &self,
        selection: LegacyPluginSelection,
    ) -> HashMap<String, McpServerConfig> {
        let value = codex_config::claude::selected_mcp_config(&self.config_layer_stack, selection);
        let mut servers = HashMap::new();
        for (name, value) in value.as_table().into_iter().flatten() {
            match value.clone().try_into::<McpServerConfig>() {
                Ok(server) => {
                    servers.insert(name.clone(), server);
                }
                Err(_) => {
                    // An explicit partial override may lose its transport after its legacy
                    // provider is excluded. Such an incomplete definition cannot be connected.
                    tracing::warn!(
                        server = name,
                        "Claudex: incomplete selected MCP definition skipped"
                    );
                }
            }
        }
        filter_mcp_servers_by_requirements(
            &mut servers,
            self.config_layer_stack.requirements().mcp_servers.as_ref(),
        );
        servers
    }
}

#[cfg(test)]
#[path = "claude_plugins_tests.rs"]
mod tests;
