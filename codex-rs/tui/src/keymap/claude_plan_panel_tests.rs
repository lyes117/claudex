use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn checklist_default_yields_to_existing_control_t_actions_and_chord_prefixes() {
    for (config, context, action) in [
        (
            json!({"global": {"open_transcript": "ctrl-t"}}),
            "global",
            "open_transcript",
        ),
        (
            json!({"pager": {"close_transcript": "ctrl-t"}}),
            "pager",
            "close_transcript",
        ),
        (json!({"global": {"copy": "ctrl-t"}}), "global", "copy"),
    ] {
        let runtime = RuntimeKeymap::from_config(&serde_json::from_value(config).unwrap()).unwrap();
        let control_t = KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL);
        assert!(!runtime.app.toggle_plan_checklist.is_pressed(control_t));
        assert!(
            bindings_for_action(&runtime, context, action)
                .unwrap()
                .is_pressed(control_t)
        );
    }
    let runtime = RuntimeKeymap::from_config(
        &serde_json::from_value(json!({"global": {"clear_terminal": "ctrl-t l"}})).unwrap(),
    )
    .unwrap();
    let control_t = KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL);
    assert!(!runtime.app.toggle_plan_checklist.is_pressed(control_t));
    let mut matcher = KeyChordMatcher::default();
    let contexts = KeymapContextSet::new(KeymapContext::Global);
    assert!(matches!(
        matcher.advance(control_t, &runtime.chords, contexts),
        KeyChordMatch::Pending(_)
    ));
    let KeyChordMatch::Completed(dispatch) =
        matcher.advance(KeyCode::Char('l').into(), &runtime.chords, contexts)
    else {
        panic!("custom Ctrl+T chord")
    };
    assert!(runtime.app.clear_terminal.is_pressed(dispatch));
}

#[test]
fn checklist_remaps_unbindings_and_chords_use_the_native_inventory() {
    for (config, expected) in [
        (json!({"global": {"toggle_plan_checklist": []}}), None),
        (
            json!({"global": {"toggle_plan_checklist": "f12"}}),
            Some("f12"),
        ),
        (
            json!({"global": {"toggle_plan_checklist": "ctrl-x t"}}),
            Some("ctrl+x t"),
        ),
    ] {
        let runtime = RuntimeKeymap::from_config(&serde_json::from_value(config).unwrap()).unwrap();
        assert_eq!(
            runtime
                .primary_hint(KeymapContext::Global, "toggle_plan_checklist")
                .map(ShortcutHint::display_label),
            expected.map(str::to_owned)
        );
        assert!(
            !runtime
                .app
                .toggle_plan_checklist
                .is_pressed(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL))
        );
    }
    let runtime = RuntimeKeymap::from_config(
        &serde_json::from_value(json!({"global": {"toggle_plan_checklist": "ctrl-x t"}})).unwrap(),
    )
    .unwrap();
    let mut matcher = KeyChordMatcher::default();
    let contexts = KeymapContextSet::new(KeymapContext::Global);
    assert!(matches!(
        matcher.advance(
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
            &runtime.chords,
            contexts
        ),
        KeyChordMatch::Pending(_)
    ));
    let KeyChordMatch::Completed(dispatch) =
        matcher.advance(KeyCode::Char('t').into(), &runtime.chords, contexts)
    else {
        panic!("checklist chord")
    };
    assert!(runtime.app.toggle_plan_checklist.is_pressed(dispatch));
}
