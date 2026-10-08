use super::*;
use codex_app_server_protocol::ClaudeCommandMetadata;
use pretty_assertions::assert_eq;

fn command() -> SkillMetadata {
    SkillMetadata {
        name: "deploy".into(),
        description: "Deploy fixture".into(),
        short_description: None,
        interface: None,
        dependencies: None,
        scope: crate::test_support::skill_scope_repo(),
        enabled: true,
        plugin_id: None,
        path: test_path_buf("/fixture/.claude/commands/deploy.md").abs(),
        claude_command: Some(ClaudeCommandMetadata {
            user_invocable: true,
            argument_hint: None,
        }),
    }
}

#[tokio::test]
async fn native_slash_skill_dispatches_or_queues_expanded_body_with_original_slash_history() {
    for should_queue in [false, true] {
        let (mut chat, mut events, mut operations) =
            make_chatwidget_manual(/*model_override*/ None).await;
        chat.thread_id = Some(ThreadId::new());
        let mut skill = command();
        skill.claude_command = None;
        chat.set_skills(Some(vec![skill.clone()]));
        chat.input_queue.suppress_queue_autosend = should_queue;
        chat.apply_external_edit("/deploy \u{00e9}quipe \u{7814}\u{7a76}".into());
        let original = chat.composer_text_with_pending();
        chat.handle_key_event(KeyEvent::from(KeyCode::Enter));
        let request = std::iter::from_fn(|| events.try_recv().ok())
            .find_map(|event| match event {
                AppEvent::ExpandClaudeCommand { request, .. } => Some(request),
                _ => None,
            })
            .expect("native skill uses server expansion");
        assert_eq!(request.name, skill.name);
        assert_eq!(request.path, skill.path);
        assert_eq!(request.arguments, "\u{00e9}quipe \u{7814}\u{7a76}");
        assert_no_submit_op(&mut operations);
        assert_eq!(chat.composer_text_with_pending(), original);
        chat.on_claude_command_expanded(request.id, Ok("!literal native skill body".into()));
        if should_queue {
            let queued = chat
                .input_queue
                .queued_user_messages
                .back()
                .expect("queued skill");
            assert_eq!(queued.user_message.text, "!literal native skill body");
            assert_eq!(queued.action, QueuedInputAction::Literal);
            assert_eq!(
                chat.input_queue.queued_user_message_history_records.back(),
                Some(&UserMessageHistoryRecord::Override(
                    UserMessageHistoryOverride {
                        text: original.clone(),
                        text_elements: Vec::new(),
                    }
                ))
            );
            assert_no_submit_op(&mut operations);
        } else {
            let Op::UserTurn { items, .. } = next_submit_op(&mut operations) else {
                panic!("native user turn");
            };
            assert_eq!(
                items,
                vec![UserInput::Text {
                    text: "!literal native skill body".into(),
                    text_elements: Vec::new(),
                }]
            );
        }
        assert_eq!(chat.composer_text_with_pending(), "");
        chat.on_claude_command_expanded(request.id, Ok("late duplicate".into()));
        assert_no_submit_op(&mut operations);
    }
}

#[tokio::test]
async fn expanded_command_dispatches_one_native_user_turn_only_after_ack_and_keeps_shell_prefix_literal()
 {
    let (mut chat, mut events, mut operations) =
        make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());
    chat.set_skills(Some(vec![command()]));
    chat.apply_external_edit("/deploy \u{00e9}quipe \u{7814}\u{7a76}".into());
    let original = chat.composer_text_with_pending();
    chat.handle_key_event(KeyEvent::from(KeyCode::Enter));
    let request = std::iter::from_fn(|| events.try_recv().ok())
        .find_map(|event| match event {
            AppEvent::ExpandClaudeCommand { request, .. } => Some(request),
            _ => None,
        })
        .expect("server expansion request");
    assert_eq!(chat.composer_text_with_pending(), original);
    assert_no_submit_op(&mut operations);
    chat.on_claude_command_expanded(
        request.id,
        Ok("!this is model text \u{00e9}quipe \u{7814}\u{7a76}".into()),
    );
    let Op::UserTurn { items, .. } = next_submit_op(&mut operations) else {
        panic!("native user turn");
    };
    assert_eq!(
        items,
        vec![UserInput::Text {
            text: "!this is model text \u{00e9}quipe \u{7814}\u{7a76}".into(),
            text_elements: Vec::new()
        }]
    );
    assert_eq!(chat.composer_text_with_pending(), "");
    chat.on_claude_command_expanded(request.id, Ok("late duplicate".into()));
    assert_no_submit_op(&mut operations);
}

#[tokio::test]
async fn async_failure_preserves_existing_queue_draft_and_exact_caret_then_success_queues_literal_body()
 {
    let (mut chat, mut events, mut operations) =
        make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());
    chat.set_skills(Some(vec![command()]));
    chat.input_queue.suppress_queue_autosend = true;
    assert!(chat.queue_user_message(UserMessage::from("existing queued input")));
    chat.apply_external_edit("/deploy \u{00e9}quipe".into());
    chat.handle_key_event(KeyEvent::from(KeyCode::Left));
    let before = (
        chat.bottom_pane.composer_text(),
        chat.bottom_pane.composer_cursor(),
        chat.bottom_pane.composer_text_elements(),
        chat.input_queue.queued_user_messages.clone(),
        chat.input_queue.queued_user_message_history_records.clone(),
    );
    chat.handle_key_event(KeyEvent::from(KeyCode::Enter));
    let request = std::iter::from_fn(|| events.try_recv().ok())
        .find_map(|event| match event {
            AppEvent::ExpandClaudeCommand { request, .. } => Some(request),
            _ => None,
        })
        .expect("first preparation");
    chat.on_claude_command_expanded(request.id, Err("backend unavailable".into()));
    assert_eq!(
        (
            chat.bottom_pane.composer_text(),
            chat.bottom_pane.composer_cursor(),
            chat.bottom_pane.composer_text_elements(),
            chat.input_queue.queued_user_messages.clone(),
            chat.input_queue.queued_user_message_history_records.clone()
        ),
        before
    );
    assert_no_submit_op(&mut operations);
    chat.handle_key_event(KeyEvent::from(KeyCode::Enter));
    let request = std::iter::from_fn(|| events.try_recv().ok())
        .find_map(|event| match event {
            AppEvent::ExpandClaudeCommand { request, .. } => Some(request),
            _ => None,
        })
        .expect("second preparation");
    chat.on_claude_command_expanded(request.id, Ok("!literal expanded body".into()));
    assert_eq!(chat.input_queue.queued_user_messages.len(), 2);
    let queued = chat
        .input_queue
        .queued_user_messages
        .back()
        .expect("expanded queue entry");
    assert_eq!(queued.user_message.text, "!literal expanded body");
    assert_eq!(queued.action, QueuedInputAction::Literal);
    assert_eq!(chat.composer_text_with_pending(), "");
    assert_no_submit_op(&mut operations);
}
