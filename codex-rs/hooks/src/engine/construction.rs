//! Apply the runtime legacy selection before discovering hooks, without changing hook policy.

use super::*;
#[cfg(test)]
use codex_config::ConfigLayerStack;
#[cfg(test)]
use codex_plugin::PluginHookSource;

impl ClaudeHooksEngine {
    #[cfg(test)]
    pub(crate) fn new(
        enabled: bool,
        bypass_hook_trust: bool,
        config_layer_stack: Option<&ConfigLayerStack>,
        plugin_hook_sources: Vec<PluginHookSource>,
        plugin_hook_load_warnings: Vec<String>,
        command_runtime: CommandHookRuntime,
        mcp_executor: Arc<dyn HookMcpExecutor>,
    ) -> Self {
        Self::from_hooks_config(
            &crate::HooksConfig {
                feature_enabled: enabled,
                bypass_hook_trust,
                config_layer_stack: config_layer_stack.cloned(),
                plugin_hook_sources,
                plugin_hook_load_warnings,
                ..Default::default()
            },
            command_runtime,
            mcp_executor,
        )
    }

    pub(crate) fn from_hooks_config(
        config: &crate::HooksConfig,
        command_runtime: CommandHookRuntime,
        mcp_executor: Arc<dyn HookMcpExecutor>,
    ) -> Self {
        if !config.feature_enabled && config.plugin_hook_sources.is_empty() {
            return Self {
                handlers: Vec::new(),
                warnings: Vec::new(),
                required_load_errors: Vec::new(),
                command_runtime,
                mcp_executor,
            };
        }
        let _ = schema_loader::generated_hook_schemas();
        let mut discovered = discovery::discover_handlers_with_legacy_plugin_selection(
            config.config_layer_stack.as_ref(),
            config.plugin_hook_sources.clone(),
            config.plugin_hook_load_warnings.clone(),
            config.bypass_hook_trust,
            config.legacy_plugin_selection,
        );
        if !config.feature_enabled {
            discovered.handlers.retain(|handler| handler.builtin);
            discovered.warnings.clear();
            discovered.required_load_errors.clear();
        }
        Self {
            handlers: discovered.handlers,
            warnings: discovered.warnings,
            required_load_errors: discovered.required_load_errors,
            command_runtime,
            mcp_executor,
        }
    }
}
