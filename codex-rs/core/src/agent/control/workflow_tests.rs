//! Native runtime fixtures: no workflow CLI, JS runner, or live inference is claimed.
use super::*;
use crate::agent::control::workflow::NativeWorkflowBridge;
use crate::agent::control::workflow::WorkflowAgentCall;
use crate::agent::control::workflow::WorkflowInput;
use crate::tools::context::ToolCallSource;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::turn_diff_tracker::TurnDiffTracker;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::collections::HashSet;
use tokio::sync::Mutex;
use wiremock::Mock;
use wiremock::Request;
use wiremock::matchers::method;
use wiremock::matchers::path_regex;

#[path = "workflow_boundary_tests.rs"]
mod boundary_tests;

#[path = "workflow_persistence_tests.rs"]
mod persistence_tests;

#[path = "workflow_reporting_tests.rs"]
mod reporting_tests;

async fn bridge_harness(
    server: &wiremock::MockServer,
) -> (AgentControlHarness, Arc<CodexThread>, ToolInvocation) {
    let (home, mut config) = test_config().await;
    config.model_provider.base_url = Some(format!("{}/v1", server.uri()));
    config.model_provider.request_max_retries = Some(0);
    config.model_provider.stream_max_retries = Some(0);
    config.features.enable(Feature::MultiAgentV2).unwrap();
    config.features.disable(Feature::CodeMode).unwrap();
    let harness = AgentControlHarness::new_with_config(home, config).await;
    let parent = harness
        .manager
        .start_thread(StartThreadOptions {
            environments: Some(Vec::new()),
            ..StartThreadOptions::new(harness.config.clone())
        })
        .await
        .unwrap();
    let turn = parent
        .thread
        .session
        .new_turn_with_default_settings("workflow-fixture".into(), Default::default())
        .await;
    let invocation = ToolInvocation {
        session: Arc::clone(&parent.thread.session),
        step_context: StepContext::for_test(Arc::clone(&turn)),
        turn,
        cancellation_token: CancellationToken::new(),
        tracker: Arc::new(Mutex::new(TurnDiffTracker::default())),
        call_id: "workflow-fixture-call".into(),
        tool_name: codex_tools::ToolName::plain("Workflow"),
        source: ToolCallSource::Direct,
        payload: ToolPayload::Function {
            arguments: "{}".into(),
        },
    };
    (harness, parent.thread, invocation)
}

fn call(task_name: &str, input: WorkflowInput) -> WorkflowAgentCall {
    WorkflowAgentCall {
        task_name: task_name.into(),
        input,
        role_name: None,
        model: None,
        effort: None,
        compatible: false,
        label: None,
        schema: json!({"type":"object", "properties":{"answer":{"type":"string"}},
            "required":["answer"], "additionalProperties":false}),
    }
}

async fn child_response(
    server: &wiremock::MockServer,
    prompt: &str,
    answer: &str,
    delay: Duration,
) {
    let prompt = prompt.to_string();
    Mock::given(method("POST"))
        .and(path_regex(".*/responses$"))
        .and(move |request: &Request| {
            let body = request.body_json::<serde_json::Value>().unwrap();
            is_turn_body(&body) && body.to_string().contains(&prompt)
        })
        .respond_with(
            responses::sse_response(responses::sse(vec![
                responses::ev_response_created("workflow-child"),
                responses::ev_assistant_message("result", answer),
                responses::ev_completed("workflow-child"),
            ]))
            .set_delay(delay),
        )
        .up_to_n_times(1)
        .mount(server)
        .await;
}

fn is_turn_body(body: &serde_json::Value) -> bool {
    let metadata = body["client_metadata"]["x-codex-turn-metadata"]
        .as_str()
        .expect("native request has canonical metadata");
    serde_json::from_str::<serde_json::Value>(metadata).unwrap()["request_kind"] == "turn"
}

async fn turn_requests(server: &wiremock::MockServer) -> Vec<responses::ResponsesRequest> {
    responses::received_responses_requests(server)
        .await
        .into_iter()
        .filter(|request| is_turn_body(&request.body_json()))
        .collect()
}

#[tokio::test]
async fn waits_for_all_children_in_input_order_and_closes_only_owned_threads() -> anyhow::Result<()>
{
    let server = responses::start_mock_server().await;
    child_response(
        &server,
        "first-sentinel",
        r#"{"answer":"first"}"#,
        Duration::ZERO,
    )
    .await;
    child_response(
        &server,
        "second-sentinel",
        r#"{"answer":"second"}"#,
        Duration::from_millis(/*millis*/ 150),
    )
    .await;
    let (harness, parent, invocation) = bridge_harness(&server).await;
    let unrelated = harness
        .manager
        .start_thread(StartThreadOptions {
            environments: Some(Vec::new()),
            ..StartThreadOptions::new(harness.config.clone())
        })
        .await?;
    let results = timeout(
        Duration::from_secs(/*secs*/ 15),
        NativeWorkflowBridge::capture(&invocation).run(vec![
            call("first", WorkflowInput::UserInput("first-sentinel".into())),
            call(
                "second",
                WorkflowInput::AgentMessage("second-sentinel".into()),
            ),
        ]),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    assert_eq!(
        results,
        vec![json!({"answer":"first"}), json!({"answer":"second"})]
    );
    assert_eq!(
        harness
            .manager
            .list_thread_ids()
            .await
            .into_iter()
            .collect::<HashSet<_>>(),
        HashSet::from([parent.session.thread_id, unrelated.thread_id])
    );
    let requests = turn_requests(&server).await;
    assert_eq!(requests.len(), 2);
    let mut child_ids = HashSet::new();
    for request in requests {
        let body = request.body_json();
        assert_eq!(
            body["text"]["format"]["schema"],
            call("unused", WorkflowInput::UserInput("unused".into())).schema
        );
        let id = request.header("thread-id").unwrap();
        assert!(child_ids.insert(ThreadId::from_string(&id)?));
        assert_ne!(id, parent.session.thread_id.to_string());
        let tools = body["tools"].to_string();
        assert!(!tools.contains("spawn_agent"));
        assert!(!tools.contains("followup_task"));
    }
    unrelated.thread.shutdown_and_wait().await?;
    parent.shutdown_and_wait().await?;
    Ok(())
}

#[tokio::test]
async fn rejects_late_invalid_schema_and_forged_task_path_without_admitting_children()
-> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    let (harness, parent, invocation) = bridge_harness(&server).await;
    for invalid in [
        call("../outside", WorkflowInput::AgentMessage("bad".into())),
        WorkflowAgentCall {
            schema: json!({"type":"string","pattern":"bad"}),
            ..call("late", WorkflowInput::AgentMessage("bad".into()))
        },
    ] {
        assert!(
            NativeWorkflowBridge::capture(&invocation)
                .run(vec![
                    call("valid", WorkflowInput::UserInput("must-never-run".into())),
                    invalid,
                ])
                .await
                .is_err()
        );
        assert_eq!(
            harness.manager.list_thread_ids().await,
            vec![parent.session.thread_id]
        );
        assert!(turn_requests(&server).await.is_empty());
    }
    parent.shutdown_and_wait().await?;
    Ok(())
}

#[tokio::test]
async fn invalid_result_closes_admitted_children_before_returning() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    child_response(
        &server,
        "invalid-result",
        r#"{"answer":false}"#,
        Duration::ZERO,
    )
    .await;
    let (harness, parent, invocation) = bridge_harness(&server).await;
    assert!(
        NativeWorkflowBridge::capture(&invocation)
            .run(vec![call(
                "invalid",
                WorkflowInput::AgentMessage("invalid-result".into())
            ),])
            .await
            .is_err()
    );
    assert_eq!(
        harness.manager.list_thread_ids().await,
        vec![parent.session.thread_id]
    );
    parent.shutdown_and_wait().await?;
    Ok(())
}

#[tokio::test]
async fn cancellation_during_inference_waits_for_owned_child_shutdown() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    child_response(
        &server,
        "cancel-sentinel",
        r#"{"answer":"late"}"#,
        Duration::from_secs(/*secs*/ 10),
    )
    .await;
    let (harness, parent, invocation) = bridge_harness(&server).await;
    let bridge = NativeWorkflowBridge::capture(&invocation);
    let task = tokio::spawn(bridge.run(vec![call(
        "cancel",
        WorkflowInput::AgentMessage("cancel-sentinel".into()),
    )]));
    timeout(Duration::from_secs(/*secs*/ 10), async {
        while turn_requests(&server).await.is_empty() {
            tokio::time::sleep(Duration::from_millis(/*millis*/ 10)).await;
        }
    })
    .await?;
    invocation.cancellation_token.cancel();
    assert!(
        timeout(Duration::from_secs(/*secs*/ 10), task)
            .await??
            .is_err()
    );
    assert_eq!(
        harness.manager.list_thread_ids().await,
        vec![parent.session.thread_id]
    );
    parent.shutdown_and_wait().await?;
    Ok(())
}
