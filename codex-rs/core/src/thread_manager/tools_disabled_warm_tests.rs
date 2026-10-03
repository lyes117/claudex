use super::*;
use crate::config::test_config;
use codex_utils_absolute_path::test_support::PathBufExt;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn warm_resume_keeps_empty_ceiling_and_refuses_to_reuse_broader_runtime() {
    for source_enabled in [false, true] {
        let home = tempfile::tempdir().expect("home");
        let mut config = test_config().await;
        config.codex_home = home.path().to_path_buf().abs();
        config.cwd = config.codex_home.clone();
        config.tools_enabled = source_enabled;
        let manager = ThreadManager::with_models_provider_and_home_for_tests(
            CodexAuth::from_api_key("dummy"),
            config.model_provider.clone(),
            config.codex_home.to_path_buf(),
            Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
        );
        let source = manager
            .start_thread(StartThreadOptions {
                history_mode: Some(ThreadHistoryMode::Legacy),
                environments: Some(Vec::new()),
                ..StartThreadOptions::new(config.clone())
            })
            .await
            .expect("source");
        source.thread.ensure_rollout_materialized().await;
        source.thread.flush_rollout().await.expect("flush");
        let metadata = codex_rollout::read_session_meta_line(
            source
                .thread
                .rollout_path()
                .as_deref()
                .expect("rollout path"),
        )
        .await
        .expect("canonical header");
        assert_eq!(
            metadata.meta.tool_policy_snapshot,
            Some(serde_json::json!({
                "version": 1,
                "allowed_tools": if source_enabled { serde_json::Value::Null } else { serde_json::json!([]) },
                "require_managed_sandbox": false,
                "require_unified_exec": false,
                "expose_additional_permissions": source_enabled,
            }))
        );
        config.tools_enabled = !source_enabled;
        let mut requested = StartThreadOptions::new(config);
        requested.initial_history = InitialHistory::Resumed(ResumedHistory {
            conversation_id: source.thread_id,
            history: Arc::new(Vec::new()),
            rollout_path: source.thread.rollout_path(),
        });
        let resumed = manager.start_thread(requested).await;
        if source_enabled {
            let error = resumed.err().expect("broader runtime must be rejected");
            assert!(error.to_string().contains(LIVE_THREAD_TOOL_POLICY_MISMATCH));
            assert_eq!(
                source.thread.session.tool_policy.as_ref(),
                &ToolPolicy::default()
            );
        } else {
            let resumed = resumed.expect("reuse restrictive runtime");
            assert_eq!(resumed.thread_id, source.thread_id);
            assert!(Arc::ptr_eq(&resumed.thread, &source.thread));
            assert_eq!(
                source.thread.session.tool_policy.allowed_tools,
                Some(Vec::new())
            );
            assert!(
                !source
                    .thread
                    .session
                    .tool_policy
                    .expose_additional_permissions
            );
        }
        source.thread.shutdown_and_wait().await.expect("shutdown");
    }
}
