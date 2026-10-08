use super::*;

#[tokio::test]
async fn claudex_workflow_prefix_refreshes_worktree_availability_without_reopening_dismissed_popup()
{
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual(None).await;
    chat.set_feature_enabled(Feature::Worktrees, true);
    chat.set_local_worktree_operations(false);
    chat.bottom_pane
        .set_composer_text("/work".into(), Vec::new(), Vec::new());
    let remote = render_bottom_popup(&chat, 100);
    assert!(remote.contains("/workflows"));
    assert!(!remote.contains("/worktree"));
    chat.set_local_worktree_operations(true);
    let local = render_bottom_popup(&chat, 100);
    assert!(local.contains("/workflows"));
    assert!(local.contains("/worktree"));
    chat.set_local_worktree_operations(false);
    assert!(!render_bottom_popup(&chat, 100).contains("/worktree"));
    chat.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    chat.set_local_worktree_operations(true);
    assert!(!render_bottom_popup(&chat, 100).contains("/workflows"));
    assert_eq!(chat.composer_text_with_pending(), "/work");
}

#[tokio::test]
async fn claudex_agent_catalogue_works_without_shared_daemon() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(None).await;
    chat.config.agent_roles.clear();
    chat.config.agent_roles.insert(
        "researcher".to_string(),
        crate::legacy_core::config::AgentRoleConfig {
            description: Some("Inspect the repository with evidence.".to_string()),
            config_file: None,
            nickname_candidates: None,
        },
    );
    chat.dispatch_command(SlashCommand::Agents);
    let rendered = render_bottom_popup(&chat, 80);
    assert!(rendered.contains("researcher"));
    assert!(
        !std::iter::from_fn(|| rx.try_recv().ok())
            .any(|event| matches!(event, AppEvent::OpenAgentsOverview))
    );
    insta::assert_snapshot!("claudex_agent_catalogue", rendered);
}

#[tokio::test]
async fn claudex_tasks_routes_to_session_picker_while_busy() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(None).await;
    chat.bottom_pane.set_task_running(true);
    chat.dispatch_command(SlashCommand::Tasks);
    assert!(
        std::iter::from_fn(|| rx.try_recv().ok())
            .any(|event| matches!(event, AppEvent::OpenAgentPicker))
    );
}

#[tokio::test]
async fn claudex_help_is_searchable_and_preserves_codex_commands() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual(None).await;
    chat.dispatch_command(SlashCommand::Help);
    let rendered = render_bottom_popup(&chat, 90);
    assert!(rendered.contains("Claudex help"));
    chat.handle_key_event(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
    chat.handle_key_event(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
    chat.handle_key_event(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    chat.handle_key_event(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
    let filtered = render_bottom_popup(&chat, 90);
    assert!(filtered.contains("/tasks"));
    insta::assert_snapshot!("claudex_help_filtered", filtered);
}

fn workflow_fixture() -> WorkflowRun {
    serde_json::from_value(serde_json::json!({
        "runId": "fixture-run", "status": "running", "startedAt": 1, "phase": "Research",
        "agents": [{"label": "Researcher", "status": "running"}, {"label": "Writer", "status": "pending"}]
    })).unwrap()
}

#[tokio::test]
async fn claudex_workflows_show_live_run_and_controls() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual(None).await;
    chat.open_claudex_workflows(None);
    chat.apply_claudex_workflows(
        chat.claudex_workflow_generation,
        None,
        Ok(vec![workflow_fixture()]),
    );
    insta::assert_snapshot!("claudex_workflow_runs", render_bottom_popup(&chat, 90));
    chat.open_claudex_workflows(Some("fixture-run".to_string()));
    chat.apply_claudex_workflows(
        chat.claudex_workflow_generation,
        Some("fixture-run".to_string()),
        Ok(vec![workflow_fixture()]),
    );
    let rendered = render_bottom_popup(&chat, 90);
    assert!(rendered.contains("Pause after current agent"));
    assert!(rendered.contains("Researcher"));
    insta::assert_snapshot!("claudex_workflow_detail", rendered);
}

#[tokio::test]
async fn claudex_workflow_refresh_cannot_reopen_closed_panel() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual(None).await;
    chat.open_claudex_workflows(None);
    chat.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    chat.apply_claudex_workflows(
        chat.claudex_workflow_generation,
        None,
        Ok(vec![workflow_fixture()]),
    );
    assert!(!render_bottom_popup(&chat, 90).contains("fixture-run"));
}

#[tokio::test]
async fn claudex_workflow_agents_remain_reachable_beyond_visible_rows() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual(None).await;
    let run = serde_json::from_value(serde_json::json!({
        "runId": "many-agents", "status": "running", "startedAt": 1,
        "agents": (0..15).map(|index| serde_json::json!({ "label": format!("Agent {index}"), "status": "pending" })).collect::<Vec<_>>()
    })).unwrap();
    chat.open_claudex_workflows(Some("many-agents".to_string()));
    chat.apply_claudex_workflows(
        chat.claudex_workflow_generation,
        Some("many-agents".to_string()),
        Ok(vec![run]),
    );
    for _ in 0..18 {
        chat.handle_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    }
    assert!(render_bottom_popup(&chat, 90).contains("Agent 14"));
}

#[tokio::test]
async fn claudex_workflow_arrows_inspect_selected_run_and_return_without_control() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(None).await;
    chat.open_claudex_workflows(None);
    chat.apply_claudex_workflows(
        chat.claudex_workflow_generation,
        None,
        Ok(vec![workflow_fixture()]),
    );
    chat.handle_key_event(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert!(std::iter::from_fn(|| rx.try_recv().ok()).any(|event| {
        matches!(event, AppEvent::OpenClaudexWorkflow(Some(id)) if id == "fixture-run")
    }));

    chat.open_claudex_workflows(Some("fixture-run".to_string()));
    chat.apply_claudex_workflows(
        chat.claudex_workflow_generation,
        Some("fixture-run".to_string()),
        Ok(vec![workflow_fixture()]),
    );
    chat.handle_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    chat.handle_key_event(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert!(
        !std::iter::from_fn(|| rx.try_recv().ok())
            .any(|event| { matches!(event, AppEvent::ControlClaudexWorkflow { .. }) })
    );
    chat.handle_key_event(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    assert!(
        std::iter::from_fn(|| rx.try_recv().ok())
            .any(|event| { matches!(event, AppEvent::OpenClaudexWorkflow(None)) })
    );
}

#[tokio::test]
async fn claudex_workflow_agent_arrow_uses_exact_thread_and_refresh_preserves_selection() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(None).await;
    let id = ThreadId::new();
    let run = |prepend: bool| {
        serde_json::from_value(serde_json::json!({
        "runId": "fixture-run", "status": "running", "startedAt": 1,
        "agents": if prepend {
            vec![serde_json::json!({"label": "Earlier", "status": "pending"}),
                 serde_json::json!({"label": "Researcher", "status": "running", "threadId": id.to_string()})]
        } else {
            vec![serde_json::json!({"label": "Researcher", "status": "running", "threadId": id.to_string()})]
        }
    })).unwrap()
    };
    chat.open_claudex_workflows(Some("fixture-run".to_string()));
    chat.apply_claudex_workflows(
        chat.claudex_workflow_generation,
        Some("fixture-run".to_string()),
        Ok(vec![run(false)]),
    );
    for _ in 0..4 {
        chat.handle_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    }
    chat.apply_claudex_workflows(
        chat.claudex_workflow_generation,
        Some("fixture-run".to_string()),
        Ok(vec![run(true)]),
    );
    assert_eq!(
        chat.bottom_pane
            .selected_index_for_present_view("claudex-workflows"),
        Some(5)
    );
    chat.handle_key_event(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert!(
        std::iter::from_fn(|| rx.try_recv().ok()).any(|event| {
            matches!(event, AppEvent::SelectAgentThread(selected) if selected == id)
        })
    );
}

#[tokio::test]
async fn claudex_workflow_arrow_does_not_open_invalid_thread_or_reopen_closed_panel() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(None).await;
    let run = || {
        serde_json::from_value(serde_json::json!({
            "runId": "fixture-run", "status": "running", "startedAt": 1,
            "agents": [{"label": "Researcher", "status": "running", "threadId": "not-a-thread"}]
        }))
        .unwrap()
    };
    chat.open_claudex_workflows(Some("fixture-run".to_string()));
    chat.apply_claudex_workflows(
        chat.claudex_workflow_generation,
        Some("fixture-run".to_string()),
        Ok(vec![run()]),
    );
    for _ in 0..4 {
        chat.handle_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    }
    chat.handle_key_event(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert!(
        !std::iter::from_fn(|| rx.try_recv().ok())
            .any(|event| matches!(event, AppEvent::SelectAgentThread(_)))
    );
    chat.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    chat.apply_claudex_workflows(
        chat.claudex_workflow_generation,
        Some("fixture-run".to_string()),
        Ok(vec![run()]),
    );
    assert!(!chat.bottom_pane.has_active_view());
}

#[tokio::test]
async fn claudex_workflow_root_refresh_keeps_selected_run_after_reorder() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual(None).await;
    let mut other = workflow_fixture();
    other.run_id = "other-run".to_string();
    chat.open_claudex_workflows(None);
    chat.apply_claudex_workflows(
        chat.claudex_workflow_generation,
        None,
        Ok(vec![workflow_fixture(), other.clone()]),
    );
    chat.handle_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    chat.apply_claudex_workflows(
        chat.claudex_workflow_generation,
        None,
        Ok(vec![other, workflow_fixture()]),
    );
    assert_eq!(
        chat.bottom_pane
            .selected_index_for_present_view("claudex-workflows"),
        Some(0)
    );
}

#[tokio::test]
async fn claudex_agent_role_panel_opens_native_thread_picker() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(None).await;
    chat.dispatch_command(SlashCommand::Agents);
    chat.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        std::iter::from_fn(|| rx.try_recv().ok())
            .any(|event| matches!(event, AppEvent::OpenAgentPicker))
    );
}

#[tokio::test]
async fn claudex_agent_picker_arrows_survive_refresh_and_do_not_reopen_after_close() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(None).await;
    let id = ThreadId::new();
    let params = || SelectionViewParams {
        view_id: Some("agent-picker"),
        items: vec![SelectionItem {
            name: "worker".to_string(),
            actions: vec![Box::new(move |tx| tx.send(AppEvent::SelectAgentThread(id)))],
            dismiss_on_select: true,
            ..Default::default()
        }],
        ..SelectionViewParams::picker()
    };
    chat.show_claudex_agent_picker_navigation(params());
    assert!(chat.replace_claudex_agent_picker_navigation(params()));
    chat.handle_key_event(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert!(
        std::iter::from_fn(|| rx.try_recv().ok())
            .any(|event| matches!(event, AppEvent::SelectAgentThread(selected) if selected == id))
    );
    assert!(!chat.replace_claudex_agent_picker_navigation(params()));
    assert!(!chat.bottom_pane.has_active_view());
    chat.show_claudex_agent_picker_navigation(params());
    chat.handle_key_event(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    assert!(!chat.bottom_pane.has_active_view());
}
