//! Native workflow RPC coverage: real script/host, no model or external effects.
#![cfg(windows)]
use anyhow::Result;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ItemCompletedNotification;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnStartedNotification;
use codex_app_server_protocol::WorkflowStartResponse;
use codex_core::config::set_project_trust_level;
use codex_protocol::config_types::TrustLevel;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::time::Duration;
use wiremock::MockServer;

#[tokio::test]
async fn native_workflow_start_executes_real_host_without_model_dispatch() -> Result<()> {
    let home = tempfile::tempdir()?;
    let cwd = tempfile::tempdir()?;
    let model_server = MockServer::start().await;
    std::fs::write(
        home.path().join("config.toml"),
        format!(
            "model = \"gpt-6.1-sol\"\nopenai_base_url = {:?}\napproval_policy = \"never\"\nsandbox_mode = \"read-only\"\n",
            format!("{}/v1", model_server.uri()),
        ),
    )?;
    std::fs::create_dir(cwd.path().join(".git"))?;
    set_project_trust_level(home.path(), cwd.path(), TrustLevel::Trusted)?;
    let script = cwd.path().join("native-workflow.js");
    std::fs::write(
        &script,
        "phase('Native fixture'); log('local-only'); return {ok:args.ok};",
    )?;
    let fixture_home = home.path().to_string_lossy();
    let mut app = TestAppServer::builder()
        .with_codex_home(home.path())
        .without_managed_config()
        .with_env_overrides(&[
            ("USERPROFILE", Some(&fixture_home)),
            ("HOME", Some(&fixture_home)),
        ])
        .build_initialized_with_timeout(Duration::from_secs(30))
        .await?;
    let mut environment = app.auto_env_params()?;
    environment.cwd = codex_utils_path_uri::LegacyAppPathString::from_path(cwd.path());
    environment.runtime_workspace_roots = Some(vec![environment.cwd.clone()]);
    let start_id = app
        .send_thread_start_request(ThreadStartParams {
            cwd: Some(cwd.path().to_string_lossy().into_owned()),
            environments: Some(vec![environment]),
            ..Default::default()
        })
        .await?;
    let started_thread: ThreadStartResponse = app.read_response(start_id).await?;
    let thread = started_thread.thread;
    let id = app.send_request("workflow/start", Some(json!({
        "threadId":thread.id, "scriptPath":script, "args":{"ok":true}, "runId":"native-rpc-fixture"
    }))).await?;
    let accepted: WorkflowStartResponse = app.read_response(id).await?;
    assert_eq!(accepted.run_id, "native-rpc-fixture");
    assert!(!accepted.submission_id.is_empty());
    let started: TurnStartedNotification = app.read_notification("turn/started").await?;
    assert_eq!(started.thread_id, thread.id);
    let completed: ItemCompletedNotification = app.read_notification("item/completed").await?;
    let ThreadItem::AgentMessage { text, .. } = completed.item else {
        anyhow::bail!("native workflow final output must be an agent message");
    };
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&text)?,
        json!({
            "runId":"native-rpc-fixture", "status":"completed", "result":{"ok":true}
        })
    );
    let finished: TurnCompletedNotification = app.read_notification("turn/completed").await?;
    assert_eq!(finished.thread_id, thread.id);
    let status = home
        .path()
        .join(".claudex/workflow-runs/native-rpc-fixture/status.json");
    let value = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if let Ok(bytes) = tokio::fs::read(&status).await
                && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes)
            {
                if value["status"] == "completed" {
                    break value;
                }
                assert_ne!(value["status"], "failed", "native workflow fixture failed");
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await?;
    assert_eq!(value["runId"], "native-rpc-fixture");
    assert_eq!(value["agents"], json!([]));
    // Startup may attempt a local WebSocket upgrade, but never dispatch inference.
    assert!(
        model_server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|request| request.method.as_str() == "GET")
    );
    app.shutdown_gracefully().await?;
    Ok(())
}

#[tokio::test]
async fn native_workflow_rpc_rejects_excessive_or_invalid_requests_before_submission() -> Result<()>
{
    let home = tempfile::tempdir()?;
    let fixture_home = home.path().to_string_lossy();
    let mut app = TestAppServer::builder()
        .with_codex_home(home.path())
        .without_managed_config()
        .with_env_overrides(&[
            ("USERPROFILE", Some(&fixture_home)),
            ("HOME", Some(&fixture_home)),
        ])
        .build_initialized_with_timeout(Duration::from_secs(30))
        .await?;
    for (run_id, args, script_path) in [
        ("../escape".to_string(), json!({}), "fixture.js"),
        ("x".repeat(129), json!({}), "fixture.js"),
        ("invalid-args".to_string(), json!([]), "fixture.js"),
        ("invalid-path".to_string(), json!({}), ""),
        (
            "excessive-args".to_string(),
            json!({"data":"x".repeat(65536)}),
            "fixture.js",
        ),
    ] {
        let id = app
            .send_request(
                "workflow/start",
                Some(json!({
                    "threadId":"unloaded", "scriptPath":script_path, "args":args, "runId":run_id
                })),
            )
            .await?;
        let error = app
            .read_stream_until_error_message(RequestId::Integer(id))
            .await?;
        assert_eq!(
            error.error.message,
            "Invalid or excessive native workflow request"
        );
    }
    assert!(!home.path().join(".claudex/workflow-runs").exists());
    app.shutdown_gracefully().await?;
    Ok(())
}
