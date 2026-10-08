use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

fn decoder() -> ClaudexChatDecoder {
    ClaudexChatDecoder::new(BTreeSet::from(["Read".to_owned(), "Grep".to_owned()])).unwrap()
}

fn frame(delta: serde_json::Value, finish: Option<&str>) -> String {
    json!({"id":"chatcmpl-fixture", "model":"glm-5.3", "choices":[{
        "index":0,"delta":delta,"finish_reason":finish
    }]})
    .to_string()
}

#[test]
fn preserves_unicode_content_reasoning_and_reported_usage() {
    let mut decoder = decoder();
    decoder
        .push_data(&frame(
            json!({"role":"assistant","reasoning_content":"Vérif"}),
            None,
        ))
        .unwrap();
    let transition = decoder
        .push_data(&frame(
            json!({"reasoning_content":"ication","content":"Résultat 🦀"}),
            None,
        ))
        .unwrap();
    match transition.as_slice() {
        [
            ResponseEvent::ReasoningContentDelta { .. },
            ResponseEvent::OutputItemDone(reasoning),
            ResponseEvent::OutputItemAdded(_),
            ResponseEvent::OutputTextDelta(_),
        ] => {
            assert_eq!(
                serde_json::to_value(reasoning).unwrap(),
                json!({
                    "type":"reasoning","id":"glm_reasoning_chatcmpl-fixture","summary":[],
                    "content":[{"type":"reasoning_text","text":"Vérification"}],"encrypted_content":null
                })
            );
        }
        _ => panic!("reasoning must close before a message starts"),
    }
    decoder
        .push_data(&frame(json!({"content":"\nvalidé"}), Some("stop")))
        .unwrap();
    decoder.push_data(&json!({"id":"chatcmpl-fixture","model":"glm-5.3","choices":[],
        "usage":{"prompt_tokens":10,"completion_tokens":8,"total_tokens":18,
            "prompt_tokens_details":{"cached_tokens":2},"completion_tokens_details":{"reasoning_tokens":3}}
    }).to_string()).unwrap();
    let events = decoder.push_data("[DONE]").unwrap();
    match events.as_slice() {
        [
            ResponseEvent::OutputItemDone(message),
            ResponseEvent::Completed {
                response_id,
                token_usage,
                usage_metadata,
                end_turn,
            },
        ] => {
            assert_eq!(
                serde_json::to_value(message).unwrap(),
                json!({
                    "type":"message","id":"glm_msg_chatcmpl-fixture","role":"assistant",
                    "content":[{"type":"output_text","text":"Résultat 🦀\nvalidé"}]
                })
            );
            assert_eq!(
                (
                    response_id.as_str(),
                    token_usage.clone(),
                    usage_metadata.is_none(),
                    *end_turn
                ),
                (
                    "chatcmpl-fixture",
                    Some(TokenUsage {
                        input_tokens: 10,
                        cached_input_tokens: 2,
                        cache_write_input_tokens: 0,
                        output_tokens: 8,
                        reasoning_output_tokens: 3,
                        total_tokens: 18,
                        codex_rollout_budget_units: None
                    }),
                    true,
                    Some(true)
                )
            );
        }
        _ => panic!("expected completed message with actual usage"),
    }
    decoder.finish_eof().unwrap();
}

#[test]
fn fragmented_interleaved_tools_are_not_dispatched_before_done() {
    let mut decoder = decoder();
    let events = decoder.push_data(&frame(json!({"tool_calls":[
        {"index":0,"id":"call-a","type":"function","function":{"name":"Re","arguments":"{\"path\":"}},
        {"index":1,"id":"call-b","type":"function","function":{"name":"Grep","arguments":"{\"query\":\"é"}}
    ]}), None)).unwrap();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, ResponseEvent::OutputItemDone(_)))
    );
    decoder
        .push_data(&frame(
            json!({"tool_calls":[
                {"index":1,"function":{"arguments":"cole\"}"}},
                {"index":0,"function":{"name":"ad","arguments":"\"src/lib.rs\"}"}}
            ]}),
            Some("tool_calls"),
        ))
        .unwrap();
    assert!(decoder.finish_eof().is_err());
    let events = decoder.push_data("[DONE]").unwrap();
    let items: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            ResponseEvent::OutputItemDone(item) => Some(serde_json::to_value(item).unwrap()),
            _ => None,
        })
        .collect();
    assert_eq!(
        items,
        vec![
            json!({"type":"function_call","id":"glm_call_call-a","name":"Read","arguments":"{\"path\":\"src/lib.rs\"}","call_id":"call-a"}),
            json!({"type":"function_call","id":"glm_call_call-b","name":"Grep","arguments":"{\"query\":\"école\"}","call_id":"call-b"})
        ]
    );
    assert!(matches!(
        events.last(),
        Some(ResponseEvent::Completed {
            token_usage: None,
            end_turn: Some(false),
            ..
        })
    ));
}

#[test]
fn incomplete_invalid_or_refused_streams_never_complete() {
    for (delta, reason) in [
        (json!({"content":"partial"}), None),
        (json!({"content":"partial"}), Some("length")),
        (json!({"refusal":"provider-sensitive-value"}), Some("stop")),
        (json!({"audio":{}}), None),
        (json!({"role":"user","content":"bad"}), Some("stop")),
        (json!({"content":"partial"}), Some("content_filter")),
    ] {
        let mut decoder = decoder();
        let result = decoder.push_data(&frame(delta, reason));
        if let Err(error) = result {
            assert_eq!(
                error.to_string(),
                "stream error: invalid or incomplete GLM chat stream"
            );
        }
        assert!(decoder.finish_eof().is_err());
        assert!(decoder.push_data("[DONE]").is_err());
    }
}

#[test]
fn unknown_tool_duplicate_id_and_invalid_arguments_are_rejected() {
    for calls in [
        json!([{"index":0,"id":"a","type":"function","function":{"name":"Bash","arguments":"{}"}}]),
        json!([{"index":0,"id":"a","type":"function","function":{"name":"Read","arguments":"{broken"}}]),
        json!([{"index":0,"id":"a","type":"function","function":{"name":"Read","arguments":"[]"}}]),
        json!([{"index":0,"id":"a","type":"function","function":{"name":"Read","arguments":"{}"}},
               {"index":1,"id":"a","type":"function","function":{"name":"Grep","arguments":"{}"}}]),
        json!([{"index":0,"id":"a","function":{"name":"Read","arguments":"{}"}}]),
    ] {
        let mut decoder = decoder();
        decoder
            .push_data(&frame(json!({"tool_calls":calls}), Some("tool_calls")))
            .unwrap();
        assert!(decoder.push_data("[DONE]").is_err());
        assert!(decoder.finish_eof().is_err());
    }
}

#[test]
fn response_identity_choices_and_usage_cannot_change() {
    for bad_frame in [
        json!({"id":"different","model":"glm-5.3","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
        json!({"id":"chatcmpl-fixture","model":"glm-other","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
        json!({"id":"chatcmpl-fixture","model":"glm-5.3","choices":[{"index":1,"delta":{},"finish_reason":"stop"}]}),
        json!({"id":"chatcmpl-fixture","model":"glm-5.3","choices":[],"usage":{"prompt_tokens":1,"completion_tokens":2,"total_tokens":99}}),
    ] {
        let mut decoder = decoder();
        decoder
            .push_data(&frame(json!({"content":"ok"}), None))
            .unwrap();
        assert!(decoder.push_data(&bad_frame.to_string()).is_err());
        assert!(decoder.push_data("[DONE]").is_err());
    }
}

#[test]
fn bounds_errors_and_post_terminal_frames_fail_closed() {
    let mut oversized = decoder();
    assert!(
        oversized
            .push_data(&"x".repeat(MAX_FRAME_BYTES + 1))
            .is_err()
    );
    let mut failed = decoder();
    assert!(
        failed
            .push_data("{\"error\":\"secret-in-provider-error\"}")
            .is_err()
    );
    assert!(
        failed
            .push_data(&frame(json!({"content":"ok"}), Some("stop")))
            .is_err()
    );
    let mut completed = decoder();
    completed
        .push_data(&frame(json!({"content":"ok"}), Some("stop")))
        .unwrap();
    completed.push_data("[DONE]").unwrap();
    assert!(completed.push_data("[DONE]").is_err());
}

#[test]
fn aggregate_bytes_frames_tool_index_and_nested_arguments_are_bounded() {
    let mut bytes = decoder();
    for _ in 0..8 {
        let mut padded: serde_json::Value = serde_json::from_str(&frame(json!({}), None)).unwrap();
        padded["system_fingerprint"] = json!("x".repeat(40_000));
        if bytes.push_data(&padded.to_string()).is_err() {
            assert!(bytes.finish_eof().is_err());
            break;
        }
    }
    assert!(bytes.push_data("[DONE]").is_err());
    let mut frames = decoder();
    let tiny = "{\"id\":\"a\",\"model\":\"glm-5.3\",\"choices\":[{\"index\":0,\"delta\":{}}]}";
    for _ in 0..MAX_FRAMES {
        frames.push_data(tiny).unwrap();
    }
    assert!(frames.push_data(tiny).is_err());
    let mut index = decoder();
    assert!(
        index
            .push_data(&frame(json!({"tool_calls":[{"index":16}]}), None))
            .is_err()
    );
    let mut nested = decoder();
    let arguments = format!("{}0{}", "{\"x\":".repeat(14), "}".repeat(14));
    nested
        .push_data(&frame(
            json!({"tool_calls":[{"index":0,"id":"a","type":"function",
        "function":{"name":"Read","arguments":arguments}}]}),
            Some("tool_calls"),
        ))
        .unwrap();
    assert!(nested.push_data("[DONE]").is_err());
}

#[test]
fn semantic_output_arguments_and_late_reasoning_are_rejected() {
    for field in ["content", "reasoning_content"] {
        let mut decoder = decoder();
        assert!(
            decoder
                .push_data(&frame(
                    json!({field:"é".repeat(MAX_OUTPUT_BYTES/2+1)}),
                    Some("stop")
                ))
                .is_err()
        );
        assert!(decoder.finish_eof().is_err());
    }
    let mut late = decoder();
    late.push_data(&frame(json!({"content":"visible"}), None))
        .unwrap();
    assert!(
        late.push_data(&frame(json!({"reasoning_content":"late"}), None))
            .is_err()
    );
    let mut tool = decoder();
    assert!(
        tool.push_data(&frame(
            json!({"tool_calls":[{"index":0,"id":"a","type":"function",
        "function":{"name":"Read","arguments":"x".repeat(MAX_ARGUMENT_BYTES+1)}}]}),
            Some("tool_calls")
        ))
        .is_err()
    );
    let mut mixed = decoder();
    mixed
        .push_data(&frame(json!({"reasoning_content":"x".repeat(4096)}), None))
        .unwrap();
    assert!(
        mixed
            .push_data(&frame(json!({"content":"y".repeat(4097)}), Some("stop")))
            .is_err()
    );
}

#[test]
fn usage_is_single_terminal_and_never_guessed() {
    let usage = json!({"prompt_tokens":1,"completion_tokens":2,"total_tokens":3});
    let mut premature = decoder();
    let mut first: serde_json::Value =
        serde_json::from_str(&frame(json!({"content":"ok"}), None)).unwrap();
    first["usage"] = usage.clone();
    assert!(premature.push_data(&first.to_string()).is_err());
    let mut duplicate = decoder();
    duplicate
        .push_data(&frame(json!({"content":"ok"}), Some("stop")))
        .unwrap();
    let tail =
        json!({"id":"chatcmpl-fixture","model":"glm-5.3","choices":[],"usage":usage}).to_string();
    duplicate.push_data(&tail).unwrap();
    assert!(duplicate.push_data(&tail).is_err());
    let mut unfinished = decoder();
    unfinished
        .push_data(&frame(json!({"content":"ok"}), Some("stop")))
        .unwrap();
    assert!(unfinished.finish_eof().is_err());
}
