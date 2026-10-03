use std::collections::HashMap;

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::DynamicToolCallStatus;
use codex_app_server_protocol::ItemStartedNotification;
use codex_app_server_protocol::ThreadHistoryMode;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadReadResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnInterruptParams;
use codex_app_server_protocol::TurnInterruptResponse;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::json;

#[tokio::test]
async fn file_cards_follow_post_tool_hooks_and_restore_in_both_history_modes() -> Result<()> {
    for history in [ThreadHistoryMode::Legacy, ThreadHistoryMode::Paginated] {
        for code_mode in [false, true] {
            for (hook, expected, success) in [
                (json!({}), "SYNTHETIC_MARKER", true),
                (
                    json!({"continue":false,"stopReason":"HOOK_FEEDBACK"}),
                    "HOOK_FEEDBACK",
                    true,
                ),
                (
                    json!({"decision":"block","reason":"HOOK_REJECTED"}),
                    "HOOK_REJECTED",
                    false,
                ),
                // Native hook parser refuses block decisions without a reason.
                (json!({"decision":"block"}), "SYNTHETIC_MARKER", true),
            ] {
                let server = responses::start_mock_server().await;
                let mock = responses::mount_sse_sequence(
                    &server,
                    vec![
                        responses::sse(vec![
                            responses::ev_response_created("files-1"),
                            read_call(code_mode),
                            responses::ev_completed("files-1"),
                        ]),
                        responses::sse(vec![
                            responses::ev_assistant_message("done", "done"),
                            responses::ev_completed("files-2"),
                        ]),
                    ],
                )
                .await;
                let home = tempfile::tempdir()?;
                let mut config =
                    MockResponsesConfig::new(&server.uri()).enable_feature(Feature::CodexHooks);
                if code_mode {
                    config = config.enable_feature(Feature::CodeModeOnly);
                }
                config.write(home.path())?;
                let mut app = isolated_server(home.path()).await?;
                let cwd = arrange_hook(&app, &format!(
                    "process.stdin.resume();process.stdin.on('end',()=>console.log(JSON.stringify({hook})));"
                )).await?;
                let thread = app.start_thread(start_params(history, cwd)).await?.thread;
                app.start_turn_and_wait_for_completion(turn_params(thread.id.clone()))
                    .await?;
                let output = if code_mode {
                    mock.requests()[1]
                        .custom_tool_call_output("cell")
                        .to_string()
                } else {
                    mock.requests()[1]
                        .function_call_output_text("read")
                        .expect("Direct output")
                };
                // Native CodeMode preserves typed results for non-blocking feedback.
                let model_expected = if code_mode && expected == "HOOK_FEEDBACK" {
                    "SYNTHETIC_MARKER"
                } else {
                    expected
                };
                assert!(
                    output.contains(model_expected),
                    "Unexpected model-visible result: {output}"
                );
                app.shutdown_gracefully().await?;
                drop(app);
                let mut restarted = isolated_server(home.path()).await?;
                assert_card(&mut restarted, thread.id, expected, success).await?;
            }
        }
    }
    Ok(())
}

#[tokio::test]
async fn interrupted_file_card_is_persisted_once_before_server_restart() -> Result<()> {
    for history in [ThreadHistoryMode::Legacy, ThreadHistoryMode::Paginated] {
        for code_mode in [false, true] {
            let server = responses::start_mock_server().await;
            responses::mount_sse_once(
                &server,
                responses::sse(vec![
                    responses::ev_response_created("files-1"),
                    read_call(code_mode),
                    responses::ev_completed("files-1"),
                ]),
            )
            .await;
            let home = tempfile::tempdir()?;
            let mut config =
                MockResponsesConfig::new(&server.uri()).enable_feature(Feature::CodexHooks);
            if code_mode {
                config = config.enable_feature(Feature::CodeModeOnly);
            }
            config.write(home.path())?;
            let mut app = isolated_server(home.path()).await?;
            let cwd = arrange_hook(&app, "process.stdin.resume();process.stdin.on('end',()=>setTimeout(()=>console.log('{}'),30000));").await?;
            let thread = app.start_thread(start_params(history, cwd)).await?.thread;
            let started: TurnStartResponse = app
                .request(|request_id| ClientRequest::TurnStart {
                    request_id,
                    params: turn_params(thread.id.clone()),
                })
                .await?;
            loop {
                let item: ItemStartedNotification = app.read_notification("item/started").await?;
                if matches!(item.item, ThreadItem::DynamicToolCall { .. }) {
                    break;
                }
            }
            let _: TurnInterruptResponse = app
                .request(|request_id| ClientRequest::TurnInterrupt {
                    request_id,
                    params: TurnInterruptParams {
                        thread_id: thread.id.clone(),
                        turn_id: started.turn.id,
                    },
                })
                .await?;
            app.shutdown_gracefully().await?;
            drop(app);
            let mut restarted = isolated_server(home.path()).await?;
            assert_card(&mut restarted, thread.id, "File tool interrupted", false).await?;
        }
    }
    Ok(())
}

fn read_call(code_mode: bool) -> serde_json::Value {
    if code_mode {
        responses::ev_custom_tool_call(
            "cell",
            "exec",
            "text(await tools.Read({file_path:'sample.txt'}));",
        )
    } else {
        responses::ev_function_call("read", "Read", r#"{"file_path":"sample.txt"}"#)
    }
}

#[tokio::test]
async fn yielded_code_mode_cell_can_read_after_its_original_turn_completes() -> Result<()> {
    let server = responses::start_mock_server().await;
    let first = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("yield-1"),
                responses::ev_custom_tool_call(
                    "yielded",
                    "exec",
                    r#"
yield_control();
let ready = false;
for (let n = 0; n < 100; n++) {
    if ((await tools.Glob({pattern:'gate-ready'})).results.length) { ready = true; break; }
    await new Promise(resolve => setTimeout(resolve, 50));
}
if (!ready) throw new Error('Fixture gate timed out');
text(await tools.Read({file_path:'sample.txt'}));
"#,
                ),
                responses::ev_completed("yield-1"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("yield-done", "waiting"),
                responses::ev_completed("yield-2"),
            ]),
        ],
    )
    .await;
    let home = tempfile::tempdir()?;
    MockResponsesConfig::new(&server.uri())
        .enable_feature(Feature::CodeModeOnly)
        .write(home.path())?;
    let mut app = isolated_server(home.path()).await?;
    let cwd = super::claude_file_tools::arrange_files(&app, false).await?;
    let thread = app
        .start_thread(start_params(ThreadHistoryMode::Paginated, cwd))
        .await?
        .thread;
    app.start_turn_and_wait_for_completion(turn_params(thread.id.clone()))
        .await?;
    let output = first.requests()[1].custom_tool_call_output("yielded");
    let header = match &output["output"] {
        serde_json::Value::String(text) => text.as_str(),
        serde_json::Value::Array(items) => items[0]["text"].as_str().expect("running cell header"),
        _ => panic!("running cell output"),
    };
    let cell_id = header
        .strip_prefix("Script running with cell ID ")
        .and_then(|text| text.lines().next())
        .expect("cell ID");
    let second = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("resume-1"),
                responses::ev_function_call(
                    "wait-file",
                    "wait",
                    &json!({
                        "cell_id": cell_id, "yield_time_ms": 10000,
                    })
                    .to_string(),
                ),
                responses::ev_completed("resume-1"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("resume-done", "done"),
                responses::ev_completed("resume-2"),
            ]),
        ],
    )
    .await;
    // Release through the executor filesystem after the original turn completes.
    // A client ServerRequest is unsuitable: app-server cancels those at TurnComplete.
    let environment = app.auto_env()?;
    environment
        .environment()
        .get_filesystem()
        .write_file(
            &environment.selection().cwd.join("gate-ready")?,
            Vec::new(),
            Default::default(),
            None,
        )
        .await?;
    app.start_turn_and_wait_for_completion(turn_params(thread.id.clone()))
        .await?;
    let output = second.requests()[1]
        .function_call_output("wait-file")
        .to_string();
    assert!(
        output.contains("SYNTHETIC_MARKER"),
        "Unexpected resumed cell output: {output}"
    );
    app.shutdown_gracefully().await?;
    drop(app);
    let mut restarted = isolated_server(home.path()).await?;
    assert_card(&mut restarted, thread.id, "SYNTHETIC_MARKER", true).await?;
    Ok(())
}

fn start_params(history: ThreadHistoryMode, cwd: String) -> ThreadStartParams {
    ThreadStartParams {
        history_mode: Some(history),
        config: Some(HashMap::from([
            (
                "projects".into(),
                json!({(cwd.clone()):{"trust_level":"trusted"}}),
            ),
            ("bypass_hook_trust".into(), json!(true)),
        ])),
        cwd: Some(cwd),
        ..Default::default()
    }
}

fn turn_params(thread_id: String) -> TurnStartParams {
    TurnStartParams {
        thread_id,
        input: vec![UserInput::Text {
            text: "Read the synthetic file".into(),
            text_elements: Vec::new(),
        }],
        ..Default::default()
    }
}

async fn arrange_hook(app: &TestAppServer, script: &str) -> Result<String> {
    let cwd = super::claude_file_tools::arrange_files(app, false).await?;
    let environment = app.auto_env()?;
    let root = &environment.selection().cwd;
    let fs = environment.environment().get_filesystem();
    fs.create_directory(
        &root.join(".claude")?,
        codex_exec_server::CreateDirectoryOptions {
            recursive: true,
            follow_symlinks: true,
        },
        None,
    )
    .await?;
    fs.write_file(
        &root.join("post-hook.cjs")?,
        script.as_bytes().to_vec(),
        Default::default(),
        None,
    )
    .await?;
    let settings = json!({"hooks":{"PostToolUse":[{"matcher":"^Read$","hooks":[{
        "type":"command","command":"node post-hook.cjs","timeout":40,
    }]}]}});
    fs.write_file(
        &root.join(".claude/settings.json")?,
        settings.to_string().into_bytes(),
        Default::default(),
        None,
    )
    .await?;
    Ok(cwd)
}

async fn assert_card(
    app: &mut TestAppServer,
    thread_id: String,
    expected: &str,
    success: bool,
) -> Result<()> {
    let id = app
        .send_thread_read_request(ThreadReadParams {
            thread_id,
            include_turns: true,
        })
        .await?;
    let read: ThreadReadResponse = app.read_response(id).await?;
    let cards: Vec<_> = read
        .thread
        .turns
        .into_iter()
        .flat_map(|turn| turn.items)
        .filter_map(|item| match item {
            ThreadItem::DynamicToolCall {
                tool,
                status,
                success,
                content_items,
                ..
            } if tool == "Read" => Some((tool, status, success, content_items)),
            _ => None,
        })
        .collect();
    assert_eq!(cards.len(), 1, "One native card must survive restart");
    assert_eq!(
        (&cards[0].0, &cards[0].1, cards[0].2),
        (
            &"Read".to_owned(),
            &if success {
                DynamicToolCallStatus::Completed
            } else {
                DynamicToolCallStatus::Failed
            },
            Some(success)
        )
    );
    let output = serde_json::to_value(&cards[0].3)?;
    let text = output[0]["text"].as_str().expect("Native file result text");
    assert!(
        text.contains(expected),
        "Unexpected persisted result: {text}"
    );
    if expected != "SYNTHETIC_MARKER" {
        assert!(!text.contains("SYNTHETIC_MARKER"));
    }
    Ok(())
}

async fn isolated_server(home: &std::path::Path) -> Result<TestAppServer> {
    // CLAUDE compatibility uses USERPROFILE/HOME independently of CODEX_HOME.
    // Override only the child process; never run the developer's global hooks or MCP.
    let fixture_home = home.to_string_lossy();
    TestAppServer::builder()
        .with_codex_home(home)
        .with_env_overrides(&[
            ("USERPROFILE", Some(&fixture_home)),
            ("HOME", Some(&fixture_home)),
        ])
        .build_initialized()
        .await
}
