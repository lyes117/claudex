use super::*;
use serde_json::json;

#[test]
fn native_agent_options_preserve_text_mode_and_alias_intent() {
    let call=decode_agent_value(json!({"name":"scouting:propose","prompt":"Recherche en francais","phase":"Scouting","model":"opus","effort":"high"})).unwrap();
    assert!(call.schema.is_null());
    assert_eq!(call.model.as_deref(), Some("opus"));
    assert_eq!(call.effort.as_deref(), Some("high"));
    validate_group("Scouting", &[call]).unwrap();
}

#[test]
fn child_frames_reject_duplicate_and_unknown_authority_fields() {
    let valid=br#"{"kind":"group","version":1,"sequence":1,"phase":"Scouting","calls":[{"name":"scouting:propose","prompt":"research","phase":"Scouting","schema":null,"model":"sonnet"}]}"#;
    assert!(matches!(
        decode_child(valid).unwrap(),
        ChildMessage::Group { sequence: 1, .. }
    ));
    assert!(
        decode_child(br#"{"kind":"done","version":1,"result":{},"threadId":"foreign"}"#).is_err()
    );
    assert!(decode_child(br#"{"kind":"done","version":1,"result":{"x":1,"x":2}}"#).is_err());
    assert!(
        decode_agent_value(
            json!({"name":"one","prompt":"research","phase":"Scouting","shell":"cmd"})
        )
        .is_err()
    );
}

#[test]
fn observable_control_does_not_accept_path_ids_or_unknown_actions() {
    assert!(valid_run_id("weekly-search-native-20261005-01"));
    for invalid in ["", "../other", "a/b", "a\\b"] {
        assert!(!valid_run_id(invalid));
    }
    assert!(
        serde_json::from_value::<WorkflowControlRequest>(
            json!({"token":"local","revision":"one","action":"publish"})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<WorkflowControlRequest>(
            json!({"token":"local","revision":"one","action":"resume","pid":123})
        )
        .is_err()
    );
}
