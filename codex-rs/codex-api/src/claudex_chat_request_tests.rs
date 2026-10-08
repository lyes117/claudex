use super::*;
use crate::Reasoning;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::sync::Arc;

fn item(value: Value) -> ResponseItem {
    serde_json::from_value(value).unwrap()
}

fn request() -> ResponsesApiRequest {
    ResponsesApiRequest {
        model: "glm-5.3".into(),
        instructions: "system\nexact ".into(),
        input: vec![item(
            json!({"type":"message","role":"user","content":[{"type":"input_text","text":" task "}]}),
        )],
        tools: None,
        tool_choice: "auto".into(),
        parallel_tool_calls: false,
        reasoning: None,
        store: false,
        stream: true,
        stream_options: None,
        include: Vec::new(),
        service_tier: None,
        prompt_cache_key: None,
        text: None,
        client_metadata: None,
        access_programs: None,
    }
}

fn tools(raw: &str) -> crate::ResponsesApiTools {
    Arc::<RawValue>::from(serde_json::from_str::<Box<RawValue>>(raw).unwrap()).into()
}

fn with_function() -> ResponsesApiRequest {
    let mut r = request();
    r.tools = Some(tools(
        r#"[{"type":"function","name":"Read","description":"Read exact text","strict":false,"parameters":{"type":"object","properties":{"path":{"type":"string"}}}}]"#,
    ));
    r
}

fn call(id: &str, args: &str) -> ResponseItem {
    item(json!({"type":"function_call","name":"Read","call_id":id,"arguments":args}))
}

fn output(id: &str, text: &str) -> ResponseItem {
    item(json!({"type":"function_call_output","call_id":id,"name":"Read","output":text}))
}

fn mapped(r: &ResponsesApiRequest) -> Value {
    serde_json::from_slice(
        &ClaudexChatRequest::from_responses(r, /*max_output_tokens*/ 4096)
            .unwrap()
            .to_json_bytes()
            .unwrap(),
    )
    .unwrap()
}

fn error(r: &ResponsesApiRequest) -> ClaudexChatRequestError {
    ClaudexChatRequest::from_responses(r, /*max_output_tokens*/ 4096).unwrap_err()
}

#[test]
fn preserves_all_text_roles_instructions_and_whitespace() {
    let mut r = request();
    r.input.push(item(json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"a\n"},{"type":"output_text","text":" b"}]})));
    r.input.push(item(json!({"type":"message","role":"system","content":[{"type":"input_text","text":"later system"}]})));
    assert_eq!(
        mapped(&r),
        json!({
            "model":"glm-5.3","messages":[{"role":"system","content":"system\nexact "},{"role":"user","content":" task "},{"role":"assistant","content":"a\n b"},{"role":"system","content":"later system"}],
            "tool_choice":"auto","thinking":{"type":"enabled"},"reasoning_effort":"low","max_tokens":4096,"stream":true,"tool_stream":true
        })
    );
}

#[test]
fn correlates_grouped_function_calls_without_rewriting_arguments_or_outputs() {
    let mut r = with_function();
    r.input.extend([
        call("a", " { \"path\": \"A\" } "),
        call("b", "{}"),
        output("b", " B "),
    ]);
    r.input.push(item(json!({"type":"function_call_output","call_id":"a","output":[{"type":"input_text","text":"\nA"},{"type":"input_text","text":" "},{"type":"input_text","text":"end"}]})));
    let result = mapped(&r);
    assert_eq!(
        result["messages"],
        json!([
            {"role":"system","content":"system\nexact "},{"role":"user","content":" task "},
            {"role":"assistant","content":"","tool_calls":[{"id":"a","type":"function","function":{"name":"Read","arguments":" { \"path\": \"A\" } "}},{"id":"b","type":"function","function":{"name":"Read","arguments":"{}"}}]},
            {"role":"tool","content":" B ","tool_call_id":"b"},{"role":"tool","content":"\nA end","tool_call_id":"a"}
        ])
    );
    assert_eq!(
        result["tools"],
        json!([{"type":"function","function":{"name":"Read","description":"Read exact text","parameters":{"type":"object","properties":{"path":{"type":"string"}}}}}])
    );
}

#[test]
fn refuses_orphans_duplicates_incomplete_and_interleaved_call_groups() {
    for history in [
        vec![output("absent", "x")],
        vec![call("a", "{}")],
        vec![call("a", "{}"), output("a", "x"), output("a", "x")],
        vec![
            call("a", "{}"),
            output("a", "x"),
            call("a", "{}"),
            output("a", "x"),
        ],
        vec![
            call("a", "{}"),
            call("b", "{}"),
            output("a", "x"),
            call("c", "{}"),
            output("b", "x"),
            output("c", "x"),
        ],
    ] {
        let mut r = with_function();
        r.input.extend(history);
        assert_eq!(error(&r), ClaudexChatRequestError::InvalidHistory);
    }
    let mut r = with_function();
    r.input.extend([
        call("a", "{}"),
        item(json!({"type":"function_call_output","call_id":"a","name":"Wrong","output":"x"})),
    ]);
    assert_eq!(error(&r), ClaudexChatRequestError::InvalidHistory);
}

#[test]
fn refuses_multimodal_reasoning_control_and_unknown_history_instead_of_dropping_it() {
    for value in [
        json!({"type":"message","role":"developer","content":[{"type":"input_text","text":"must remain"}]}),
        json!({"type":"message","role":"user","content":[{"type":"input_audio","audio_url":"private"}]}),
        json!({"type":"reasoning","summary":[{"type":"summary_text","text":"must remain"}],"encrypted_content":"cipher"}),
        json!({"type":"custom_tool_call","call_id":"c","name":"apply_patch","input":"patch"}),
        json!({"type":"compaction","encrypted_content":"cipher"}),
        json!({"type":"compaction_trigger"}),
        json!({"type":"future_unknown"}),
        json!({"type":"message","role":"assistant","phase":"final_answer","content":[]}),
        json!({"type":"message","role":"user","content":[],"internal_chat_message_metadata_passthrough":{"turn_id":"private"}}),
    ] {
        let mut r = request();
        r.input.push(item(value));
        assert_eq!(error(&r), ClaudexChatRequestError::UnsupportedItem);
    }
    let mut r = with_function();
    r.input.extend([call("a", "{}"), item(json!({"type":"function_call_output","call_id":"a","output":[{"type":"input_text","text":"visible"},{"type":"encrypted_content","encrypted_content":"cipher"}]}))]);
    assert_eq!(error(&r), ClaudexChatRequestError::UnsupportedItem);
}

#[test]
fn rejects_unsupported_request_controls_and_efforts_without_coercion() {
    for effort in [
        ReasoningEffort::None,
        ReasoningEffort::Minimal,
        ReasoningEffort::Medium,
        ReasoningEffort::XHigh,
        ReasoningEffort::Ultra,
        ReasoningEffort::Persistent,
        ReasoningEffort::Custom("private".into()),
    ] {
        let mut r = request();
        r.reasoning = Some(Reasoning {
            effort: Some(effort),
            summary: None,
            context: None,
        });
        assert_eq!(error(&r), ClaudexChatRequestError::UnsupportedControl);
    }
    for (effort, expected) in [
        (ReasoningEffort::Low, "low"),
        (ReasoningEffort::High, "high"),
        (ReasoningEffort::Max, "max"),
    ] {
        let mut r = request();
        r.reasoning = Some(Reasoning {
            effort: Some(effort),
            summary: None,
            context: None,
        });
        assert_eq!(mapped(&r)["reasoning_effort"], json!(expected));
    }
    let controls: [fn(&mut ResponsesApiRequest); 8] = [
        |r: &mut ResponsesApiRequest| r.parallel_tool_calls = true,
        |r: &mut ResponsesApiRequest| r.store = true,
        |r: &mut ResponsesApiRequest| r.stream = false,
        |r: &mut ResponsesApiRequest| r.tool_choice = "required".into(),
        |r: &mut ResponsesApiRequest| r.model = "gpt-5.2".into(),
        |r: &mut ResponsesApiRequest| r.text = Some(crate::TextControls::default()),
        |r: &mut ResponsesApiRequest| r.include.push("reasoning.encrypted_content".into()),
        |r: &mut ResponsesApiRequest| r.prompt_cache_key = Some("private".into()),
    ];
    for mutate in controls {
        let mut r = request();
        mutate(&mut r);
        assert_eq!(error(&r), ClaudexChatRequestError::UnsupportedControl);
    }
}

#[test]
fn refuses_custom_strict_deferred_duplicate_and_unknown_function_fields() {
    for raw in [
        r#"[{"type":"custom","name":"Read","description":"x","parameters":{}}]"#,
        r#"[{"type":"function","name":"Read","description":"x","parameters":{},"strict":true}]"#,
        r#"[{"type":"function","name":"Read","description":"x","parameters":{},"defer_loading":false}]"#,
        r#"[{"type":"function","name":"Read","description":"x","parameters":{},"future":1}]"#,
        r#"[{"type":"function","name":"Read","name":"Other","description":"x","parameters":{}}]"#,
        r#"[{"type":"function","name":"Read","description":"x","parameters":{}},{"type":"function","name":"Read","description":"x","parameters":{}}]"#,
    ] {
        let mut r = request();
        r.tools = Some(tools(raw));
        assert_eq!(error(&r), ClaudexChatRequestError::UnsupportedTool);
    }
    for args in ["[]", r#"{"path":1,"p\u0061th":2}"#, "{} trailing"] {
        let mut r = with_function();
        r.input.extend([call("a", args), output("a", "x")]);
        assert_eq!(error(&r), ClaudexChatRequestError::InvalidJson);
    }
}

#[test]
fn bounds_fragments_aggregate_json_expansion_schema_depth_and_nodes() {
    let mut r = request();
    r.instructions = "x".repeat(MAX_FRAGMENT_BYTES + 1);
    assert_eq!(error(&r), ClaudexChatRequestError::LimitExceeded);
    r.instructions = "x".repeat(MAX_FRAGMENT_BYTES);
    assert!(ClaudexChatRequest::from_responses(&r, /*max_output_tokens*/ 1).is_ok());
    r.input = (0..7).map(|_| item(json!({"type":"message","role":"user","content":[{"type":"input_text","text":"x".repeat(MAX_FRAGMENT_BYTES)}]}))).collect();
    assert_eq!(error(&r), ClaudexChatRequestError::LimitExceeded);
    r.instructions.clear();
    r.input = (0..2).map(|_| item(json!({"type":"message","role":"user","content":[{"type":"input_text","text":"\u{0001}".repeat(MAX_FRAGMENT_BYTES)}]}))).collect();
    assert_eq!(error(&r), ClaudexChatRequestError::LimitExceeded);
    for schema in [
        format!(
            "{}0{}",
            "[".repeat(MAX_JSON_DEPTH + 1),
            "]".repeat(MAX_JSON_DEPTH + 1)
        ),
        format!("[{}]", vec!["0"; MAX_JSON_NODES + 1].join(",")),
        r#"{"type":"object","type":"string"}"#.into(),
    ] {
        let mut r = request();
        r.tools = Some(tools(&format!(
            r#"[{{"type":"function","name":"Read","description":"x","parameters":{schema}}}]"#
        )));
        assert_eq!(error(&r), ClaudexChatRequestError::InvalidJson);
    }
    let mut r = with_function();
    r.input.extend([
        call(
            "a",
            &format!(r#"{{"x":"{}"}}"#, "x".repeat(MAX_FRAGMENT_BYTES)),
        ),
        output("a", "x"),
    ]);
    assert_eq!(error(&r), ClaudexChatRequestError::LimitExceeded);
    for budget in [0, 16385] {
        assert_eq!(
            ClaudexChatRequest::from_responses(&request(), budget).unwrap_err(),
            ClaudexChatRequestError::InvalidOutputBudget
        );
    }
}

#[test]
fn preserves_schema_number_lexemes_and_refuses_namespaces_and_mismatched_catalog() {
    let mut r = request();
    let raw = r#"[{"type":"function","name":"Read","description":"x","parameters":{"type":"object","properties":{"n":{"const":18446744073709551617}}}}]"#;
    r.tools = Some(tools(raw));
    let bytes = ClaudexChatRequest::from_responses(&r, /*max_output_tokens*/ 4096)
        .unwrap()
        .to_json_bytes()
        .unwrap();
    assert!(
        std::str::from_utf8(&bytes)
            .unwrap()
            .contains("18446744073709551617")
    );
    r.input.extend([item(json!({"type":"function_call","name":"Read","namespace":"functions","call_id":"a","arguments":"{}"})), output("a","x")]);
    assert_eq!(error(&r), ClaudexChatRequestError::UnsupportedItem);
    let mut r = request();
    r.input.extend([call("a", "{}"), output("a", "x")]);
    assert_eq!(error(&r), ClaudexChatRequestError::InvalidHistory);
}
