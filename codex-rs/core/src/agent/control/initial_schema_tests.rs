//! The first native child request must carry the schema for both input paths.
use super::*;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::json;
use test_case::test_case;

#[tokio::test]
#[test_case(false, false; "user_input_without_schema")]
#[test_case(false, true; "user_input_with_schema")]
#[test_case(true, false; "agent_message_without_schema")]
#[test_case(true, true; "agent_message_with_schema")]
async fn initial_child_request_receives_output_schema(
    message_input: bool,
    structured: bool,
) -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    let response = responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_response_created("child-schema"),
            responses::ev_assistant_message("child-result", r#"{"answer":"ok"}"#),
            responses::ev_completed("child-schema"),
        ]),
    )
    .await;
    let (home, mut config) = test_config().await;
    config.model_provider.base_url = Some(format!("{}/v1", server.uri()));
    config.model_provider.request_max_retries = Some(0);
    config.model_provider.stream_max_retries = Some(0);
    config
        .features
        .enable(Feature::Collab)
        .expect("enable collaboration");
    config
        .features
        .enable(Feature::MultiAgentV2)
        .expect("enable V2");
    config
        .features
        .disable(Feature::CodeMode)
        .expect("direct requests");
    let harness = AgentControlHarness::new_with_config(home, config).await;
    let parent = harness
        .manager
        .start_thread(StartThreadOptions {
            environments: Some(Vec::new()),
            ..StartThreadOptions::new(harness.config.clone())
        })
        .await?;
    let schema = json!({
        "type": "object", "properties": {"answer": {"type": "string"}},
        "required": ["answer"], "additionalProperties": false,
    });
    let expected_schema = structured.then_some(schema);
    let input = if message_input {
        AgentInput::Message {
            message: AgentMessage::Plaintext("Produce the result".into()),
            mode: MessageDeliveryMode::TriggerTurn,
        }
    } else {
        AgentInput::UserInput(text_input("Produce the result"))
    };
    let (agent, _) = parent
        .thread
        .session
        .services
        .agent_control
        .spawn(SpawnRequest {
            caller: parent.thread_id,
            config: harness.config.clone(),
            input,
            source: SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
                parent_thread_id: parent.thread_id,
                depth: 1,
                agent_path: Some(
                    AgentPath::try_from("/root/schema_worker").expect("valid fixture path"),
                ),
                agent_nickname: None,
                agent_role: None,
            }),
            options: SpawnAgentOptions {
                parent_thread_id: Some(parent.thread_id),
                environments: Some(TurnEnvironmentSnapshot::default()),
                final_output_json_schema: expected_schema.clone(),
                ..Default::default()
            },
        })
        .await?;
    let child = harness.manager.get_thread(agent.thread_id).await?;
    tokio::time::timeout(Duration::from_secs(/*secs*/ 10), async {
        loop {
            let event = child.next_event().await.expect("child event");
            if matches!(event.msg, EventMsg::TurnComplete(_)) {
                break;
            }
            if let EventMsg::Error(error) = event.msg {
                panic!("child failed: {error:?}");
            }
        }
    })
    .await?;
    let requests = response.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].header("thread-id"),
        Some(agent.thread_id.to_string())
    );
    let body = requests[0].body_json();
    if let Some(schema) = expected_schema {
        assert_eq!(
            body["text"]["format"],
            json!({
                "type": "json_schema", "name": "codex_output_schema",
                "strict": true, "schema": schema,
            })
        );
    } else {
        assert!(body["text"]["format"].is_null());
    }
    child.shutdown_and_wait().await?;
    parent.thread.shutdown_and_wait().await?;
    Ok(())
}
