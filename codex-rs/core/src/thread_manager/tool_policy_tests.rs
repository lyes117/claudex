use super::*;
use crate::config::test_config;
use codex_extension_api::ToolName;
use codex_extension_api::ToolPolicy;
use codex_thread_store::ForkBoundary;
use codex_thread_store::PrepareForkParams;
use codex_utils_absolute_path::test_support::PathBufExt;
use pretty_assertions::assert_eq;
use test_case::test_case;

#[derive(Clone, Copy)]
enum SourceFork {
    CopiedLegacy,
    CopiedPaginated,
    Prepared,
}

#[tokio::test]
#[test_case(SourceFork::CopiedLegacy; "legacy")]
#[test_case(SourceFork::CopiedPaginated; "copied_paginated")]
#[test_case(SourceFork::Prepared; "prepared_paginated")]
async fn fork_keeps_immediate_source_ceiling_instead_of_wider_ancestor(mode: SourceFork) {
    let directory = tempfile::tempdir().expect("temp home");
    let mut config = test_config().await;
    config.codex_home = directory.path().join("codex-home").abs();
    config.cwd = directory.path().to_path_buf().abs();
    config.sqlite = codex_state::SqliteConfig::new_for_testing(config.codex_home.clone());
    std::fs::create_dir_all(&config.codex_home).expect("create home");
    let state_db = crate::init_state_db(&config).await;
    assert!(
        state_db.is_some(),
        "prepared history requires the fixture state database"
    );
    let manager = ThreadManager::with_models_provider_home_and_state_for_tests(
        CodexAuth::from_api_key("dummy"),
        config.model_provider.clone(),
        config.codex_home.to_path_buf(),
        Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
        state_db,
    );
    let ancestor = manager
        .start_thread(StartThreadOptions {
            environments: Some(Vec::new()),
            ..StartThreadOptions::new(config.clone())
        })
        .await
        .expect("start ancestor");
    let source_identity = SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
        parent_thread_id: ancestor.thread_id,
        depth: 1,
        agent_path: None,
        agent_role: None,
        agent_nickname: None,
    });
    let policy = ToolPolicy {
        allowed_tools: Some(vec![ToolName::plain("Read")]),
        expose_additional_permissions: false,
        require_unified_exec: true,
        ..Default::default()
    };
    let mut init = ExtensionDataInit::new();
    init.insert(policy.clone());
    let source = manager
        .start_thread(StartThreadOptions {
            session_source: Some(source_identity.clone()),
            history_mode: Some(match mode {
                SourceFork::CopiedLegacy => ThreadHistoryMode::Legacy,
                SourceFork::CopiedPaginated | SourceFork::Prepared => ThreadHistoryMode::Paginated,
            }),
            environments: Some(Vec::new()),
            thread_extension_init: init,
            ..StartThreadOptions::new(config.clone())
        })
        .await
        .expect("start restrictive source");
    assert_eq!(source.thread.session.tool_policy.as_ref(), &policy);
    let options = StartThreadOptions {
        session_source: Some(source_identity),
        environments: Some(Vec::new()),
        ..StartThreadOptions::new(config)
    };
    let fork = match mode {
        SourceFork::CopiedLegacy | SourceFork::CopiedPaginated => {
            manager
                .fork_thread_from_history(
                    ForkSnapshot::Interrupted,
                    options,
                    InitialHistory::Resumed(ResumedHistory {
                        conversation_id: source.thread_id,
                        history: Arc::new(Vec::new()),
                        rollout_path: None,
                    }),
                )
                .await
        }
        SourceFork::Prepared => {
            manager
                .state
                .thread_store
                .flush_thread(source.thread_id)
                .await
                .expect("flush source");
            let prepared = manager
                .state
                .thread_store
                .prepare_fork(PrepareForkParams {
                    thread_id: source.thread_id,
                    boundary: ForkBoundary::Latest,
                })
                .await
                .expect("prepare fork");
            manager.fork_prepared_thread(options, prepared).await
        }
    }
    .expect("fork restrictive source");
    assert_eq!(fork.thread.session.tool_policy.as_ref(), &policy);
    fork.thread.shutdown_and_wait().await.expect("stop fork");
    source
        .thread
        .shutdown_and_wait()
        .await
        .expect("stop source");
    ancestor
        .thread
        .shutdown_and_wait()
        .await
        .expect("stop ancestor");
}
