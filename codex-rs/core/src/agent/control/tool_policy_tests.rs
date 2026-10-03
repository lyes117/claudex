use super::*;
use crate::StartThreadOptions;
use crate::ThreadManager;
use crate::config::test_config;
use crate::thread_manager::build_models_manager;
use crate::thread_manager::passthrough_image_store;
use crate::thread_manager::thread_store_from_config;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadStartInput;
use codex_extension_api::ToolPolicy;
use codex_features::Feature;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_utils_absolute_path::test_support::PathBufExt;
use pretty_assertions::assert_eq;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::sync::Notify;
use tokio::time::timeout;

#[derive(Default)]
struct ReloadGate {
    armed: AtomicBool,
    entered: Notify,
    release: Notify,
}

impl ThreadLifecycleContributor<Config> for ReloadGate {
    fn on_thread_start<'a>(
        &'a self,
        input: ThreadStartInput<'a, Config>,
    ) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if matches!(input.session_source, SessionSource::SubAgent(_))
                && self.armed.swap(/*val*/ false, Ordering::AcqRel)
            {
                self.entered.notify_one();
                self.release.notified().await;
            }
        })
    }
}

#[tokio::test]
async fn compatible_concurrent_v2_reload_keeps_the_published_runtime() {
    check_concurrent_reload(/*restrict_parent*/ false).await;
}

#[tokio::test]
async fn incompatible_concurrent_v2_reload_is_not_accepted_by_fallback() {
    check_concurrent_reload(/*restrict_parent*/ true).await;
}

async fn check_concurrent_reload(restrict_parent: bool) {
    let home = tempfile::tempdir().expect("temp home");
    let mut config = test_config().await;
    config.codex_home = home.path().join("codex-home").abs();
    config.cwd = home.path().to_path_buf().abs();
    config.sqlite = codex_state::SqliteConfig::new_for_testing(config.codex_home.clone());
    config.multi_agent_v2.max_concurrent_threads_per_session = 8;
    config
        .features
        .enable(Feature::MultiAgentV2)
        .expect("enable v2");
    std::fs::create_dir_all(&config.codex_home).expect("create home");
    let gate = Arc::new(ReloadGate::default());
    let mut extensions = ExtensionRegistryBuilder::new();
    extensions.thread_lifecycle_contributor(gate.clone());
    let auth = AuthManager::from_auth_for_testing(CodexAuth::from_api_key("dummy"));
    let manager = ThreadManager::new(
        &config,
        Arc::clone(&auth),
        build_models_manager(&config, auth),
        crate::CodexAppsToolsCache::default(),
        SessionSource::Exec,
        Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
        Arc::new(extensions.build()),
        Arc::new(crate::test_support::EmptyUserInstructionsProvider),
        /*analytics_events_client*/ None,
        passthrough_image_store(),
        thread_store_from_config(&config, /*state_db*/ None),
        /*agent_graph_store*/ None,
        "11111111-1111-4111-8111-111111111111".to_owned(),
        /*attestation_provider*/ None,
        /*external_time_provider*/ None,
    );
    let control = manager.agent_control();
    let state = control.runtime.upgrade().expect("live manager");
    let parent_id = manager.reserve_thread_id();
    let mut reservation = control
        .runtime
        .registry
        .reserve_spawn_slot(/*max_threads*/ None)
        .expect("reserve child");
    let (source, mut metadata) = control
        .prepare_thread_spawn(
            &mut reservation,
            &config,
            parent_id,
            /*depth*/ 1,
            /*agent_path*/ None,
            /*agent_role*/ None,
            /*preferred_agent_nickname*/ None,
        )
        .expect("prepare v2 child");
    // A cold parent has no durable ceiling in this stage. Persist a broad child
    // before admitting the parent so the incompatible case has real history.
    let original = state
        .spawn_new_thread_with_source(
            config.clone(),
            control.clone(),
            source.clone(),
            Some(ThreadHistoryMode::Legacy),
            Some(parent_id),
            /*forked_from_thread_id*/ None,
            Some(ThreadSource::Subagent),
            /*metrics_service_name*/ None,
            /*inherited_environments*/ None,
            /*inherited_exec_policy*/ None,
            Some(Vec::new()),
            /*inherited_tool_policy*/ None,
        )
        .await
        .expect("create original child");
    metadata.agent_id = Some(original.thread_id);
    reservation.commit(metadata);
    assert_eq!(
        original.thread.session.tool_policy.as_ref(),
        &ToolPolicy::default()
    );
    assert_eq!(
        original.thread.multi_agent_version(),
        Some(MultiAgentVersion::V2)
    );
    original.thread.ensure_rollout_materialized().await;
    original
        .thread
        .flush_rollout()
        .await
        .expect("persist child metadata");
    original
        .thread
        .shutdown_and_wait()
        .await
        .expect("close the original writer");
    let removed = manager
        .remove_thread(&original.thread_id)
        .await
        .expect("unpublish original");
    assert!(Arc::ptr_eq(&removed, &original.thread));
    let mut options = StartThreadOptions::new(config.clone());
    options.reserved_thread_id = Some(parent_id);
    options.environments = Some(Vec::new());
    if restrict_parent {
        options.thread_extension_init.insert(ToolPolicy {
            allowed_tools: Some(Vec::new()),
            ..Default::default()
        });
    }
    let parent = manager
        .start_thread(options)
        .await
        .expect("start resident parent");

    // This is deliberately an independently created runtime, not a second
    // ordinary reload: its separate home/writer lets us inject the publication
    // collision after startup. The assertions exercise the sender fallback,
    // not cross-manager ownership or concurrent writer acquisition.
    let candidate_home = tempfile::tempdir().expect("candidate home");
    let mut candidate_config = config.clone();
    candidate_config.codex_home = candidate_home.path().join("codex-home").abs();
    candidate_config.cwd = candidate_home.path().to_path_buf().abs();
    candidate_config.sqlite =
        codex_state::SqliteConfig::new_for_testing(candidate_config.codex_home.clone());
    std::fs::create_dir_all(&candidate_config.codex_home).expect("create candidate home");
    let candidate_manager = ThreadManager::with_models_provider_and_home_for_tests(
        CodexAuth::from_api_key("dummy"),
        candidate_config.model_provider.clone(),
        candidate_config.codex_home.to_path_buf(),
        Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
    );
    let mut options = StartThreadOptions::new(candidate_config);
    options.reserved_thread_id = Some(original.thread_id);
    options.session_source = Some(source);
    options.history_mode = Some(ThreadHistoryMode::Legacy);
    options.environments = Some(Vec::new());
    let candidate = candidate_manager
        .start_thread(options)
        .await
        .expect("create independent candidate");
    assert_eq!(candidate.thread_id, original.thread_id);
    assert_eq!(
        candidate.thread.session.tool_policy.as_ref(),
        &ToolPolicy::default()
    );
    assert!(candidate.thread.is_running());
    gate.armed.store(/*val*/ true, Ordering::Release);
    let child_id = candidate.thread_id;
    let mut reload = tokio::spawn(async move {
        control
            .ensure_v2_agent_loaded(config, child_id, /*parent*/ None)
            .await
    });
    let entered = timeout(Duration::from_secs(/*secs*/ 10), async {
        tokio::select! {
            result = &mut reload => panic!("reload finished before the startup gate: {result:?}"),
            _ = gate.entered.notified() => {}
        }
    })
    .await;
    if entered.is_err() {
        reload.abort();
    }
    entered.expect("reload is still pending before the startup gate");
    // The extension callback runs before finalize_thread_spawn. Publish under
    // the actual registry lock, then let finalization encounter the collision.
    assert!(
        state
            .threads
            .write()
            .await
            .insert(child_id, Arc::clone(&candidate.thread))
            .is_none()
    );
    gate.release.notify_one();
    let result = timeout(Duration::from_secs(/*secs*/ 10), reload)
        .await
        .expect("reload completes")
        .expect("reload task does not panic");
    if restrict_parent {
        assert!(matches!(
            result.expect_err("refuse broad candidate").details(),
            CodexErrorDetails::InvalidRequest(_)
        ));
    } else {
        result.expect("compatible publication wins the race");
    }
    let published = manager
        .get_thread(child_id)
        .await
        .expect("published candidate remains");
    assert!(Arc::ptr_eq(&published, &candidate.thread));
    assert_eq!(
        published
            .session
            .tool_policy
            .is_subset_of(&parent.thread.session.tool_policy),
        !restrict_parent,
    );
    candidate
        .thread
        .shutdown_and_wait()
        .await
        .expect("stop candidate");
    parent
        .thread
        .shutdown_and_wait()
        .await
        .expect("stop parent");
}
