//! Explicit Claude command expansion stays on the server that owns the active thread.
use super::catalog_processor::CatalogRequestProcessor;
use super::internal_error;
use super::invalid_request;
use codex_app_server_protocol::ClaudeCommandExpandParams;
use codex_app_server_protocol::ClaudeCommandExpandResponse;
use codex_app_server_protocol::ClientResponsePayload;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_protocol::ThreadId;
use codex_utils_absolute_path::AbsolutePathBuf;

impl CatalogRequestProcessor {
    pub(crate) async fn claude_command_expand(
        &self,
        params: ClaudeCommandExpandParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        super::claude_command_deadline::resolve(Box::pin(self.claude_command_expand_inner(params)))
            .await
    }

    async fn claude_command_expand_inner(
        &self,
        params: ClaudeCommandExpandParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let reject =
            || invalid_request("Claude command is unavailable for this thread".to_string());
        if params.name.len() > 129 || params.arguments.len() > 8 * 1024 {
            return Err(reject());
        }
        let id = ThreadId::from_string(&params.thread_id).map_err(|_| reject())?;
        let thread = self
            .thread_manager
            .get_thread(id)
            .await
            .map_err(|_| reject())?;
        let current = thread.config_snapshot().await;
        let requested_cwd =
            AbsolutePathBuf::from_absolute_path(params.cwd).map_err(|_| reject())?;
        if &requested_cwd != current.cwd() {
            return Err(reject());
        }
        let thread_config = thread.config().await;
        let config = self
            .config_manager
            .load_latest_config_with_session_layers(
                &thread_config.config_layer_stack,
                current.cwd(),
            )
            .await
            .map_err(|_| {
                internal_error("Could not resolve Claude command configuration".to_string())
            })?;
        if codex_config::claude::permission_block(
            &config.config_layer_stack,
            "Skill",
            &serde_json::json!({"name": params.name}),
        )
        .map_err(|_| reject())?
        .is_some()
        {
            return Err(reject());
        }
        let manager = self.thread_manager.plugins_manager();
        let plugins_input = config.plugins_config_input();
        let plugins = manager
            .plugins_for_config(&plugins_input)
            .await
            .without_plugins(&current.disabled_plugin_ids);
        let selection = config.claude_plugin_selection(&plugins);
        let plugin_snapshots = manager.plugin_skill_snapshots_for_config(&plugins_input);
        let input = codex_skills_extension::HostSkillsLoadInput::new(
            config.cwd.clone(),
            plugins.effective_plugin_skill_roots(),
            config.config_layer_stack,
        )
        .with_legacy_plugin_selection(selection)
        .with_plugin_skill_snapshots(plugin_snapshots);
        let environment_manager = self.thread_manager.environment_manager();
        let fs = match current.environment_selections().first() {
            Some(selection) => Some(
                environment_manager
                    .get_environment(&selection.environment_id)
                    .ok_or_else(reject)?
                    .get_filesystem(),
            ),
            None => Some(
                environment_manager
                    .try_local_environment()
                    .ok_or_else(reject)?
                    .get_filesystem(),
            ),
        };
        let service = self.thread_manager.skills_service();
        let request = service.for_request();
        let snapshot = request
            .snapshot_for_cwd(&input, /*force_reload*/ true, fs)
            .await;
        let outcome = snapshot.outcome();
        let skill = outcome
            .skills
            .iter()
            .find(|skill| skill.name == params.name && skill.path_to_skills_md == params.path)
            .ok_or_else(reject)?;
        if !outcome.is_skill_enabled(skill)
            || skill
                .policy
                .as_ref()
                .and_then(|policy| policy.claude_command.as_ref())
                .is_some_and(|command| !command.user_invocable)
        {
            return Err(reject());
        }
        let contents = snapshot
            .read_claude_command_text(skill)
            .await
            .map_err(|_| reject())?;
        let (metadata, body) = codex_config::claude::markdown(&contents).map_err(|_| reject())?;
        let tokens = shlex::split(&params.arguments).ok_or_else(reject)?;
        let text = codex_config::claude::expand_command_bounded(
            &metadata,
            &body,
            &params.arguments,
            &tokens,
            skill.path_to_skills_md.as_path(),
        )
        .map_err(|_| reject())?;
        if text.is_empty() {
            return Err(reject());
        }
        Ok(Some(ClaudeCommandExpandResponse { text }.into()))
    }
}
