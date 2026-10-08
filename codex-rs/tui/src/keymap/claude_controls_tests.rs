//! Resolve compatibility aliases through the same custom-key and chord authority as dispatch.

use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn claude_transcript_alias_yields_to_custom_keys_and_preserves_native_copy() {
    for (config, context, action) in [
        (json!({"global": {"copy": "ctrl-o"}}), "global", "copy"),
        (
            json!({"editor": {"move_line_start": "ctrl-o"}}),
            "editor",
            "move_line_start",
        ),
        (
            json!({"composer": {"submit": "ctrl-o"}}),
            "composer",
            "submit",
        ),
    ] {
        let runtime = RuntimeKeymap::from_config(&serde_json::from_value(config).unwrap())
            .expect("custom control key stays authoritative");
        let control_o = KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL);
        assert!(
            bindings_for_action(&runtime, context, action)
                .unwrap()
                .is_pressed(control_o)
        );
        assert!(!runtime.app.open_transcript.is_pressed(control_o));
        // Native search remains an entry to detailed history when a custom key
        // owns Ctrl+O and Ctrl+T now belongs to the checklist.
        assert!(runtime.app.find_transcript.is_pressed(KeyCode::F(3).into()));
        assert!(
            !runtime
                .app
                .open_transcript
                .is_pressed(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL,))
        );
        assert_eq!(
            runtime
                .primary_hint(KeymapContext::Global, "open_transcript")
                .map(ShortcutHint::display_label),
            None,
        );
    }
}

#[test]
fn claude_transcript_and_copy_defaults_yield_to_custom_chord_prefixes() {
    for (prefix, config, absent) in [
        (
            KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL),
            json!({"global": {"clear_terminal": "ctrl-o l"}}),
            "open_transcript",
        ),
        (
            KeyEvent::from(KeyCode::F(6)),
            json!({"global": {"clear_terminal": "f6 l"}}),
            "copy",
        ),
    ] {
        let runtime = RuntimeKeymap::from_config(&serde_json::from_value(config).unwrap())
            .expect("new defaults cannot shadow a configured prefix");
        assert!(
            !bindings_for_action(&runtime, "global", absent)
                .unwrap()
                .is_pressed(prefix)
        );
        let contexts = KeymapContextSet::new(KeymapContext::Global);
        let mut matcher = KeyChordMatcher::default();
        assert!(matches!(
            matcher.advance(prefix, &runtime.chords, contexts),
            KeyChordMatch::Pending(_)
        ));
        let KeyChordMatch::Completed(dispatch) =
            matcher.advance(KeyCode::Char('l').into(), &runtime.chords, contexts)
        else {
            panic!("configured chord should complete")
        };
        assert!(runtime.app.clear_terminal.is_pressed(dispatch));
    }
}

#[test]
fn claude_controls_remaps_unbindings_and_pager_actions_are_authoritative() {
    for (config, remapped) in [
        (
            json!({"global": {"open_transcript": [], "copy": []}, "pager": {"close_transcript": []}}),
            false,
        ),
        (
            json!({"global": {"open_transcript": "f12", "copy": "f11"}, "pager": {"close_transcript": "f12"}}),
            true,
        ),
    ] {
        let runtime = RuntimeKeymap::from_config(&serde_json::from_value(config).unwrap()).unwrap();
        for control in ['o', 't'] {
            let event = KeyEvent::new(KeyCode::Char(control), KeyModifiers::CONTROL);
            assert!(!runtime.app.open_transcript.is_pressed(event));
            assert!(!runtime.pager.close_transcript.is_pressed(event));
        }
        assert!(!runtime.app.copy.is_pressed(KeyCode::F(6).into()));
        if remapped {
            assert!(
                runtime
                    .app
                    .open_transcript
                    .is_pressed(KeyCode::F(12).into())
            );
            assert!(
                runtime
                    .pager
                    .close_transcript
                    .is_pressed(KeyCode::F(12).into())
            );
            assert!(runtime.app.copy.is_pressed(KeyCode::F(11).into()));
        } else {
            assert_eq!(
                (
                    runtime.app.open_transcript,
                    runtime.pager.close_transcript,
                    runtime.app.copy
                ),
                (Vec::new(), Vec::new(), Vec::new()),
            );
        }
    }
    let runtime = RuntimeKeymap::from_config(
        &serde_json::from_value(json!({
            "pager": {"find": "ctrl-o"}, "global": {"clear_terminal": "f6"}
        }))
        .unwrap(),
    )
    .unwrap();
    assert!(
        runtime
            .pager
            .find
            .is_pressed(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL))
    );
    assert!(
        !runtime
            .pager
            .close_transcript
            .is_pressed(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL))
    );
    assert!(!runtime.app.copy.is_pressed(KeyCode::F(6).into()));
    assert!(runtime.app.clear_terminal.is_pressed(KeyCode::F(6).into()));
}

#[test]
fn claude_controls_explicit_conflicts_are_still_rejected() {
    for config in [
        json!({"global": {"open_transcript": "ctrl-o", "copy": "ctrl-o"}}),
        json!({"global": {"copy": "f6", "clear_terminal": "f6"}}),
        json!({"pager": {"close_transcript": "ctrl-o", "find": "ctrl-o"}}),
    ] {
        RuntimeKeymap::from_config(&serde_json::from_value(config).unwrap())
            .expect_err("explicit conflicts must remain actionable errors");
    }
}
