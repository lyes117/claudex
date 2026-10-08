//! Exercise the actual queue plus typed RPC against a server that never replies.
use super::*;
use crate::render::renderable::Renderable;
use codex_app_server_protocol::ClaudeCommandMetadata;
use codex_app_server_protocol::JSONRPCMessage;
use futures::SinkExt;
use futures::StreamExt;
use pretty_assertions::assert_eq;
use tokio_tungstenite::tungstenite::Message;

#[tokio::test]
async fn claude_command_deadline_covers_queue_and_rpc_and_preserves_draft_caret() -> Result<()> {
    let (mut app, mut events, mut operations) = make_test_app_with_channels().await;
    let id = ThreadId::new();
    app.active_thread_id = Some(id);
    app.chat_widget
        .handle_thread_session(test_thread_session(id, app.config.cwd.to_path_buf()));
    let path = app.config.cwd.join(".claude/commands/deadline.md");
    let skills = vec![codex_app_server_protocol::SkillMetadata {
        name: "deadline".into(),
        description: "Synthetic command".into(),
        short_description: None,
        interface: None,
        dependencies: None,
        scope: crate::test_support::skill_scope_repo(),
        enabled: true,
        plugin_id: None,
        path,
        claude_command: Some(ClaudeCommandMetadata {
            user_invocable: true,
            argument_hint: None,
        }),
    }];
    app.chat_widget
        .set_skills_from_response(&codex_app_server_protocol::SkillsListResponse {
            data: vec![codex_app_server_protocol::SkillsListEntry {
                cwd: app.config.cwd.to_path_buf(),
                skills,
                errors: Vec::new(),
            }],
        });
    app.chat_widget
        .apply_external_edit("/deadline \u{00e9}quipe \u{7814}\u{7a76}".into());
    app.chat_widget
        .handle_key_event(KeyEvent::from(KeyCode::Left));
    let area = ratatui::layout::Rect::new(0, 0, 100, 24);
    let before = (
        app.chat_widget.composer_text_with_pending(),
        app.chat_widget.as_renderable().cursor_pos(area),
    );
    assert!(before.1.is_some(), "interior Unicode caret is visible");
    app.chat_widget
        .handle_key_event(KeyEvent::from(KeyCode::Enter));
    let request = std::iter::from_fn(|| events.try_recv().ok())
        .find_map(|event| match event {
            AppEvent::ExpandClaudeCommand { request, .. } => Some(request),
            _ => None,
        })
        .expect("actual composer preparation");
    let request_id = request.id;
    while operations.try_recv().is_ok() {}
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = crate::resolve_remote_addr(&format!("ws://{}", listener.local_addr()?))?;
    let (seen_tx, mut seen_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await?;
        let mut socket = tokio_tungstenite::accept_async(stream).await?;
        while let Some(frame) = socket.next().await {
            let Message::Text(text) = frame? else {
                continue;
            };
            let JSONRPCMessage::Request(request) = serde_json::from_str(&text)? else {
                continue;
            };
            if request.method == "initialize" {
                socket.send(Message::Text(serde_json::json!({"id":request.id,"result":{"userAgent":"deadline-fixture"}}).to_string().into())).await?;
            } else {
                assert_eq!(request.method, "skills/claudeCommand/expand");
                let _ = seen_tx.send(());
                std::future::pending::<()>().await;
                break;
            }
        }
        Ok::<_, color_eyre::Report>(())
    });
    let session = AppServerSession::new(
        crate::connect_remote_app_server(endpoint).await?,
        crate::app_server_session::ThreadParamsMode::Remote,
    );
    let queued = crate::app::claude_commands::CLAUDE_PREPARATION.lock().await;
    tokio::time::pause();
    let started = tokio::time::Instant::now();
    app.expand_claude_command(&session, Some(id), app.config.cwd.clone(), request);
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(/*secs*/ 20)).await;
    assert!(seen_rx.try_recv().is_err(), "RPC cannot start while queued");
    assert!(
        events.try_recv().is_err(),
        "queue still has ten seconds in its total deadline"
    );
    drop(queued);
    let mut rpc_seen = false;
    for _ in 0..1000 {
        tokio::task::yield_now().await;
        if seen_rx.try_recv().is_ok() {
            rpc_seen = true;
            break;
        }
    }
    assert!(rpc_seen, "real typed RPC reached nonreplying server");
    tokio::time::advance(Duration::from_secs(/*secs*/ 9)).await;
    assert!(events.try_recv().is_err(), "not before the total deadline");
    tokio::time::advance(Duration::from_secs(/*secs*/ 1)).await;
    // Tokio timers have millisecond resolution; let the deadline's timer tick wake its task.
    tokio::time::advance(Duration::from_millis(/*millis*/ 1)).await;
    let AppEvent::ClaudeCommandExpanded {
        id: completed_id,
        result,
        ..
    } = tokio::time::timeout_at(
        started + Duration::from_millis(/*millis*/ 30_002),
        events.recv(),
    )
    .await
    .expect("deadline task completes when its timer wakes")
    .expect("deadline response")
    else {
        panic!("expected command response");
    };
    assert_eq!(completed_id, request_id);
    assert_eq!(
        tokio::time::Instant::now() - started,
        Duration::from_millis(/*millis*/ 30_001)
    );
    assert_eq!(result, Err("Could not resolve Claude command".to_string()));
    app.chat_widget
        .on_claude_command_expanded(completed_id, result);
    assert_eq!(
        (
            app.chat_widget.composer_text_with_pending(),
            app.chat_widget.as_renderable().cursor_pos(area)
        ),
        before
    );
    assert!(
        operations.try_recv().is_err(),
        "timeout dispatches no user turn"
    );
    server.abort();
    Ok(())
}
