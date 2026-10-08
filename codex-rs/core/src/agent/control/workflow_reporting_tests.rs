//! Native terminal delivery must stay private until the supervisor validates it.
use super::*;
use crate::agent::api::AgentInput;
use crate::agent::api::SpawnRequest;
use crate::agent::control::CompletionReporting;
use crate::agent::types::AgentMessage;
use crate::agent::types::MessageDeliveryMode;
use crate::agent::types::SpawnAgentOptions;
use codex_protocol::AgentPath;
use codex_protocol::error::CodexErrorDetails;
use pretty_assertions::assert_eq;
use test_case::test_case;

#[test_case(MultiAgentVersion::V2, false, CompletionReporting::SupervisorOwned; "v2_user_supervised")]
#[test_case(MultiAgentVersion::V2, true, CompletionReporting::SupervisorOwned; "v2_message_supervised")]
#[test_case(MultiAgentVersion::V1, false, CompletionReporting::SupervisorOwned; "legacy_user_supervised")]
#[test_case(MultiAgentVersion::V1, true, CompletionReporting::SupervisorOwned; "legacy_message_supervised")]
#[test_case(MultiAgentVersion::V2, false, CompletionReporting::Automatic; "v2_user_automatic")]
#[test_case(MultiAgentVersion::V2, true, CompletionReporting::Automatic; "v2_message_automatic")]
#[test_case(MultiAgentVersion::V1, false, CompletionReporting::Automatic; "legacy_user_automatic")]
#[test_case(MultiAgentVersion::V1, true, CompletionReporting::Automatic; "legacy_message_automatic")]
#[tokio::test]
async fn native_completion_routing_respects_admission_policy(
    version: MultiAgentVersion,
    message: bool,
    reporting: CompletionReporting,
) -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    child_response(
        &server,
        "reporting-prompt",
        "UNVALIDATED_REPORTING_CANARY",
        Duration::ZERO,
    )
    .await;
    let (home, mut config) = test_config().await;
    config.model_provider.base_url = Some(format!("{}/v1", server.uri()));
    config.model_provider.request_max_retries = Some(0);
    config.model_provider.stream_max_retries = Some(0);
    config.features.disable(Feature::CodeMode)?;
    if version == MultiAgentVersion::V2 {
        config.features.enable(Feature::MultiAgentV2)?;
    } else {
        config.features.disable(Feature::MultiAgentV2)?;
        config.features.enable(Feature::Collab)?;
    }
    let harness = AgentControlHarness::new_with_config(home, config).await;
    let parent = harness
        .manager
        .start_thread(StartThreadOptions {
            environments: Some(Vec::new()),
            ..StartThreadOptions::new(harness.config.clone())
        })
        .await?;
    let before = parent
        .thread
        .session
        .clone_history()
        .await
        .raw_items()
        .cloned()
        .collect::<Vec<_>>();
    let source = SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
        parent_thread_id: parent.thread_id,
        depth: 1,
        agent_path: Some(
            AgentPath::root()
                .join("reporting")
                .map_err(anyhow::Error::msg)?,
        ),
        agent_nickname: None,
        agent_role: None,
    });
    let input = if message {
        AgentInput::Message {
            message: AgentMessage::Plaintext("reporting-prompt".into()),
            mode: MessageDeliveryMode::TriggerTurn,
        }
    } else {
        AgentInput::UserInput(vec![UserInput::Text {
            text: "reporting-prompt".into(),
            text_elements: Vec::new(),
        }])
    };
    let control = harness.manager.agent_control();
    let request = SpawnRequest {
        caller: parent.thread_id,
        config: harness.config.clone(),
        input,
        source,
        options: SpawnAgentOptions {
            parent_thread_id: Some(parent.thread_id),
            environments: Some(crate::environment_selection::TurnEnvironmentSnapshot {
                environments: Vec::new(),
            }),
            ..Default::default()
        },
    };
    let admitted = if reporting == CompletionReporting::Automatic {
        let (agent, config) = crate::agent::api::AgentControl::spawn(&control, request).await?;
        let thread = harness.manager.get_thread(agent.thread_id).await?;
        super::super::spawn_admission::AdmittedAgent {
            agent,
            config,
            thread,
        }
    } else {
        control
            .spawn_retained_with_reporting(request, reporting)
            .await?
    };
    let mut status = admitted.thread.subscribe_status();
    timeout(Duration::from_secs(/*secs*/ 15), async {
        loop {
            if matches!(*status.borrow_and_update(), AgentStatus::Completed(_)) {
                break;
            }
            status.changed().await.expect("child status sender");
        }
    })
    .await?;
    // AgentMessage admission returns before its asynchronous Full turn context
    // resolves the inherited legacy version. Assert the version actually run.
    assert_eq!(admitted.thread.multi_agent_version(), Some(version));
    assert_eq!(
        admitted.thread.agent_status().await,
        AgentStatus::Completed(Some("UNVALIDATED_REPORTING_CANARY".into()))
    );
    // Mutable extension data cannot change the policy captured before startup.
    if reporting == CompletionReporting::SupervisorOwned {
        admitted
            .thread
            .session
            .services
            .thread_extension_data
            .insert(CompletionReporting::Automatic);
        assert_eq!(
            admitted.thread.session.completion_reporting(),
            CompletionReporting::SupervisorOwned
        );
        let terminal_turn = admitted.thread.session.new_default_turn().await;
        admitted
            .thread
            .session
            .send_event(
                &terminal_turn,
                EventMsg::TurnComplete(codex_protocol::protocol::TurnCompleteEvent {
                    turn_id: terminal_turn.sub_id.clone(),
                    last_agent_message: Some("UNVALIDATED_REPORTING_CANARY".into()),
                    error: None,
                    started_at: None,
                    completed_at: None,
                    duration_ms: None,
                    time_to_first_token_ms: None,
                }),
            )
            .await;
    }
    if reporting == CompletionReporting::Automatic {
        timeout(Duration::from_secs(/*secs*/ 5), async {
            loop {
                if parent
                    .thread
                    .session
                    .input_queue
                    .has_pending_input(&parent.thread.session.active_turn)
                    .await
                    || history_contains_text(
                        parent.thread.session.clone_history().await.raw_items(),
                        "UNVALIDATED_REPORTING_CANARY",
                    )
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await?;
    }
    admitted.thread.shutdown_and_wait().await?;
    if reporting == CompletionReporting::SupervisorOwned {
        assert!(
            !parent
                .thread
                .session
                .input_queue
                .has_pending_input(&parent.thread.session.active_turn)
                .await
        );
        assert_eq!(
            parent
                .thread
                .session
                .clone_history()
                .await
                .raw_items()
                .cloned()
                .collect::<Vec<_>>(),
            before
        );
    }
    parent.thread.shutdown_and_wait().await?;
    Ok(())
}

#[test_case("{invalid"; "invalid_json")]
#[test_case("{\"other\":\"UNVALIDATED_REPORTING_CANARY\"}"; "invalid_schema")]
#[test_case("{\"answer\":\"VALID_REPORTING_CANARY\"}"; "validated_result")]
#[tokio::test]
async fn bridge_results_never_enter_the_parent_mailbox_or_history(
    output: &str,
) -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    child_response(&server, "private-bridge-prompt", output, Duration::ZERO).await;
    let (_harness, parent, invocation) = bridge_harness(&server).await;
    let before = parent
        .session
        .clone_history()
        .await
        .raw_items()
        .cloned()
        .collect::<Vec<_>>();
    let result = NativeWorkflowBridge::capture(&invocation)
        .run(vec![call(
            "private_result",
            WorkflowInput::AgentMessage("private-bridge-prompt".into()),
        )])
        .await;
    let expected = match output {
        "{invalid" => Err("invalid or unbounded workflow result JSON".into()),
        r#"{"other":"UNVALIDATED_REPORTING_CANARY"}"# => {
            Err("workflow result is missing a required property".into())
        }
        r#"{"answer":"VALID_REPORTING_CANARY"}"# => {
            Ok(vec![json!({"answer":"VALID_REPORTING_CANARY"})])
        }
        unexpected => panic!("unsupported reporting fixture output: {unexpected}"),
    };
    assert_eq!(result, expected);
    assert_eq!(turn_requests(&server).await.len(), 1);
    assert!(
        !parent
            .session
            .input_queue
            .has_pending_input(&parent.session.active_turn)
            .await
    );
    assert_eq!(
        parent
            .session
            .clone_history()
            .await
            .raw_items()
            .cloned()
            .collect::<Vec<_>>(),
        before
    );
    parent.shutdown_and_wait().await?;
    Ok(())
}

#[tokio::test]
async fn excessive_child_result_is_not_reported_to_the_parent() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    let output = serde_json::to_string(&json!({"answer":"X".repeat(9000)}))?;
    child_response(&server, "private-large-prompt", &output, Duration::ZERO).await;
    let (_harness, parent, invocation) = bridge_harness(&server).await;
    let before = parent
        .session
        .clone_history()
        .await
        .raw_items()
        .cloned()
        .collect::<Vec<_>>();
    let result = NativeWorkflowBridge::capture(&invocation)
        .run(vec![call(
            "private_large",
            WorkflowInput::UserInput("private-large-prompt".into()),
        )])
        .await;
    assert_eq!(result, Err("workflow result exceeds 8192 bytes".into()));
    assert_eq!(turn_requests(&server).await.len(), 1);
    assert!(
        !parent
            .session
            .input_queue
            .has_pending_input(&parent.session.active_turn)
            .await
    );
    assert_eq!(
        parent
            .session
            .clone_history()
            .await
            .raw_items()
            .cloned()
            .collect::<Vec<_>>(),
        before
    );
    parent.shutdown_and_wait().await?;
    Ok(())
}

#[tokio::test]
async fn supervised_admission_refuses_a_host_controller_before_factory_or_publication()
-> anyhow::Result<()> {
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    let (_home, config) = test_config().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let manager = ThreadManager::with_models_provider_and_home_for_tests(
        CodexAuth::from_api_key("dummy"),
        config.model_provider.clone(),
        config.codex_home.to_path_buf(),
        Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
    )
    .with_agent_control_factory(move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
        async {
            Err(CodexErr::Fatal(
                "fixture controller must not be called".into(),
            ))
        }
    });
    let mut options = StartThreadOptions::new(config);
    options.environments = Some(Vec::new());
    options
        .thread_extension_init
        .insert(CompletionReporting::SupervisorOwned);
    let result = manager.start_thread(options).await;
    let error = result
        .err()
        .expect("host-backed supervised startup must be refused");
    assert!(
        matches!(error.details(), CodexErrorDetails::UnsupportedOperation(message)
        if message == "supervised admission requires the local agent backend")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(manager.list_thread_ids().await.is_empty());
    Ok(())
}

#[test_case(CompletionReporting::Automatic, CompletionReporting::SupervisorOwned; "automatic_cannot_become_supervised")]
#[test_case(CompletionReporting::SupervisorOwned, CompletionReporting::Automatic; "supervised_cannot_become_automatic")]
#[tokio::test]
async fn warm_resume_refuses_a_different_completion_owner(
    original: CompletionReporting,
    requested: CompletionReporting,
) -> anyhow::Result<()> {
    let (_home, config) = test_config().await;
    let manager = ThreadManager::with_models_provider_and_home_for_tests(
        CodexAuth::from_api_key("dummy"),
        config.model_provider.clone(),
        config.codex_home.to_path_buf(),
        Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
    );
    let mut options = StartThreadOptions::new(config.clone());
    options.environments = Some(Vec::new());
    options.thread_extension_init.insert(original);
    let parent = manager.start_thread(options).await?;
    let mut resume = StartThreadOptions::new(config);
    resume.environments = Some(Vec::new());
    resume.initial_history =
        codex_history::InitialHistory::Resumed(codex_history::ResumedHistory {
            conversation_id: parent.thread_id,
            history: Arc::new(Vec::new()),
            rollout_path: parent.thread.rollout_path(),
        });
    resume.thread_extension_init.insert(requested);
    let error = manager
        .start_thread(resume)
        .await
        .err()
        .expect("completion owner mismatch must be refused");
    assert!(
        matches!(error.details(), CodexErrorDetails::InvalidRequest(message)
        if message == "live thread completion reporting does not match the requested admission")
    );
    assert_eq!(manager.list_thread_ids().await, vec![parent.thread_id]);
    assert!(Arc::ptr_eq(
        &manager.get_thread(parent.thread_id).await?,
        &parent.thread
    ));
    assert_eq!(parent.thread.session.completion_reporting(), original);
    parent.thread.shutdown_and_wait().await?;
    Ok(())
}

#[test_case(false; "user_input")]
#[test_case(true; "agent_message")]
#[tokio::test]
async fn forked_supervised_child_keeps_its_raw_result_out_of_the_parent(
    message: bool,
) -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    child_response(
        &server,
        "private-fork-prompt",
        "UNVALIDATED_FORK_CANARY",
        Duration::ZERO,
    )
    .await;
    let (harness, parent, invocation) = bridge_harness(&server).await;
    parent
        .session
        .record_conversation_items(
            &invocation.turn,
            invocation.turn.model_info(),
            &[
                user_message("fork seed"),
                spawn_agent_call("fork-reporting-call"),
            ],
        )
        .await;
    let before = parent
        .session
        .clone_history()
        .await
        .raw_items()
        .cloned()
        .collect::<Vec<_>>();
    let input = if message {
        AgentInput::Message {
            message: AgentMessage::Plaintext("private-fork-prompt".into()),
            mode: MessageDeliveryMode::TriggerTurn,
        }
    } else {
        AgentInput::UserInput(text_input("private-fork-prompt"))
    };
    let control = harness.manager.agent_control();
    let admitted = control
        .spawn_retained_with_reporting(
            SpawnRequest {
                caller: parent.session.thread_id,
                config: harness.config.clone(),
                input,
                source: SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
                    parent_thread_id: parent.session.thread_id,
                    depth: 1,
                    agent_path: Some(
                        AgentPath::root()
                            .join("reporting_fork")
                            .map_err(anyhow::Error::msg)?,
                    ),
                    agent_nickname: None,
                    agent_role: None,
                }),
                options: SpawnAgentOptions {
                    parent_thread_id: Some(parent.session.thread_id),
                    environments: Some(crate::environment_selection::TurnEnvironmentSnapshot {
                        environments: Vec::new(),
                    }),
                    fork_mode: Some(crate::agent::types::SpawnAgentForkMode::FullHistory),
                    fork_parent_spawn_call_id: Some("fork-reporting-call".into()),
                    ..Default::default()
                },
            },
            CompletionReporting::SupervisorOwned,
        )
        .await?;
    assert_eq!(
        admitted.thread.session.completion_reporting(),
        CompletionReporting::SupervisorOwned
    );
    let mut status = admitted.thread.subscribe_status();
    timeout(Duration::from_secs(/*secs*/ 15), async {
        loop {
            if matches!(*status.borrow_and_update(), AgentStatus::Completed(_)) {
                break;
            }
            status.changed().await.expect("fork child status sender");
        }
    })
    .await?;
    assert_eq!(
        admitted.thread.agent_status().await,
        AgentStatus::Completed(Some("UNVALIDATED_FORK_CANARY".into()))
    );
    admitted.thread.shutdown_and_wait().await?;
    assert!(
        !parent
            .session
            .input_queue
            .has_pending_input(&parent.session.active_turn)
            .await
    );
    assert_eq!(
        parent
            .session
            .clone_history()
            .await
            .raw_items()
            .cloned()
            .collect::<Vec<_>>(),
        before
    );
    parent.shutdown_and_wait().await?;
    Ok(())
}
