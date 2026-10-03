use std::time::Instant;

use codex_extension_api::ExtensionTurnItem;
use codex_extension_api::FunctionCallError;
use codex_extension_api::ToolCall;
use codex_extension_items::ExtensionItem;
use codex_extension_items::file_tool::FileToolItem;
use codex_extension_items::file_tool::FileToolStatus;
use serde_json::Value;
use serde_json::json;

use crate::FileTool;
use crate::MAX_RESPONSE_BYTES;

pub(crate) struct DisplayCall {
    item: FileToolItem,
    started: Instant,
}

impl DisplayCall {
    pub(crate) async fn start(call: &ToolCall<'_>, tool: FileTool, arguments: &str) -> Self {
        // The UI carries display metadata, never an additional model-context item.
        // Charge escaped JSON, not input characters, before keeping arguments.
        let arguments = serde_json::from_str::<Value>(arguments)
            .ok()
            .filter(|value| value.to_string().len() <= 4096)
            .unwrap_or_else(|| json!({"display": "Arguments omitted (invalid or over 4 KiB)"}));
        let item = FileToolItem {
            id: call.call_id.clone(),
            tool: tool.name().to_owned(),
            arguments,
            status: FileToolStatus::InProgress,
            output: None,
            success: None,
            duration_ms: None,
        };
        call.turn_item_emitter
            .emit_started(envelope(item.clone()))
            .await;
        Self {
            item,
            started: Instant::now(),
        }
    }

    pub(crate) async fn finish(
        mut self,
        call: &ToolCall<'_>,
        result: &Result<Value, FunctionCallError>,
    ) {
        let success = result.is_ok();
        self.item.status = if success {
            FileToolStatus::Completed
        } else {
            FileToolStatus::Failed
        };
        self.item.success = Some(success);
        self.item.duration_ms = i64::try_from(self.started.elapsed().as_millis()).ok();
        let mut output = match result {
            Ok(value) => value.to_string(),
            Err(error) => error.to_string(),
        };
        if output.len() > MAX_RESPONSE_BYTES {
            let mut end = MAX_RESPONSE_BYTES - 64;
            while !output.is_char_boundary(end) {
                end -= 1;
            }
            output.truncate(end);
            output.push_str("\n[Display truncated at 8 KiB]");
        }
        self.item.output = Some(output);
        call.turn_item_emitter
            .emit_completed(envelope(self.item))
            .await;
    }
}

fn envelope(item: FileToolItem) -> ExtensionTurnItem {
    ExtensionTurnItem {
        item: ExtensionItem::FileTool(item),
        legacy_events: Vec::new(),
    }
}
