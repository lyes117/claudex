//! Exercise the native transcript input owner in both Classic and Fullscreen modes.

use super::*;
use crate::app::tests::make_test_app_with_channels;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn claude_control_o_toggles_native_transcript_and_preserves_the_draft_in_both_renderers()
-> Result<()> {
    for owned in [false, true] {
        let (mut app, _events, mut operations) = make_test_app_with_channels().await;
        app.chat_widget
            .apply_external_edit("draft \u{00e9}quipe \u{7814}\u{7a76}".into());
        app.chat_widget
            .handle_key_event(KeyEvent::from(KeyCode::Left));
        app.transcript_cells = vec![Arc::new(history_cell::PlainHistoryCell::new(vec![
            "retained transcript".into(),
        ]))];
        let draft = app.chat_widget.composer_text_with_pending();
        let area = Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 80, /*height*/ 24,
        );
        let caret = app.chat_widget.as_renderable().cursor_pos(area);
        assert!(caret.is_some(), "interior Unicode caret is visible");
        let retained = Arc::clone(&app.transcript_cells[0]);
        let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
        let mut tui = crate::tui::test_support::make_test_tui()?;
        tui.set_owned_screen(owned)?;
        let key = KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL);
        app.handle_tui_event(&mut tui, &mut server, TuiEvent::Key(key))
            .await?;
        if owned {
            assert!(app.overlay.is_none());
            assert!(app.transcript_view.is_detailed());
        } else {
            assert!(matches!(app.overlay, Some(Overlay::Transcript(_))));
        }
        app.handle_tui_event(&mut tui, &mut server, TuiEvent::Key(key))
            .await?;
        assert!(app.overlay.is_none());
        assert!(!app.transcript_view.is_detailed());
        assert_eq!(app.chat_widget.composer_text_with_pending(), draft);
        assert_eq!(app.chat_widget.as_renderable().cursor_pos(area), caret);
        assert!(Arc::ptr_eq(&app.transcript_cells[0], &retained));
        assert_eq!(tui.is_owned_screen(), owned);
        while let Ok(operation) = operations.try_recv() {
            assert!(!matches!(operation, AppCommand::UserTurn { .. }));
        }
        server.shutdown().await?;
    }
    Ok(())
}
