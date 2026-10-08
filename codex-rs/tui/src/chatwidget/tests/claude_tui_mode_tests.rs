use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn claude_tui_inline_selection_only_requests_a_next_launch_preference() {
    for (args, enabled) in [
        ("classic", false),
        ("  FULLSCREEN  ", true),
        ("scrollback", false),
    ] {
        let (mut chat, mut events, mut operations) =
            make_chatwidget_manual(/*model_override*/ None).await;
        let before = chat.local_settings.clone();
        chat.dispatch_command_with_args(SlashCommand::Tui, args.into(), Vec::new());
        let choices = std::iter::from_fn(|| events.try_recv().ok())
            .filter_map(|event| match event {
                AppEvent::FullscreenTranscriptSelected { enabled } => Some(enabled),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(choices, vec![enabled]);
        assert_eq!(chat.local_settings, before);
        assert_no_submit_op(&mut operations);
    }
}

#[tokio::test]
async fn claude_tui_inline_invalid_arguments_do_not_select_or_start_a_turn() {
    for args in [
        "modern",
        "classic fullscreen",
        "fullscreen now",
        "classic\nfullscreen",
    ] {
        let (mut chat, mut events, mut operations) =
            make_chatwidget_manual(/*model_override*/ None).await;
        let before = chat.local_settings.clone();
        chat.dispatch_command_with_args(SlashCommand::Tui, args.into(), Vec::new());
        let mut errors = Vec::new();
        while let Ok(event) = events.try_recv() {
            match event {
                AppEvent::FullscreenTranscriptSelected { .. } => panic!("invalid mode selected"),
                AppEvent::InsertHistoryCell(cell) => errors.extend(
                    cell.display_lines(/*width*/ 80)
                        .iter()
                        .map(ToString::to_string),
                ),
                _ => {}
            }
        }
        assert!(
            errors
                .join("\n")
                .contains("Usage: /tui [classic|fullscreen]")
        );
        assert_eq!(chat.local_settings, before);
        assert_no_submit_op(&mut operations);
    }
}

#[tokio::test]
async fn claude_tui_empty_arguments_open_the_picker_without_selecting() {
    let (mut chat, mut events, mut operations) =
        make_chatwidget_manual(/*model_override*/ None).await;
    chat.dispatch_command_with_args(SlashCommand::Tui, "  ".into(), Vec::new());
    assert!(render_bottom_popup(&chat, 80).contains("TUI mode for next launch"));
    assert!(
        !std::iter::from_fn(|| events.try_recv().ok())
            .any(|event| matches!(event, AppEvent::FullscreenTranscriptSelected { .. }))
    );
    assert_no_submit_op(&mut operations);
}

#[tokio::test]
async fn claude_tui_typed_inline_selection_reaches_dispatch_once() {
    let (mut chat, mut events, mut operations) =
        make_chatwidget_manual(/*model_override*/ None).await;
    chat.apply_external_edit("/tui fullscreen".into());
    chat.handle_key_event(KeyEvent::from(KeyCode::Esc));
    chat.handle_key_event(KeyEvent::from(KeyCode::Enter));
    let choices = std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|event| match event {
            AppEvent::FullscreenTranscriptSelected { enabled } => Some(enabled),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(choices, vec![true]);
    assert_no_submit_op(&mut operations);
}
