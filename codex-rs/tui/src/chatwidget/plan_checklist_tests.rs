use super::*;
use codex_protocol::plan_tool::PlanItemArg;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn foreign_initial_replay_plan_leaves_complete_cached_plan_replay_flags_history_and_draft_untouched()
 {
    use crate::chatwidget::ReplayKind;
    use crate::chatwidget::tests::helpers::make_chatwidget_manual_with_sender;
    use codex_app_server_protocol::ServerNotification;
    use codex_app_server_protocol::TurnPlanStep;
    use codex_app_server_protocol::TurnPlanStepStatus;
    use codex_app_server_protocol::TurnPlanUpdatedNotification;
    use codex_protocol::ThreadId;
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;

    let (mut chat, _sender, mut events, _operations) = make_chatwidget_manual_with_sender().await;
    let thread = ThreadId::new();
    chat.thread_id = Some(thread);
    let notification = |thread_id: ThreadId, status| {
        ServerNotification::TurnPlanUpdated(TurnPlanUpdatedNotification {
            thread_id: thread_id.to_string(),
            turn_id: "replay-plan-fixture".into(),
            explanation: Some("The full native plan is retained".into()),
            plan: (0..40)
                .map(|index| TurnPlanStep {
                    step: format!("Native step {index} équipe 研究"),
                    status,
                })
                .collect(),
        })
    };
    chat.handle_server_notification(notification(thread, TurnPlanStepStatus::InProgress), None);
    chat.toggle_plan_checklist();
    chat.apply_external_edit("Draft équipe 研究".into());
    chat.handle_key_event(KeyEvent::from(KeyCode::Left));
    let area = Rect::new(0, 0, 80, 20);
    let draft = (
        chat.composer_text_with_pending(),
        chat.as_renderable().cursor_pos(area),
    );
    let cached_plan = |chat: &ChatWidget| {
        let checklist = &chat.transcript.plan_checklist;
        let snapshot = checklist
            .snapshot
            .as_ref()
            .expect("real native plan was cached");
        (
            checklist.visible,
            snapshot.total,
            snapshot.completed,
            snapshot
                .steps
                .iter()
                .map(|step| (step.text.clone(), format!("{:?}", step.status)))
                .collect::<Vec<_>>(),
        )
    };
    let before = cached_plan(&chat);
    assert_eq!((before.1, before.2, before.3.len()), (40, 0, 32));
    let plan_progress = chat.transcript.last_plan_progress;
    let saw_plan = chat.transcript.saw_plan_update_this_turn;
    while events.try_recv().is_ok() {}
    for replay_flag in [false, true] {
        chat.thread_usage.replaying_turn_completion = replay_flag;
        chat.handle_server_notification(
            notification(ThreadId::new(), TurnPlanStepStatus::Completed),
            Some(ReplayKind::ResumeInitialMessages),
        );
        assert_eq!(cached_plan(&chat), before);
        assert_eq!(chat.thread_usage.replaying_turn_completion, replay_flag);
        assert_eq!(chat.transcript.last_plan_progress, plan_progress);
        assert_eq!(chat.transcript.saw_plan_update_this_turn, saw_plan);
        assert_eq!(
            (
                chat.composer_text_with_pending(),
                chat.as_renderable().cursor_pos(area)
            ),
            draft
        );
        assert!(
            events.try_recv().is_err(),
            "foreign replay emitted a UI/history event"
        );
    }
}

#[test]
fn hidden_native_plan_projection_is_bounded_and_controls_are_sanitized_before_append() {
    let update = UpdatePlanArgs {
        explanation: None,
        plan: (0..100)
            .map(|index| PlanItemArg {
                step: format!("{index} \u{1b}[31m\n\u{202e}{}", "研究".repeat(1_000)),
                status: if index % 2 == 0 {
                    StepStatus::Completed
                } else {
                    StepStatus::Pending
                },
            })
            .collect(),
    };
    let mut checklist = PlanChecklist::default();
    checklist.update(&update);
    assert!(!checklist.visible);
    let snapshot = checklist.snapshot.as_ref().unwrap();
    assert_eq!(
        (snapshot.steps.len(), snapshot.total, snapshot.completed),
        (32, 100, 50)
    );
    for step in &snapshot.steps {
        assert!(step.text.chars().count() <= MAX_STEP_CHARS + 1);
        assert!(step.text.len() <= (MAX_STEP_CHARS + 1) * 4);
        assert!(!step.text.chars().any(char::is_control));
        assert!(!step.text.contains('\u{202e}'));
        assert!(step.text.ends_with('…'));
    }
    assert!(update.plan[99].step.contains('\u{1b}'));
    assert!(update.plan[99].step.chars().count() > MAX_STEP_CHARS);
    checklist.visible = true;
    let lines = checklist
        .lines(6)
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>();
    assert!(lines[0].contains("50/100 complete"));
    assert!(lines.last().unwrap().contains("96 other steps"));
}

struct Composer;

impl Renderable for Composer {
    fn render(&self, area: Rect, buffer: &mut Buffer) {
        Widget::render(Paragraph::new("draft 研究\nfooter\nlast row"), area, buffer);
    }
    fn desired_height(&self, _width: u16) -> u16 {
        3
    }
    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        (!area.is_empty()).then_some((area.x, area.y))
    }
}

#[test]
fn panel_layout_reserves_the_original_composer_and_cursor_on_short_and_narrow_screens() {
    let mut checklist = PlanChecklist::default();
    checklist.update(&UpdatePlanArgs {
        explanation: None,
        plan: vec![],
    });
    checklist.visible = true;
    let child = Composer;
    let composition = ChecklistComposition {
        checklist: &checklist,
        child: RenderableItem::Borrowed(&child),
    };
    for (height, panel) in [(0, 0), (2, 0), (3, 0), (4, 0), (5, 2), (12, 2)] {
        for width in [0, 1, 32, 80] {
            let area = Rect::new(0, 0, width, height);
            let expected = if width == 0 { 0 } else { panel };
            assert_eq!(composition.panel_height(area), expected);
            assert_eq!(composition.child_area(area).height, height - expected);
            assert_eq!(
                composition.cursor_pos(area),
                child.cursor_pos(composition.child_area(area))
            );
            let mut buffer = Buffer::empty(area);
            composition.render(area, &mut buffer);
        }
    }
}
