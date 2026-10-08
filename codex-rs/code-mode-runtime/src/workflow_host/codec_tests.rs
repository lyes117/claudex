//! Codec fixtures. Synthetic protocol data, not native agent execution.
use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::io::Cursor;

#[test]
fn framing_utf8_and_truncation() {
    let message = ParentMessage::Start {
        version: VERSION,
        script: "return 'é漢字';".into(),
        arguments: json!({}),
    };
    let mut frame = Vec::new();
    write_frame(&mut frame, &message).unwrap();
    let decoded: ParentMessage = read_frame(&mut Cursor::new(&frame)).unwrap();
    validate_start(&decoded).unwrap();
    for length in 0..frame.len() {
        assert!(read_frame(&mut Cursor::new(&frame[..length])).is_err());
    }
    assert!(decode(b"\xff").is_err());
    assert!(read_frame(&mut Cursor::new(u32::MAX.to_be_bytes())).is_err());
}
#[test]
fn duplicate_nested_keys_and_precision() {
    assert!(json(r#"{"schema":{"type":"object","type":"string"}}"#).is_err());
    let raw = "0.9999999999999999999999999999999999999999999";
    assert_eq!(json(raw).unwrap().to_string(), raw);
    assert!(json("[null] trailing").is_err());
    let deep = format!("{}null{}", "[".repeat(13), "]".repeat(13));
    assert!(json(&deep).is_err());
    let many = format!("[{}]", vec!["null"; 256].join(","));
    assert!(json(&many).is_err());
}
#[test]
fn unknown_fields_versions_imports_and_group_budget() {
    assert!(decode(br#"{"kind":"cancel","version":1,"parent":"forged"}"#).is_err());
    let bad = ParentMessage::Start {
        version: 2,
        script: "return null;".into(),
        arguments: json!({}),
    };
    assert!(validate_start(&bad).is_err());
    let late = ParentMessage::Start {
        version: VERSION,
        script: "await workflow.group({}); import('late');".into(),
        arguments: json!({}),
    };
    validate_start(&late).unwrap();
    let ParentMessage::Start { script, .. } = late else {
        panic!("start")
    };
    assert!(super::super::import_preflight::prepare(&script).is_err());
    let calls: Vec<_> = (0..65)
        .map(|i| AgentCall {
            name: i.to_string(),
            prompt: "fixture".into(),
            role: None,
            phase: "fixture".into(),
            schema: json!({"type":"object"}),
        })
        .collect();
    assert!(validate_group("phase", &calls).is_err());
    assert!(encode(&"x".repeat(FRAME_BYTES), FRAME_BYTES).is_err());
}

#[test]
fn logical_group_limit_is_distinct_from_active_native_concurrency() {
    let calls: Vec<_> = (0..64)
        .map(|i| AgentCall {
            name: i.to_string(),
            prompt: "fixture".into(),
            role: None,
            phase: "fixture".into(),
            schema: json!({"type":"object"}),
        })
        .collect();
    validate_group("fixture", &calls).unwrap();
}
#[test]
fn script_and_agent_payload_have_distinct_caps() {
    let start = ParentMessage::Start {
        version: VERSION,
        script: format!("/*{}*/ return null;", "x".repeat(300 * 1024)),
        arguments: json!({}),
    };
    validate_start(&start).unwrap();
    let call = AgentCall {
        name: "one".into(),
        prompt: "x".repeat(JSON_BYTES + 1),
        role: None,
        phase: "fixture".into(),
        schema: json!({"type":"object"}),
    };
    assert_eq!(validate_group("fixture", &[call]), Err(Fault::Limit));
}

#[test]
fn reserved_serde_keys_remain_ordinary_arguments_and_group_results() {
    for data in [
        r#"{"$serde_json::private::Number":"123456789012345678901234567890"}"#,
        r#"{"$serde_json::private::RawValue":"[[[[[[[[[[[[[[null]]]]]]]]]]]]]]"}"#,
        r#"{"nested":[{"$serde_json::private::Number":"1.00000000000000000001"}]}"#,
        " \n\t {\"$serde_json::private::Number\":\"123456789012345678901234567890\"} ",
    ] {
        let expected = json(data).unwrap();
        let bytes =
            format!(r#"{{"kind":"start","version":1,"script":"return args;","arguments":{data}}}"#);
        let message = decode(bytes.as_bytes()).unwrap();
        validate_start(&message).unwrap();
        let ParentMessage::Start { arguments, .. } = message else {
            panic!("start")
        };
        assert_eq!(arguments, expected);

        let bytes =
            format!(r#"{{"kind":"group_result","version":1,"sequence":1,"values":[{data}]}}"#);
        let ParentMessage::GroupResult { values, .. } = decode(bytes.as_bytes()).unwrap() else {
            panic!("group result")
        };
        assert_eq!(values, vec![expected]);
    }
}

#[test]
fn schema_move_preserves_rawvalue_keys_without_reparsing_or_expanding_payload() {
    let deep_string = format!("{}null{}", "[".repeat(1000), "]".repeat(1000));
    let mut schema = serde_json::Map::new();
    schema.insert(
        "$serde_json::private::RawValue".to_string(),
        Value::String(deep_string),
    );
    let schema = Value::Object(schema);
    let call = super::super::decoding::agent(
        json!({"name":"one","prompt":"fixture","phase":"fixture","schema":schema}),
    )
    .unwrap();
    assert_eq!(call.schema, schema);
    // This object stays string data; a later native schema validator must reject its keyword.
    validate_group("fixture", &[call]).unwrap();

    let raw = format!("{}null{}", "[".repeat(13), "]".repeat(13));
    let schema = strict_json::parse_envelope(&raw).unwrap();
    let call = super::super::decoding::agent(
        json!({"name":"one","prompt":"fixture","phase":"fixture","schema":schema}),
    )
    .unwrap();
    assert_eq!(validate_group("fixture", &[call]), Err(Fault::Protocol));

    let mut oversized = serde_json::Map::new();
    oversized.insert(
        "$serde_json::private::RawValue".to_string(),
        Value::String("[".repeat(JSON_BYTES + 1)),
    );
    let message = ParentMessage::Start {
        version: VERSION,
        script: "return args;".into(),
        arguments: Value::Object(oversized),
    };
    assert_eq!(validate_start(&message), Err(Fault::Limit));
}

#[test]
fn manual_dto_extraction_refuses_unknown_fields_and_noninteger_metadata() {
    for bytes in [
        br#"{"kind":"cancel","version":1,"parent":"forged"}"#.as_slice(),
        br#"{"kind":"cancel","version":256}"#.as_slice(),
        br#"{"kind":"cancel","version":1.5}"#.as_slice(),
        br#"{"kind":"group_result","version":1,"sequence":18446744073709551616,"values":[]}"#
            .as_slice(),
        br#"{"kind":"group_result","version":1,"sequence":1.5,"values":[]}"#.as_slice(),
    ] {
        assert!(decode(bytes).is_err());
    }
    assert!(
        super::super::decoding::agent(
            json!({"name":"one","prompt":"fixture","phase":"fixture","schema":{},"tools":["shell"]})
        )
        .is_err()
    );
}
