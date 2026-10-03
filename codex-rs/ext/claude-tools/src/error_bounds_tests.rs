use std::sync::Arc;

use codex_protocol::protocol::TruncationPolicy;
use codex_tools::ConversationHistory;
use codex_tools::NoopTurnItemEmitter;
use codex_tools::ToolCallSource;
use codex_tools::ToolPayload;

use crate::*;

#[tokio::test]
async fn filesystem_errors_are_bounded_before_direct_and_code_mode_propagation() {
    let directory = tempfile::tempdir().unwrap();
    let mut environment = tests::environment(directory.path());
    let mut file_system =
        test_file_system::CheckedFileSystem::new(environment.file_system_sandbox_context.clone());
    file_system.metadata_error = Some(format!("{}TAIL_MUST_BE_REMOVED", "éj~%#;💥".repeat(4000)));
    environment.file_system = Arc::new(file_system);
    for source in [
        ToolCallSource::Direct,
        ToolCallSource::CodeMode {
            cell_id: "cell".into(),
            runtime_tool_call_id: "nested".into(),
        },
    ] {
        let call = ToolCall {
            turn_id: "turn".into(),
            call_id: "error".into(),
            tool_name: ToolName::plain("Read"),
            model: "fixture".into(),
            codex_turn_metadata: None,
            truncation_policy: TruncationPolicy::Bytes(MAX_RESPONSE_BYTES),
            source,
            conversation_history: ConversationHistory::default(),
            turn_item_emitter: Arc::new(NoopTurnItemEmitter),
            environments: vec![environment.clone()],
            payload: ToolPayload::Function {
                arguments: r#"{"file_path":"fixture"}"#.into(),
            },
        };
        let message = FileTool::Read
            .handle(call)
            .await
            .err()
            .expect("The filesystem must fail")
            .to_string();
        assert!(
            message.len() <= MAX_RESPONSE_BYTES,
            "Filesystem errors must obey the model-context byte cap"
        );
        assert!(
            message.len() > 8000,
            "Exercise truncation, not a short validation error"
        );
        assert!(!message.contains("TAIL_MUST_BE_REMOVED"));
        assert!(message.ends_with("[Error truncated at 8 KiB]"));
    }
}

#[tokio::test]
async fn incompatible_payload_does_not_echo_an_arbitrary_call_name() {
    let call = ToolCall {
        turn_id: "turn".into(),
        call_id: "invalid".into(),
        tool_name: ToolName::plain("SYNTHETIC_CALL_NAME".repeat(2000)),
        model: "fixture".into(),
        codex_turn_metadata: None,
        truncation_policy: TruncationPolicy::Bytes(MAX_RESPONSE_BYTES),
        source: ToolCallSource::Direct,
        conversation_history: ConversationHistory::default(),
        turn_item_emitter: Arc::new(NoopTurnItemEmitter),
        environments: Vec::new(),
        payload: ToolPayload::Custom {
            input: "invalid".into(),
        },
    };
    let failure = FileTool::Read
        .handle(call)
        .await
        .err()
        .expect("Reject the incompatible payload");
    assert!(matches!(failure, FunctionCallError::Fatal(_)));
    assert!(failure.to_string().len() <= MAX_RESPONSE_BYTES);
    assert!(!failure.to_string().contains("SYNTHETIC_CALL_NAME"));
}
