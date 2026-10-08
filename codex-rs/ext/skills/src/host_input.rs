//! Runtime inputs, including native-authority-derived legacy selection, before discovery.

use super::*;

#[derive(Debug, Clone)]
pub struct HostSkillsLoadInput {
    pub(super) cwd: AbsolutePathBuf,
    pub(super) effective_skill_roots: Vec<PluginSkillRoot>,
    pub(super) config_layer_stack: ConfigLayerStack,
    pub(super) plugin_skill_snapshots: Option<SkillRootSnapshots<PluginSkillRoot>>,
    pub(super) legacy_plugin_selection: Option<codex_config::claude::LegacyPluginSelection>,
    #[cfg(test)]
    pub(super) home_dir_override: Option<AbsolutePathBuf>,
}

impl HostSkillsLoadInput {
    pub fn new(
        cwd: AbsolutePathBuf,
        effective_skill_roots: Vec<PluginSkillRoot>,
        config_layer_stack: ConfigLayerStack,
    ) -> Self {
        Self {
            cwd,
            effective_skill_roots,
            config_layer_stack,
            plugin_skill_snapshots: None,
            legacy_plugin_selection: None,
            #[cfg(test)]
            home_dir_override: None,
        }
    }

    /// Attaches plugin skill snapshots parsed during plugin loading, when available.
    pub fn with_plugin_skill_snapshots(
        mut self,
        plugin_skill_snapshots: Option<SkillRootSnapshots<PluginSkillRoot>>,
    ) -> Self {
        self.plugin_skill_snapshots = plugin_skill_snapshots;
        self
    }

    /// Selection comes from the native loader after thread exclusions, before roots and caches.
    pub fn with_legacy_plugin_selection(
        mut self,
        selection: codex_config::claude::LegacyPluginSelection,
    ) -> Self {
        self.legacy_plugin_selection = Some(selection);
        self
    }
}
