use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::DynamicToolCallOutputContentItem;
use codex_app_server_protocol::DynamicToolCallResponse;
use codex_app_server_protocol::DynamicToolFunctionSpec;
use codex_app_server_protocol::DynamicToolSpec;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::json;

#[tokio::test]
async fn explicit_client_file_tools_keep_their_runtime_and_schema() -> Result<()> {
    for name in ["Read", "Grep", "Glob"] {
        let server = responses::start_mock_server().await;
        let mock = responses::mount_sse_sequence(
            &server,
            vec![
                responses::sse(vec![
                    responses::ev_function_call("client", name, r#"{"city":"Paris"}"#),
                    responses::ev_completed("first"),
                ]),
                responses::sse(vec![
                    responses::ev_assistant_message("done", "done"),
                    responses::ev_completed("second"),
                ]),
            ],
        )
        .await;
        let codex_home = tempfile::tempdir()?;
        MockResponsesConfig::new(&server.uri()).write(codex_home.path())?;
        let mut app = TestAppServer::builder()
            .with_codex_home(codex_home.path())
            .build_initialized()
            .await?;
        let schema = json!({"type":"object","properties":{"city":{"type":"string"}},"required":["city"],"additionalProperties":false});
        let function = DynamicToolFunctionSpec {
            name: name.into(),
            description: "Explicit client tool".into(),
            input_schema: schema,
            defer_loading: false,
        };
        let thread = app
            .start_thread(ThreadStartParams {
                dynamic_tools: Some(vec![DynamicToolSpec::Function(function)]),
                ..Default::default()
            })
            .await?
            .thread;
        app.send_turn_start_request(TurnStartParams {
            thread_id: thread.id,
            input: vec![UserInput::Text {
                text: "Use the client tool".into(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
        let request = app.read_stream_until_request_message().await?;
        let ServerRequest::DynamicToolCall { request_id, params } = request else {
            panic!("client request")
        };
        assert_eq!(params.tool, name);
        assert_eq!(params.arguments, json!({"city":"Paris"}));
        app.send_response(
            request_id,
            serde_json::to_value(DynamicToolCallResponse {
                content_items: vec![DynamicToolCallOutputContentItem::InputText {
                    text: "SYNTHETIC_CLIENT_RESULT".into(),
                }],
                success: true,
            })?,
        )
        .await?;
        app.read_stream_until_notification_message("turn/completed")
            .await?;
        let requests = mock.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[1].function_call_output_text("client").as_deref(),
            Some("SYNTHETIC_CLIENT_RESULT")
        );
    }
    Ok(())
}
