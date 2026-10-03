use anyhow::Context;
use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::DynamicToolCallStatus;
use codex_app_server_protocol::ItemCompletedNotification;
use codex_app_server_protocol::ThreadHistoryMode;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadReadResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;

#[tokio::test]
async fn native_claude_file_tools_execute_and_respect_project_denials() -> Result<()> {
    for history_mode in [ThreadHistoryMode::Legacy, ThreadHistoryMode::Paginated] {
        for denied in [false, true] {
            let server = responses::start_mock_server().await;
            let mock = responses::mount_sse_sequence(
            &server,
            vec![
                responses::sse(vec![
                    responses::ev_response_created("files-1"),
                    responses::ev_function_call("read", "Read", r#"{"file_path":"sample.txt"}"#),
                    responses::ev_function_call("glob", "Glob", r#"{"pattern":"*.txt"}"#),
                    responses::ev_function_call(
                        "grep",
                        "Grep",
                        r#"{"pattern":"SYNTHETIC_MARKER","glob":"*.txt","output_mode":"content"}"#,
                    ),
                    responses::ev_completed("files-1"),
                ]),
                responses::sse(vec![
                    responses::ev_assistant_message("done", "done"),
                    responses::ev_completed("files-2"),
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
            let cwd = arrange_files(&app, denied).await?;
            let thread = app
                .start_thread(ThreadStartParams {
                    history_mode: Some(history_mode),
                    config: Some(HashMap::from([(
                        "projects".into(),
                        json!({ (cwd.clone()): {"trust_level":"trusted"} }),
                    )])),
                    cwd: Some(cwd),
                    ..Default::default()
                })
                .await?
                .thread;
            app.start_turn_and_wait_for_completion(TurnStartParams {
                thread_id: thread.id.clone(),
                input: vec![UserInput::Text {
                    text: "Use the three file tools".into(),
                    text_elements: Vec::new(),
                }],
                ..Default::default()
            })
            .await?;
            let expected = if denied {
                Vec::new()
            } else {
                vec!["Read", "Glob", "Grep"]
            };
            let mut live_items = Vec::new();
            while app
                .pending_notification_methods()
                .iter()
                .any(|method| method == "item/completed")
            {
                let completed: ItemCompletedNotification =
                    app.read_notification("item/completed").await?;
                live_items.push(completed.item);
            }
            assert_file_cards(&live_items, &expected);
            let requests = mock.requests();
            assert_eq!(requests.len(), 2);
            for call_id in ["read", "glob", "grep"] {
                let output = requests[1]
                    .function_call_output_text(call_id)
                    .expect("tool result");
                if denied {
                    assert!(
                        output.contains("blocked by Claude deny"),
                        "{call_id}: {output}"
                    );
                    assert!(!output.contains("SYNTHETIC_MARKER"));
                } else {
                    let value: Value = serde_json::from_str(&output)
                        .with_context(|| format!("{call_id} result: {output}"))?;
                    assert_eq!(value["truncated"], false);
                    match call_id {
                        "read" => assert_eq!(
                            value["lines"],
                            json!([{"line":1,"text":"SYNTHETIC_MARKER"}])
                        ),
                        "glob" => assert_eq!(value["results"], json!(["sample.txt"])),
                        _ => assert_eq!(
                            value["results"],
                            json!([{"path":"sample.txt","line":1,"text":"SYNTHETIC_MARKER"}])
                        ),
                    }
                }
            }
            app.shutdown_gracefully().await?;
            drop(app);
            let mut restarted = TestAppServer::builder()
                .with_codex_home(codex_home.path())
                .build_initialized()
                .await?;
            let read_id = restarted
                .send_thread_read_request(ThreadReadParams {
                    thread_id: thread.id,
                    include_turns: true,
                })
                .await?;
            let response: ThreadReadResponse = restarted.read_response(read_id).await?;
            let items: Vec<_> = response
                .thread
                .turns
                .into_iter()
                .flat_map(|turn| turn.items)
                .collect();
            assert_file_cards(&items, &expected);
        }
    }
    Ok(())
}

fn assert_file_cards(items: &[ThreadItem], expected: &[&str]) {
    let mut tools = Vec::new();
    for item in items {
        if let ThreadItem::DynamicToolCall {
            tool,
            status,
            success,
            content_items,
            ..
        } = item
        {
            assert_eq!(status, &DynamicToolCallStatus::Completed);
            assert_eq!(*success, Some(true));
            assert!(content_items.is_some());
            tools.push(tool.as_str());
        }
    }
    tools.sort_unstable();
    let mut expected = expected.to_vec();
    expected.sort_unstable();
    assert_eq!(
        tools, expected,
        "Completed cards must be unique and survive persisted history"
    );
}

#[tokio::test]
async fn code_mode_claude_file_tools_respect_the_same_permission_gate() -> Result<()> {
    for denied in [false, true] {
        let server = responses::start_mock_server().await;
        let mock = responses::mount_sse_sequence(
            &server,
            vec![
                responses::sse(vec![
                    responses::ev_response_created("files-1"),
                    responses::ev_custom_tool_call(
                        "cell",
                        "exec",
                        "text(await tools.Read({file_path: 'sample.txt'}));",
                    ),
                    responses::ev_completed("files-1"),
                ]),
                responses::sse(vec![
                    responses::ev_assistant_message("done", "done"),
                    responses::ev_completed("files-2"),
                ]),
            ],
        )
        .await;
        let codex_home = tempfile::tempdir()?;
        MockResponsesConfig::new(&server.uri())
            .enable_feature(Feature::CodeModeOnly)
            .write(codex_home.path())?;
        let mut app = TestAppServer::builder()
            .with_codex_home(codex_home.path())
            .build_initialized()
            .await?;
        let cwd = arrange_files(&app, denied).await?;
        let thread = app
            .start_thread(ThreadStartParams {
                config: Some(HashMap::from([(
                    "projects".into(),
                    json!({ (cwd.clone()): {"trust_level":"trusted"} }),
                )])),
                cwd: Some(cwd),
                ..Default::default()
            })
            .await?
            .thread;
        app.start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.id,
            input: vec![UserInput::Text {
                text: "Read using code mode".into(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
        let requests = mock.requests();
        assert_eq!(requests.len(), 2);
        let result = requests[1].custom_tool_call_output("cell");
        let output = result["output"]
            .as_array()
            .context("code-mode content blocks")?
            .iter()
            .filter_map(|item| item["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if denied {
            assert!(output.contains("blocked by Claude deny"), "{output}");
            assert!(!output.contains("SYNTHETIC_MARKER"));
        } else {
            assert!(output.contains("SYNTHETIC_MARKER"), "{output}");
        }
    }
    Ok(())
}

async fn arrange_files(app: &TestAppServer, denied: bool) -> Result<String> {
    let environment = app.auto_env()?;
    let root = &environment.selection().cwd;
    let file_system = environment.environment().get_filesystem();
    file_system
        .write_file(
            &root.join("sample.txt")?,
            b"SYNTHETIC_MARKER\n".to_vec(),
            Default::default(),
            None,
        )
        .await?;
    if denied {
        file_system
            .create_directory(
                &root.join(".claude")?,
                codex_exec_server::CreateDirectoryOptions {
                    recursive: true,
                    follow_symlinks: true,
                },
                None,
            )
            .await?;
        file_system
            .write_file(
                &root.join(".claude/settings.json")?,
                br#"{"permissions":{"deny":["Read"]}}"#.to_vec(),
                Default::default(),
                None,
            )
            .await?;
    }
    Ok(environment.cwd().to_string_lossy().into_owned())
}
