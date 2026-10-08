use super::*;
use codex_app_server_protocol::TurnPlanStep;
use codex_app_server_protocol::TurnPlanStepStatus;
use codex_app_server_protocol::TurnPlanUpdatedNotification;
use pretty_assertions::assert_eq;

fn notification(thread_id: ThreadId, status: TurnPlanStepStatus) -> ServerNotification {
    ServerNotification::TurnPlanUpdated(TurnPlanUpdatedNotification {
        thread_id: thread_id.to_string(),
        turn_id: "fixture-plan-turn".into(),
        explanation: None,
        plan: vec![TurnPlanStep {
            step: "Verify native workflow 研究".into(),
            status,
        }],
    })
}

#[tokio::test]
async fn native_plan_event_updates_hidden_panel_and_completed_plan_survives_turn_end_and_rewind_clears_it()
 {
    let (mut chat, mut events, mut operations) =
        make_chatwidget_manual(/*model_override*/ None).await;
    let thread = ThreadId::new();
    chat.thread_id = Some(thread);
    chat.apply_external_edit("Unicode draft équipe 研究".into());
    chat.handle_key_event(KeyEvent::from(KeyCode::Left));
    let area = Rect::new(0, 0, 80, 16);
    let before = (
        chat.composer_text_with_pending(),
        chat.as_renderable().cursor_pos(area),
    );
    chat.handle_server_notification(
        notification(thread, TurnPlanStepStatus::InProgress),
        /*replay_kind*/ None,
    );
    assert!(!render_bottom_popup(&chat, 80).contains("Tasks ·"));
    chat.toggle_plan_checklist();
    assert!(render_bottom_popup(&chat, 80).contains("0/1 complete"));
    assert!(render_bottom_popup(&chat, 80).contains("Verify native workflow"));
    chat.handle_server_notification(
        notification(thread, TurnPlanStepStatus::Completed),
        /*replay_kind*/ None,
    );
    handle_turn_completed(&mut chat, "fixture-plan-turn", /*duration_ms*/ None);
    assert!(render_bottom_popup(&chat, 80).contains("1/1 complete"));
    chat.toggle_plan_checklist();
    assert_eq!(
        (
            chat.composer_text_with_pending(),
            chat.as_renderable().cursor_pos(area)
        ),
        before
    );
    let history = drain_insert_history(&mut events);
    assert!(
        history
            .iter()
            .any(|lines| lines_to_single_string(lines).contains("Updated Plan"))
    );
    assert_no_submit_op(&mut operations);
    chat.reset_after_prompt_revert(/*rollout_path*/ None, &[]);
    assert!(!chat.transcript.plan_checklist.visible);
    chat.toggle_plan_checklist();
    assert!(render_bottom_popup(&chat, 80).contains("No native plan received"));
    assert!(!render_bottom_popup(&chat, 80).contains("Verify native workflow"));
}

#[tokio::test]
async fn plan_panel_rejects_another_threads_update_and_yields_to_modals() {
    let (mut chat, _events, _operations) = make_chatwidget_manual(/*model_override*/ None).await;
    let thread = ThreadId::new();
    chat.thread_id = Some(thread);
    chat.handle_server_notification(
        notification(ThreadId::new(), TurnPlanStepStatus::Completed),
        /*replay_kind*/ None,
    );
    chat.toggle_plan_checklist();
    assert!(render_bottom_popup(&chat, 80).contains("No native plan received"));
    chat.show_tui_mode_picker();
    let modal = render_bottom_popup(&chat, 80);
    assert!(modal.contains("TUI mode for next launch"));
    assert!(!modal.contains("Tasks"));
    chat.handle_server_notification(
        notification(thread, TurnPlanStepStatus::InProgress),
        /*replay_kind*/ None,
    );
    chat.handle_key_event(KeyEvent::from(KeyCode::Esc));
    assert!(render_bottom_popup(&chat, 80).contains("0/1 complete"));
}

#[tokio::test]
async fn native_plan_panel_composer_frames_narrow_and_wide() {
    let (mut chat, _events, _operations) = make_chatwidget_manual(/*model_override*/ None).await;
    let thread = ThreadId::new();
    chat.thread_id = Some(thread);
    chat.apply_external_edit("Review équipe 研究".into());
    chat.handle_server_notification(
        notification(thread, TurnPlanStepStatus::InProgress),
        /*replay_kind*/ None,
    );
    chat.toggle_plan_checklist();
    let frames = [32, 80]
        .into_iter()
        .map(|width| format!("width={width}\n{}", render_bottom_popup(&chat, width)))
        .collect::<Vec<_>>();
    insta::assert_snapshot!(frames.join("\n\n"));
}
