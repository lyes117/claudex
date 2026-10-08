use super::super::tests::new_test_composer;
use super::*;
use codex_app_server_protocol::ClaudeCommandMetadata;
use codex_app_server_protocol::SkillScope;
use pretty_assertions::assert_eq;

fn command(user_invocable: bool) -> SkillMetadata {
    SkillMetadata {
        name: "deploy".into(),
        description: "Deploy fixture".into(),
        short_description: None,
        interface: None,
        dependencies: None,
        scope: SkillScope::Repo,
        enabled: true,
        plugin_id: None,
        path: codex_utils_absolute_path::AbsolutePathBuf::from_absolute_path(
            std::env::temp_dir().join("absent/.claude/commands/deploy.md"),
        )
        .expect("path"),
        claude_command: Some(ClaudeCommandMetadata {
            user_invocable,
            argument_hint: Some("[révision]".into()),
        }),
    }
}

#[test]
fn opened_slash_popup_tracks_changed_skill_catalogue_and_keeps_dismissal() {
    let (mut composer, _events) = new_test_composer();
    composer.set_skill_mentions(Some(vec![command(true)]));
    composer.set_text_content("/deploy".into(), Vec::new(), Vec::new());
    composer.set_current_cursor(7);
    assert!(matches!(composer.popups.active, ActivePopup::Command(_)));
    let mut replacement = command(true);
    replacement.name = "other".into();
    composer.set_skill_mentions(Some(vec![replacement]));
    assert!(!matches!(composer.popups.active, ActivePopup::Command(_)));

    composer.set_skill_mentions(Some(vec![command(true)]));
    composer.handle_key_event(KeyEvent::from(KeyCode::Esc));
    let dismissed = composer.popups.dismissed_command_token.clone();
    composer.set_skill_mentions(Some(vec![command(true)]));
    assert!(!matches!(composer.popups.active, ActivePopup::Command(_)));
    assert_eq!(composer.popups.dismissed_command_token, dismissed);
}

#[test]
fn opened_slash_popup_requalifies_skill_after_native_tier_arrives() {
    let (mut composer, _events) = new_test_composer();
    composer.set_service_tier_commands_enabled(true);
    let mut skill = command(true);
    skill.name = "fast".into();
    composer.set_skill_mentions(Some(vec![skill]));
    composer.set_text_content("/fast".into(), Vec::new(), Vec::new());
    composer.set_current_cursor(5);
    let ActivePopup::Command(popup) = &composer.popups.active else {
        panic!("slash popup");
    };
    assert!(matches!(
        popup.selected_item(),
        Some(CommandItem::ClaudeSkill(_))
    ));
    composer.set_service_tier_commands(vec![ServiceTierCommand {
        id: "priority".into(),
        name: "fast".into(),
        description: "Native tier".into(),
    }]);
    let ActivePopup::Command(popup) = &composer.popups.active else {
        panic!("refreshed slash popup");
    };
    assert!(matches!(
        popup.selected_item(),
        Some(CommandItem::ServiceTier(_))
    ));
}

#[test]
fn native_skill_popup_and_typed_submission_keep_arguments_and_canonical_source_identity() {
    for input in ["/dep 研究", "/deploy 研究"] {
        let (mut composer, _events) = new_test_composer();
        let mut skill = command(true);
        skill.claude_command = None;
        composer.set_skill_mentions(Some(vec![skill.clone()]));
        composer.set_text_content(input.into(), Vec::new(), Vec::new());
        composer.set_current_cursor(if input.starts_with("/dep ") {
            input.find(' ').expect("arguments")
        } else {
            input.len()
        });
        let (result, _) = composer.handle_key_event(KeyEvent::from(KeyCode::Enter));
        let InputResult::ClaudeCommand(request) = result else {
            panic!("native skill must request server expansion");
        };
        assert_eq!(
            (request.name, request.path, request.arguments),
            (skill.name, skill.path, "研究".into())
        );
        assert_eq!(composer.current_text(), "/deploy 研究");
        let before = composer.snapshot_draft();
        let resolved = composer
            .resolve_claude_command(request.id, Ok("expanded".into()))
            .expect("native skill resolved");
        assert_eq!(composer.snapshot_draft(), before);
        composer.acknowledge_claude_command(resolved, /*accepted*/ true);
        assert_eq!(composer.current_text(), "");
    }
}

#[test]
fn qualified_skill_does_not_shadow_builtin_and_ambiguous_paths_preserve_draft() {
    let (mut composer, _events) = new_test_composer();
    let mut skill = command(true);
    skill.name = "model".into();
    composer.set_skill_mentions(Some(vec![skill.clone()]));
    composer.set_text_content("/model".into(), Vec::new(), Vec::new());
    assert!(
        composer
            .try_prepare_claude_command(/*should_queue*/ false)
            .is_none()
    );
    composer.set_text_content("/skill:model 研究".into(), Vec::new(), Vec::new());
    let InputResult::ClaudeCommand(request) = composer
        .try_prepare_claude_command(/*should_queue*/ false)
        .expect("qualified command")
    else {
        panic!("qualified skill expansion");
    };
    assert_eq!(request.name, "model");
    assert_eq!(request.path, skill.path);
    assert_eq!(request.arguments, "研究");

    let mut duplicate = skill.clone();
    duplicate.path = codex_utils_absolute_path::AbsolutePathBuf::from_absolute_path(
        std::env::temp_dir().join("other/model.md"),
    )
    .expect("path");
    composer.set_skill_mentions(Some(vec![skill, duplicate]));
    let before = composer.snapshot_draft();
    assert!(request.cancellation.is_cancelled());
    assert!(
        composer
            .resolve_claude_command(request.id, Ok("late".into()))
            .is_none()
    );
    let (result, _) = composer.handle_key_event(KeyEvent::from(KeyCode::Enter));
    assert!(matches!(result, InputResult::None));
    assert_eq!(composer.snapshot_draft(), before);
}

#[test]
fn tab_completes_only_token_preserving_unicode_arguments_and_elements_without_dispatch() {
    let (mut composer, _events) = new_test_composer();
    composer.set_skill_mentions(Some(vec![command(true)]));
    let text = "/dep 研究\nsecond line";
    let element = TextElement::new((5..11).into(), Some("研究".into()));
    composer.set_text_content(text.into(), vec![element.clone()], Vec::new());
    composer.set_current_cursor(4);
    let (result, _) = composer.handle_key_event(KeyEvent::from(KeyCode::Tab));
    assert!(matches!(result, InputResult::None));
    assert_eq!(composer.current_text(), "/deploy 研究\nsecond line");
    assert_eq!(
        composer.current_text_elements(),
        vec![element.map_range(|_| (8..14).into())]
    );
    assert!(composer.pending_claude_command.is_none());
}

#[test]
fn normal_and_queued_failures_preserve_complete_draft_caret_and_do_not_duplicate_enter() {
    for should_queue in [false, true] {
        let (mut composer, _events) = new_test_composer();
        composer.set_skill_mentions(Some(vec![command(true)]));
        composer.set_text_content(
            "/deploy \"équipe 研究\"\nsecond".into(),
            Vec::new(),
            Vec::new(),
        );
        composer.set_current_cursor(11);
        let before = composer.snapshot_draft();
        let InputResult::ClaudeCommand(request) = composer
            .try_prepare_claude_command(should_queue)
            .expect("command")
        else {
            panic!("expected preparation");
        };
        assert_eq!(composer.snapshot_draft(), before);
        assert!(matches!(
            composer.try_prepare_claude_command(should_queue),
            Some(InputResult::None)
        ));
        assert!(
            composer
                .resolve_claude_command(request.id, Err("failed".into()))
                .is_none()
        );
        assert_eq!(composer.snapshot_draft(), before);
        assert!(request.cancellation.is_cancelled());
        assert!(
            composer
                .resolve_claude_command(request.id, Ok("late response".into()))
                .is_none()
        );
    }
}

#[test]
fn edits_pastes_and_scope_cancellation_retire_response_even_after_draft_returns_to_same_text() {
    for edit in 0..6 {
        let (mut composer, _events) = new_test_composer();
        composer.set_skill_mentions(Some(vec![command(true)]));
        composer.set_text_content("/deploy équipe".into(), Vec::new(), Vec::new());
        let InputResult::ClaudeCommand(request) = composer
            .try_prepare_claude_command(/*should_queue*/ false)
            .expect("command")
        else {
            panic!("preparation");
        };
        match edit {
            0 => {
                composer.handle_key_event(KeyEvent::from(KeyCode::Char('x')));
            }
            1 => {
                composer.handle_paste("paste".into());
            }
            2 => composer.cancel_claude_command(),
            3 => {
                composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
            }
            4 => composer.set_current_cursor(1),
            5 => composer.set_parent_owned_thread(),
            _ => unreachable!(),
        }
        composer.set_text_content("/deploy équipe".into(), Vec::new(), Vec::new());
        let before = composer.snapshot_draft();
        assert!(request.cancellation.is_cancelled());
        assert!(
            composer
                .resolve_claude_command(request.id, Ok("expanded".into()))
                .is_none()
        );
        assert_eq!(composer.snapshot_draft(), before);
    }
}

#[test]
fn typed_hidden_command_is_rejected_and_success_is_acknowledged_only_after_acceptance() {
    let (mut composer, _events) = new_test_composer();
    composer.set_skill_mentions(Some(vec![command(false)]));
    composer.set_text_content("/deploy équipe".into(), Vec::new(), Vec::new());
    composer.set_current_cursor(11);
    let before = composer.snapshot_draft();
    assert!(matches!(
        composer.try_prepare_claude_command(/*should_queue*/ false),
        Some(InputResult::None)
    ));
    assert_eq!(composer.snapshot_draft(), before);
    composer.set_skill_mentions(Some(vec![command(true)]));
    let InputResult::ClaudeCommand(request) = composer
        .try_prepare_claude_command(/*should_queue*/ false)
        .expect("command")
    else {
        panic!("preparation");
    };
    let resolved = composer
        .resolve_claude_command(request.id, Ok("expanded".into()))
        .expect("resolved");
    assert_eq!(composer.snapshot_draft(), before);
    composer.acknowledge_claude_command(resolved, /*accepted*/ false);
    assert_eq!(composer.snapshot_draft(), before);
}
