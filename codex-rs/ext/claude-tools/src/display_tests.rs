use std::sync::Arc;
use std::sync::Mutex;

use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_items::ExtensionItem;
use codex_extension_items::file_tool::FileToolStatus;
use codex_protocol::protocol::TruncationPolicy;
use codex_tools::ConversationHistory;
use codex_tools::ExtensionTurnItem;
use codex_tools::ToolCallSource;
use codex_tools::ToolPayload;
use codex_tools::TurnItemEmissionFuture;
use codex_tools::TurnItemEmitter;
use pretty_assertions::assert_eq;

use crate::FileTool;
use crate::MAX_RESPONSE_BYTES;
use crate::ToolName;
use crate::tests::environment;

#[derive(Default)]
struct Capture(Mutex<Vec<(&'static str, ExtensionTurnItem)>>);

impl TurnItemEmitter for Capture {
    fn emit_started<'a>(&'a self, item: ExtensionTurnItem) -> TurnItemEmissionFuture<'a> {
        self.0.lock().unwrap().push(("started", item));
        Box::pin(std::future::ready(()))
    }

    fn emit_completed<'a>(&'a self, item: ExtensionTurnItem) -> TurnItemEmissionFuture<'a> {
        self.0.lock().unwrap().push(("completed", item));
        Box::pin(std::future::ready(()))
    }
}

#[tokio::test]
async fn file_tool_cards_reflect_real_success_and_failure_once() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("sample"), "SYNTHETIC_CONTENT").unwrap();
    for file_path in ["sample", "missing"] {
        let capture = Arc::new(Capture::default());
        let call = ToolCall {
            turn_id: "turn".into(),
            call_id: "read".into(),
            tool_name: ToolName::plain("Read"),
            model: "fixture".into(),
            codex_turn_metadata: None,
            truncation_policy: TruncationPolicy::Bytes(MAX_RESPONSE_BYTES),
            source: ToolCallSource::Direct,
            conversation_history: ConversationHistory::default(),
            turn_item_emitter: capture.clone(),
            environments: vec![environment(directory.path())],
            payload: ToolPayload::Function {
                arguments: serde_json::json!({"file_path":file_path}).to_string(),
            },
        };
        let result = FileTool::Read.handle(call).await;
        let captured = capture.0.lock().unwrap();
        assert_eq!(captured.len(), 2);
        assert_eq!([captured[0].0, captured[1].0], ["started", "completed"]);
        let ExtensionItem::FileTool(started) = &captured[0].1.item else {
            panic!("file item")
        };
        let ExtensionItem::FileTool(completed) = &captured[1].1.item else {
            panic!("file item")
        };
        assert_eq!(started.status, FileToolStatus::InProgress);
        assert_eq!(started.id, completed.id);
        assert_eq!(completed.success, Some(result.is_ok()));
        assert_eq!(
            completed.status,
            if result.is_ok() {
                FileToolStatus::Completed
            } else {
                FileToolStatus::Failed
            }
        );
        assert!(completed.duration_ms.is_some());
        assert!(completed.output.as_ref().unwrap().len() <= MAX_RESPONSE_BYTES);
        assert_eq!(
            completed
                .output
                .as_ref()
                .unwrap()
                .contains("SYNTHETIC_CONTENT"),
            result.is_ok()
        );
        assert!(
            captured
                .iter()
                .all(|(_, item)| item.legacy_events.is_empty())
        );
    }
}
