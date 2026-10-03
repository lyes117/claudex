use super::*;
use codex_protocol::protocol::InternalSessionSource;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::ReviewRequest;
use codex_protocol::protocol::ReviewTarget;
use codex_protocol::protocol::SessionSource;
use pretty_assertions::assert_eq;
use test_case::test_case;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[test_case(false; "parent_allows_plan")]
#[test_case(true; "parent_allows_nothing")]
async fn inherited_policy_limits_real_child_advertising_and_dispatch(
    empty_parent: bool,
) -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    let server = responses::start_mock_server().await;
    let fixture = test_codex()
        .with_config(|config| {
            config.update_plan_enabled = true;
            config
                .features
                .disable(codex_features::Feature::CodeMode)
                .expect("use direct tools for this fixture");
        })
        .build_with_auto_env(&server)
        .await?;
    let parent_policy = ToolPolicy {
        allowed_tools: Some(if empty_parent {
            vec![]
        } else {
            vec![ToolName::namespaced("functions", "update_plan")]
        }),
        expose_additional_permissions: false,
        ..Default::default()
    };
    let mut parent_options = StartThreadOptions::new(fixture.config.clone());
    parent_options.environments = Some(fixture.codex.environment_selections().await);
    parent_options
        .thread_extension_init
        .insert(SessionIsolation::Isolated);
    parent_options
        .thread_extension_init
        .insert(parent_policy.clone());
    let cancelled = CancellationToken::new();
    let tasks = TaskTracker::new();
    let parent = fixture
        .thread_manager
        .start_thread_until(parent_options, cancelled.clone().cancelled_owned(), &tasks)
        .await?;
    // Mutable extension attachments must not replace the captured parent authority.
    parent
        .thread
        .thread_extension_data()
        .insert(ToolPolicy::default());
    let mut child_options = StartThreadOptions::new(fixture.config.clone());
    child_options.session_source = Some(SessionSource::Internal(
        InternalSessionSource::MemoryConsolidation,
    ));
    child_options.environments = Some(fixture.codex.environment_selections().await);
    child_options
        .thread_extension_init
        .insert(SessionIsolation::Isolated);
    child_options.thread_extension_init.insert(ToolPolicy {
        allowed_tools: Some(vec![
            ToolName::plain("update_plan"),
            ToolName::plain("exec_command"),
        ]),
        ..Default::default()
    });
    let child = fixture
        .thread_manager
        .spawn_internal_session(parent.thread_id, child_options)
        .await?;
    let composed = child
        .thread
        .thread_extension_data()
        .get::<ToolPolicy>()
        .expect("effective startup policy");
    assert!(composed.is_subset_of(&parent_policy));
    assert_eq!(
        composed.allows(&ToolName::plain("update_plan")),
        !empty_parent
    );
    assert!(!composed.allows(&ToolName::plain("exec_command")));
    child
        .thread
        .thread_extension_data()
        .insert(ToolPolicy::default());

    let response = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "plan",
                    "update_plan",
                    r#"{"plan":[{"step":"inspect","status":"completed"}]}"#,
                ),
                responses::ev_function_call(
                    "excluded",
                    "exec_command",
                    r#"{"cmd":"echo should-not-run"}"#,
                ),
                responses::ev_completed("tools"),
            ]),
            responses::sse(vec![responses::ev_completed("done")]),
        ],
    )
    .await;
    child
        .thread
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "Try both tools".to_owned(),
            text_elements: Vec::new(),
        }]))
        .await?;
    wait_for_event(&child.thread, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    let requests = response.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0].body_json()["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .map(|tool| tool["name"].as_str().expect("tool name"))
            .collect::<Vec<_>>(),
        if empty_parent {
            vec![]
        } else {
            vec!["update_plan"]
        },
    );
    assert_eq!(
        requests[1].function_call_output("excluded")["output"],
        "unsupported call: exec_command"
    );
    assert_eq!(
        requests[1].function_call_output("plan")["output"],
        if empty_parent {
            "unsupported call: update_plan"
        } else {
            "Plan updated"
        }
    );
    child.thread.shutdown_and_wait().await?;
    cancelled.cancel();
    tasks.close();
    tasks.wait().await;
    fixture.codex.shutdown_and_wait().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn review_delegate_honors_empty_parent_policy() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    let server = responses::start_mock_server().await;
    let fixture = test_codex()
        .with_config(|config| {
            config
                .features
                .disable(codex_features::Feature::CodeMode)
                .expect("direct fixture");
        })
        .build_with_auto_env(&server)
        .await?;
    let mut options = StartThreadOptions::new(fixture.config.clone());
    options.environments = Some(fixture.codex.environment_selections().await);
    options
        .thread_extension_init
        .insert(SessionIsolation::Isolated);
    options.thread_extension_init.insert(ToolPolicy {
        allowed_tools: Some(vec![]),
        ..Default::default()
    });
    let cancelled = CancellationToken::new();
    let tasks = TaskTracker::new();
    let parent = fixture
        .thread_manager
        .start_thread_until(options, cancelled.clone().cancelled_owned(), &tasks)
        .await?;
    let response = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("review-tools"),
                responses::ev_function_call(
                    "excluded",
                    "exec_command",
                    r#"{"cmd":"echo should-not-run"}"#,
                ),
                responses::ev_completed("review-tools"),
            ]),
            responses::sse(vec![
                responses::ev_response_created("review-done"),
                responses::ev_assistant_message(
                    "result",
                    &json!({
                        "findings": [], "overall_correctness": "patch is correct",
                        "overall_explanation": "fixture", "overall_confidence_score": 1.0,
                    })
                    .to_string(),
                ),
                responses::ev_completed("review-done"),
            ]),
        ],
    )
    .await;
    parent
        .thread
        .submit(Op::Review {
            review_request: ReviewRequest {
                target: ReviewTarget::Custom {
                    instructions: "Review this synthetic fixture".to_owned(),
                },
                user_facing_hint: None,
            },
        })
        .await?;
    wait_for_event(&parent.thread, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    let requests = response.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].body_json()["tools"], json!([]));
    assert_eq!(
        requests[1].function_call_output("excluded")["output"],
        "unsupported call: exec_command"
    );
    cancelled.cancel();
    tasks.close();
    tasks.wait().await;
    fixture.codex.shutdown_and_wait().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inherited_policy_limits_code_mode_nested_tools() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    let server = responses::start_mock_server().await;
    let fixture = test_codex()
        .with_model_info_override("test-gpt-5.1-codex", |model| {
            model.tool_mode = Some(codex_protocol::openai_models::ToolMode::CodeModeOnly);
        })
        .with_config(|config| {
            config.update_plan_enabled = true;
            config
                .features
                .enable(codex_features::Feature::CodeMode)
                .expect("Code Mode fixture");
        })
        .build_with_auto_env(&server)
        .await?;
    let mut parent_options = StartThreadOptions::new(fixture.config.clone());
    parent_options.environments = Some(fixture.codex.environment_selections().await);
    parent_options
        .thread_extension_init
        .insert(SessionIsolation::Isolated);
    parent_options.thread_extension_init.insert(ToolPolicy {
        allowed_tools: Some(vec![
            ToolName::plain("exec"),
            ToolName::plain("wait"),
            ToolName::plain("update_plan"),
        ]),
        ..Default::default()
    });
    let cancelled = CancellationToken::new();
    let tasks = TaskTracker::new();
    let parent = fixture
        .thread_manager
        .start_thread_until(parent_options, cancelled.clone().cancelled_owned(), &tasks)
        .await?;
    let mut options = StartThreadOptions::new(fixture.config.clone());
    options.session_source = Some(SessionSource::Internal(
        InternalSessionSource::MemoryConsolidation,
    ));
    options.environments = Some(fixture.codex.environment_selections().await);
    options
        .thread_extension_init
        .insert(SessionIsolation::Isolated);
    // The child's unrestricted request must still inherit the parent's ceiling.
    let child = fixture
        .thread_manager
        .spawn_internal_session(parent.thread_id, options)
        .await?;
    let response = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("code-tools"),
                responses::ev_custom_tool_call(
                    "cell",
                    "exec",
                    r#"
let blocked = false;
try { await tools.exec_command({ cmd: "echo should-not-run" }); }
catch { blocked = true; }
const plan = await tools.update_plan({ plan: [{ step: "inspect", status: "completed" }] });
text(JSON.stringify({
  blocked, plan,
  shellCallable: typeof tools.exec_command === "function",
  shellListed: ALL_TOOLS.some(({ name }) => name === "exec_command"),
}));
"#,
                ),
                responses::ev_completed("code-tools"),
            ]),
            responses::sse(vec![responses::ev_completed("done")]),
        ],
    )
    .await;
    child
        .thread
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "Try both nested tools".to_owned(),
            text_elements: Vec::new(),
        }]))
        .await?;
    wait_for_event(&child.thread, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    let requests = response.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0].body_json()["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .map(|tool| tool["name"].as_str().expect("name"))
            .collect::<Vec<_>>(),
        vec!["exec", "wait"]
    );
    let output =
        super::super::code_mode::custom_tool_output_last_non_empty_text(&requests[1], "cell")
            .expect("Code Mode result");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&output)?,
        json!({
            "blocked": true, "plan": {}, "shellCallable": false, "shellListed": false,
        })
    );
    child.thread.shutdown_and_wait().await?;
    cancelled.cancel();
    tasks.close();
    tasks.wait().await;
    fixture.codex.shutdown_and_wait().await?;
    Ok(())
}
