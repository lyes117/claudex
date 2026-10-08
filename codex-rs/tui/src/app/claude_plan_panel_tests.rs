use super::*;
use crate::app::owned_transcript::tests::attach_thread;
use crate::app::tests::make_test_app_with_channels;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::TurnPlanStep;
use codex_app_server_protocol::TurnPlanStepStatus;
use codex_app_server_protocol::TurnPlanUpdatedNotification;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;

fn pane(chat: &ChatWidget, area: Rect) -> String {
    let mut buffer = Buffer::empty(area);
    chat.bottom_pane_renderable(
        /*footer*/ None,
        crate::bottom_pane::CommandPopupPlacement::AboveComposer,
        /*composer_gap*/ None,
        /*working_tip*/ None,
    )
    .render(area, &mut buffer);
    buffer
        .content()
        .chunks(usize::from(area.width))
        .map(|row| {
            row.iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn plan(thread_id: ThreadId, status: TurnPlanStepStatus) -> ServerNotification {
    ServerNotification::TurnPlanUpdated(TurnPlanUpdatedNotification {
        thread_id: thread_id.to_string(),
        turn_id: "native-panel-turn".into(),
        explanation: None,
        plan: vec![TurnPlanStep {
            step: "Native task fixture 研究".into(),
            status,
        }],
    })
}

#[tokio::test]
async fn control_t_native_event_panel_respects_modal_priority_drafts_and_thread_replacement_in_both_renderers()
-> Result<()> {
    for owned in [false, true] {
        let (mut app, _events, mut commands) = make_test_app_with_channels().await;
        let thread = ThreadId::new();
        attach_thread(&mut app, thread);
        app.chat_widget
            .apply_external_edit("Native draft équipe 研究".into());
        app.chat_widget
            .handle_key_event(KeyEvent::from(KeyCode::Left));
        let area = Rect::new(0, 0, 80, 20);
        let before = (
            app.chat_widget.composer_text_with_pending(),
            app.chat_widget.as_renderable().cursor_pos(area),
        );
        assert!(before.1.is_some());
        app.chat_widget.handle_server_notification(
            plan(thread, TurnPlanStepStatus::Pending),
            /*replay_kind*/ None,
        );
        let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
        let mut tui = crate::tui::test_support::make_test_tui()?;
        tui.set_owned_screen(owned)?;
        let key = KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL);
        app.chat_widget.show_tui_mode_picker();
        app.handle_tui_event(&mut tui, &mut server, TuiEvent::Key(key))
            .await?;
        let modal = pane(&app.chat_widget, area);
        assert!(modal.contains("TUI mode for next launch"));
        assert!(!modal.contains("Tasks"));
        app.handle_tui_event(&mut tui, &mut server, TuiEvent::Key(KeyCode::Esc.into()))
            .await?;
        app.handle_tui_event(&mut tui, &mut server, TuiEvent::Key(key))
            .await?;
        assert!(pane(&app.chat_widget, area).contains("0/1 complete"));
        app.chat_widget.handle_server_notification(
            plan(thread, TurnPlanStepStatus::Completed),
            /*replay_kind*/ None,
        );
        assert!(pane(&app.chat_widget, area).contains("1/1 complete"));
        app.handle_tui_event(&mut tui, &mut server, TuiEvent::Key(key))
            .await?;
        assert!(!pane(&app.chat_widget, area).contains("Tasks"));
        assert_eq!(
            (
                app.chat_widget.composer_text_with_pending(),
                app.chat_widget.as_renderable().cursor_pos(area)
            ),
            before
        );
        assert_eq!(tui.is_owned_screen(), owned);
        while let Ok(command) = commands.try_recv() {
            assert!(!matches!(command, AppCommand::UserTurn { .. }));
        }
        attach_thread(&mut app, ThreadId::new());
        app.handle_tui_event(&mut tui, &mut server, TuiEvent::Key(key))
            .await?;
        assert!(pane(&app.chat_widget, area).contains("No native plan received"));
        assert!(!pane(&app.chat_widget, area).contains("Native task fixture"));
        server.shutdown().await?;
    }
    Ok(())
}
