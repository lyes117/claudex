//! Exact native runtime ownership across replacement of a manager's thread ID entry.
use super::spawn::SpawnInitialInput;
use super::spawn_guard::PendingSpawn;
use super::workflow::shutdown_owned_child;
use super::*;
use crate::StartThreadOptions;
use crate::ThreadManager;
use crate::agent::api::AgentInput;
use crate::agent::api::SpawnRequest;
use crate::agent::types::AgentMessage;
use crate::agent::types::MessageDeliveryMode;
use crate::agent::types::SpawnAgentOptions;
use crate::codex_thread::CodexThread;
use codex_features::Feature;
use codex_login::CodexAuth;
use codex_utils_absolute_path::test_support::PathBufExt;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use std::time::Duration;
use tempfile::TempDir;
use test_case::test_case;
use tokio::sync::Notify;
use tokio::sync::oneshot;
use tokio::time::timeout;
use wiremock::Mock;
use wiremock::Request;
use wiremock::matchers::method;
use wiremock::matchers::path_regex;

async fn manager(server: &wiremock::MockServer) -> (TempDir, Config, ThreadManager) {
    let home = tempfile::tempdir().expect("isolated home");
    let mut config = crate::config::test_config().await;
    config.codex_home = home.path().join("codex-home").abs();
    config.cwd = home.path().to_path_buf().abs();
    config.sqlite = codex_state::SqliteConfig::new_for_testing(config.codex_home.clone());
    config.model_provider.base_url = Some(format!("{}/v1", server.uri()));
    config.model_provider.request_max_retries = Some(0);
    config.model_provider.stream_max_retries = Some(0);
    config.features.enable(Feature::MultiAgentV2).unwrap();
    config.features.disable(Feature::CodeMode).unwrap();
    std::fs::create_dir_all(&config.codex_home).unwrap();
    let manager = ThreadManager::with_models_provider_and_home_for_tests(
        CodexAuth::from_api_key("dummy"),
        config.model_provider.clone(),
        config.codex_home.to_path_buf(),
        Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
    );
    (home, config, manager)
}

fn source(parent: ThreadId) -> SessionSource {
    SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
        parent_thread_id: parent,
        depth: 1,
        agent_path: Some(AgentPath::root().join("owned").unwrap()),
        agent_nickname: None,
        agent_role: None,
    })
}

fn initial_input(message: bool, parent: ThreadId) -> SpawnInitialInput {
    if message {
        SpawnInitialInput::InterAgentCommunication(
            AgentMessage::Plaintext("retained input".into()).into_communication(
                AgentPath::root(),
                AgentPath::root().join("owned").unwrap(),
                MessageDeliveryMode::TriggerTurn,
            ),
            AgentCommunicationContext::new(AgentCommunicationKind::Spawn, parent),
        )
    } else {
        SpawnInitialInput::UserInput(vec![UserInput::Text {
            text: "retained input".into(),
            text_elements: Vec::new(),
        }])
    }
}

async fn hold_response(server: &wiremock::MockServer) {
    Mock::given(method("POST"))
        .and(path_regex(".*/responses$"))
        .and(|request: &Request| {
            request
                .body_json::<serde_json::Value>()
                .is_ok_and(|body| body.to_string().contains("retained input"))
        })
        .respond_with(
            responses::sse_response(responses::sse(vec![
                responses::ev_response_created("owned"),
                responses::ev_completed("owned"),
            ]))
            .set_delay(Duration::from_secs(/*secs*/ 30)),
        )
        .mount(server)
        .await;
}

async fn replacement(
    server: &wiremock::MockServer,
    manager: &ThreadManager,
    id: ThreadId,
) -> (TempDir, ThreadManager, Arc<CodexThread>) {
    // Independent home/writer intentionally isolates persistence while reproducing
    // the real manager publication collision, following tool_policy_tests.
    let (home, config, candidate_manager) = self::manager(server).await;
    let mut options = StartThreadOptions::new(config);
    options.reserved_thread_id = Some(id);
    options.environments = Some(Vec::new());
    let candidate = candidate_manager.start_thread(options).await.unwrap();
    assert_eq!(candidate.thread_id, id);
    manager
        .agent_control()
        .runtime
        .upgrade()
        .unwrap()
        .threads
        .write()
        .await
        .insert(id, Arc::clone(&candidate.thread));
    (home, candidate_manager, candidate.thread)
}

async fn wait_running(thread: &CodexThread) {
    let mut status = thread.subscribe_status();
    timeout(Duration::from_secs(/*secs*/ 10), async {
        loop {
            if matches!(*status.borrow_and_update(), AgentStatus::Running) {
                break;
            }
            status
                .changed()
                .await
                .expect("native status source remains alive");
        }
    })
    .await
    .expect("original inference admitted");
}

#[tokio::test]
#[test_case(false; "user_input")]
#[test_case(true; "agent_message")]
async fn retained_spawn_handoff_never_captures_same_id_replacement(message: bool) {
    let server = responses::start_mock_server().await;
    hold_response(&server).await;
    let (_home, config, manager) = self::manager(&server).await;
    let parent = manager
        .start_thread(StartThreadOptions {
            environments: Some(Vec::new()),
            ..StartThreadOptions::new(config.clone())
        })
        .await
        .unwrap();
    let control = manager.agent_control();
    let (entered, entered_rx) = oneshot::channel();
    let release = Arc::new(Notify::new());
    let resume = Arc::clone(&release);
    let caller = parent.thread_id;
    let input = match initial_input(message, caller) {
        SpawnInitialInput::UserInput(items) => AgentInput::UserInput(items),
        SpawnInitialInput::InterAgentCommunication(_, _) => AgentInput::Message {
            message: AgentMessage::Plaintext("retained input".into()),
            mode: MessageDeliveryMode::TriggerTurn,
        },
    };
    let admission_control = control.clone();
    let admission = tokio::spawn(async move {
        let admitted = admission_control
            .spawn_retained(SpawnRequest {
                caller,
                config,
                input,
                source: source(caller),
                options: SpawnAgentOptions {
                    parent_thread_id: Some(caller),
                    environments: Some(Default::default()),
                    ..Default::default()
                },
            })
            .await
            .unwrap();
        entered
            .send(Arc::clone(&admitted.thread))
            .unwrap_or_else(|_| panic!("handoff observer alive"));
        resume.notified().await;
        admitted
    });
    let original = timeout(Duration::from_secs(/*secs*/ 10), entered_rx)
        .await
        .unwrap()
        .unwrap();
    wait_running(&original).await;
    let id = original.session.thread_id;
    let (_candidate_home, _candidate_manager, candidate) = replacement(&server, &manager, id).await;
    release.notify_one();
    let admitted = admission.await.unwrap();
    assert!(Arc::ptr_eq(&admitted.thread, &original));
    assert!(!Arc::ptr_eq(&admitted.thread, &candidate));
    shutdown_owned_child(&control, &admitted.thread)
        .await
        .unwrap();
    original.wait_until_terminated().await;
    assert!(!original.is_running());
    assert!(candidate.is_running());
    assert!(Arc::ptr_eq(
        &manager.get_thread(id).await.unwrap(),
        &candidate
    ));
    assert_eq!(
        control
            .runtime
            .registry
            .agent_id_for_path(&AgentPath::root().join("owned").unwrap()),
        Some(id)
    );
    candidate.shutdown_and_wait().await.unwrap();
    parent.thread.shutdown_and_wait().await.unwrap();
}

#[tokio::test]
#[test_case(false; "user_input")]
#[test_case(true; "agent_message")]
async fn initial_input_targets_exact_runtime_after_same_id_replacement(message: bool) {
    let server = responses::start_mock_server().await;
    hold_response(&server).await;
    let (_home, config, manager) = self::manager(&server).await;
    let control = manager.agent_control();
    let original = manager
        .start_thread(StartThreadOptions {
            environments: Some(Vec::new()),
            ..StartThreadOptions::new(config)
        })
        .await
        .unwrap();
    let id = original.thread_id;
    control
        .runtime
        .register_session_root(id, /*current_parent_thread_id*/ None);
    let (_candidate_home, _candidate_manager, candidate) = replacement(&server, &manager, id).await;
    let state = control.runtime.upgrade().unwrap();
    control
        .admit_input_to_thread(
            &original.thread,
            &state,
            initial_input(message, id),
            TurnStartOptions::default(),
        )
        .await
        .unwrap();
    wait_running(&original.thread).await;
    assert!(!matches!(
        candidate.agent_status().await,
        AgentStatus::Running
    ));
    assert!(candidate.is_running());
    shutdown_owned_child(&control, &original.thread)
        .await
        .unwrap();
    assert!(Arc::ptr_eq(
        &manager.get_thread(id).await.unwrap(),
        &candidate
    ));
    assert_eq!(
        control
            .runtime
            .registry
            .agent_id_for_path(&AgentPath::root()),
        Some(id)
    );
    candidate.shutdown_and_wait().await.unwrap();
}

#[tokio::test]
async fn v2_message_admission_refuses_unpublished_original_without_touching_replacement() {
    let server = responses::start_mock_server().await;
    let (_home, config, manager) = self::manager(&server).await;
    let control = manager.agent_control();
    let parent = manager
        .start_thread(StartThreadOptions {
            environments: Some(Vec::new()),
            ..StartThreadOptions::new(config.clone())
        })
        .await
        .unwrap();
    let original = manager
        .start_thread(StartThreadOptions {
            environments: Some(Vec::new()),
            session_source: Some(source(parent.thread_id)),
            ..StartThreadOptions::new(config)
        })
        .await
        .unwrap();
    assert_eq!(
        original.thread.multi_agent_version(),
        Some(MultiAgentVersion::V2)
    );
    let (_candidate_home, _candidate_manager, candidate) =
        replacement(&server, &manager, original.thread_id).await;
    let state = control.runtime.upgrade().unwrap();
    let result = control
        .admit_input_to_thread(
            &original.thread,
            &state,
            initial_input(/*message*/ true, parent.thread_id),
            TurnStartOptions::default(),
        )
        .await;
    assert!(result.as_ref().is_err_and(|error| matches!(
        error.details(),
        CodexErrorDetails::ThreadNotFound(id) if *id == original.thread_id
    )));
    assert!(candidate.is_running());
    assert!(!matches!(
        candidate.agent_status().await,
        AgentStatus::Running
    ));
    assert!(Arc::ptr_eq(
        &manager.get_thread(original.thread_id).await.unwrap(),
        &candidate
    ));
    shutdown_owned_child(&control, &original.thread)
        .await
        .unwrap();
    assert!(candidate.is_running());
    candidate.shutdown_and_wait().await.unwrap();
    parent.thread.shutdown_and_wait().await.unwrap();
}

#[tokio::test]
async fn pending_spawn_drop_stops_only_exact_runtime_after_replacement() {
    let server = responses::start_mock_server().await;
    let (_home, config, manager) = self::manager(&server).await;
    let original = manager
        .start_thread(StartThreadOptions {
            environments: Some(Vec::new()),
            ..StartThreadOptions::new(config)
        })
        .await
        .unwrap();
    let control = manager.agent_control();
    let state = control.runtime.upgrade().unwrap();
    control
        .runtime
        .register_session_root(original.thread_id, /*current_parent_thread_id*/ None);
    assert_eq!(
        control
            .runtime
            .registry
            .agent_id_for_path(&AgentPath::root()),
        Some(original.thread_id)
    );
    let mut guard = PendingSpawn::new(
        Arc::clone(&state),
        Arc::clone(&original.thread),
        control.clone(),
    );
    let cleaned = guard.notify_when_cleaned();
    let (_candidate_home, _candidate_manager, candidate) =
        replacement(&server, &manager, original.thread_id).await;
    drop(guard);
    timeout(Duration::from_secs(/*secs*/ 10), cleaned)
        .await
        .unwrap()
        .unwrap();
    timeout(
        Duration::from_secs(/*secs*/ 10),
        original.thread.wait_until_terminated(),
    )
    .await
    .unwrap();
    assert!(!original.thread.is_running());
    assert!(candidate.is_running());
    assert!(Arc::ptr_eq(
        &manager.get_thread(original.thread_id).await.unwrap(),
        &candidate
    ));
    assert_eq!(
        control
            .runtime
            .registry
            .agent_id_for_path(&AgentPath::root()),
        Some(original.thread_id)
    );
    candidate.shutdown_and_wait().await.unwrap();
}

#[tokio::test]
#[test_case(ThreadHistoryMode::Legacy; "legacy")]
#[test_case(ThreadHistoryMode::Paginated; "paginated")]
async fn stale_guard_cannot_discard_resumed_writer_in_same_store(history_mode: ThreadHistoryMode) {
    let server = responses::start_mock_server().await;
    hold_response(&server).await;
    let (_home, config, manager) = self::manager(&server).await;
    let control = manager.agent_control();
    let original = manager
        .start_thread(StartThreadOptions {
            environments: Some(Vec::new()),
            history_mode: Some(history_mode),
            ..StartThreadOptions::new(config.clone())
        })
        .await
        .unwrap();
    let state = control.runtime.upgrade().unwrap();
    let mut guard = PendingSpawn::new(
        Arc::clone(&state),
        Arc::clone(&original.thread),
        control.clone(),
    );
    let cleaned = guard.notify_when_cleaned();
    original.thread.ensure_rollout_materialized().await;
    original.thread.flush_rollout().await.unwrap();
    let live = original
        .thread
        .session
        .live_thread()
        .expect("original local persistence");
    let path = live.local_rollout_path().await.unwrap().unwrap();
    let history = codex_rollout::RolloutRecorder::get_rollout_history(&path)
        .await
        .unwrap();
    original.thread.shutdown_and_wait().await.unwrap();
    assert!(
        live.local_rollout_path().await.is_err(),
        "native shutdown retired original writer"
    );
    manager
        .remove_thread_if_matches(&original.thread_id, &original.thread)
        .await
        .unwrap();
    let resumed = manager
        .start_thread(StartThreadOptions {
            initial_history: history,
            history_mode: Some(history_mode),
            environments: Some(Vec::new()),
            ..StartThreadOptions::new(config)
        })
        .await
        .unwrap();
    assert_eq!(resumed.thread_id, original.thread_id);
    assert!(!Arc::ptr_eq(&resumed.thread, &original.thread));
    let resumed_live = resumed
        .thread
        .session
        .live_thread()
        .expect("resumed shared store persistence");
    assert_eq!(
        resumed_live.local_rollout_path().await.unwrap(),
        Some(path.clone())
    );
    drop(guard);
    timeout(Duration::from_secs(/*secs*/ 10), cleaned)
        .await
        .unwrap()
        .unwrap();
    assert!(resumed.thread.is_running());
    assert!(Arc::ptr_eq(
        &manager.get_thread(original.thread_id).await.unwrap(),
        &resumed.thread
    ));
    control
        .admit_input_to_thread(
            &resumed.thread,
            &state,
            initial_input(/*message*/ false, resumed.thread_id),
            TurnStartOptions::default(),
        )
        .await
        .unwrap();
    wait_running(&resumed.thread).await;
    resumed
        .thread
        .flush_rollout()
        .await
        .expect("replacement writer remains writable");
    assert_eq!(resumed_live.local_rollout_path().await.unwrap(), Some(path));
    resumed.thread.shutdown_and_wait().await.unwrap();
}

#[tokio::test]
async fn dead_original_request_never_unpublishes_same_id_replacement() {
    let server = responses::start_mock_server().await;
    let (_home, config, manager) = self::manager(&server).await;
    let control = manager.agent_control();
    let original = manager
        .start_thread(StartThreadOptions {
            environments: Some(Vec::new()),
            ..StartThreadOptions::new(config)
        })
        .await
        .unwrap();
    original.thread.shutdown_and_wait().await.unwrap();
    control
        .runtime
        .register_session_root(original.thread_id, /*current_parent_thread_id*/ None);
    let (_candidate_home, _candidate_manager, candidate) =
        replacement(&server, &manager, original.thread_id).await;
    let state = control.runtime.upgrade().unwrap();
    let result = control
        .admit_input_to_thread(
            &original.thread,
            &state,
            initial_input(/*message*/ false, original.thread_id),
            TurnStartOptions::default(),
        )
        .await;
    assert!(
        result
            .as_ref()
            .is_err_and(|error| matches!(error.details(), CodexErrorDetails::InternalAgentDied))
    );
    assert!(candidate.is_running());
    assert!(Arc::ptr_eq(
        &manager.get_thread(original.thread_id).await.unwrap(),
        &candidate
    ));
    assert_eq!(
        control
            .runtime
            .registry
            .agent_id_for_path(&AgentPath::root()),
        Some(original.thread_id)
    );
    candidate.shutdown_and_wait().await.unwrap();
}

#[tokio::test]
async fn dead_initial_admission_closes_open_edge_when_exact_runtime_already_removed() {
    use codex_agent_graph_store::ThreadSpawnEdgeStatus;
    let server = responses::start_mock_server().await;
    let (_home, config, _unused_manager) = self::manager(&server).await;
    let state_db =
        codex_state::StateRuntime::init(config.sqlite.clone(), config.model_provider_id.clone())
            .await
            .unwrap();
    let manager = ThreadManager::with_models_provider_home_and_state_for_tests(
        CodexAuth::from_api_key("dummy"),
        config.model_provider.clone(),
        config.codex_home.to_path_buf(),
        Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
        Some(state_db),
    );
    let control = manager.agent_control();
    let parent = manager
        .start_thread(StartThreadOptions {
            environments: Some(Vec::new()),
            ..StartThreadOptions::new(config.clone())
        })
        .await
        .unwrap();
    let original = manager
        .start_thread(StartThreadOptions {
            environments: Some(Vec::new()),
            ..StartThreadOptions::new(config)
        })
        .await
        .unwrap();
    parent.thread.ensure_rollout_materialized().await;
    original.thread.ensure_rollout_materialized().await;
    let state = control.runtime.upgrade().unwrap();
    let graph = state.agent_graph_store().expect("real SQLite graph");
    graph
        .upsert_thread_spawn_edge(
            parent.thread_id,
            original.thread_id,
            ThreadSpawnEdgeStatus::Open,
        )
        .await
        .unwrap();
    let mut guard = PendingSpawn::new(
        Arc::clone(&state),
        Arc::clone(&original.thread),
        control.clone(),
    );
    let cleaned = guard.notify_when_cleaned();
    original.thread.shutdown_and_wait().await.unwrap();
    let result = control
        .admit_input_to_thread(
            &original.thread,
            &state,
            initial_input(/*message*/ false, parent.thread_id),
            TurnStartOptions::default(),
        )
        .await;
    assert!(
        result
            .as_ref()
            .is_err_and(|error| matches!(error.details(), CodexErrorDetails::InternalAgentDied))
    );
    assert!(manager.get_thread(original.thread_id).await.is_err());
    drop(guard);
    timeout(Duration::from_secs(/*secs*/ 10), cleaned)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        graph
            .list_thread_spawn_children(parent.thread_id, Some(ThreadSpawnEdgeStatus::Closed))
            .await
            .unwrap(),
        vec![original.thread_id]
    );
    assert!(
        graph
            .list_thread_spawn_children(parent.thread_id, Some(ThreadSpawnEdgeStatus::Open))
            .await
            .unwrap()
            .is_empty()
    );
    parent.thread.shutdown_and_wait().await.unwrap();
}
