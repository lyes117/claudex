use codex_extension_api::ExtensionTurnItem;
use codex_extension_items::ExtensionItem;
use codex_extension_items::file_tool::FileToolItem;
use codex_extension_items::file_tool::FileToolStatus;
use serde_json::Value;
use serde_json::json;

use crate::FileTool;
use crate::MAX_RESPONSE_BYTES;

#[derive(Clone)]
pub(crate) struct Completion {
    pub(crate) success: bool,
    pub(crate) output: String,
}

impl Completion {
    pub(crate) fn new(success: bool, output: impl ToString) -> Self {
        let mut output = output.to_string();
        if output.len() > MAX_RESPONSE_BYTES {
            let mut end = MAX_RESPONSE_BYTES - 64;
            while !output.is_char_boundary(end) {
                end -= 1;
            }
            output.truncate(end);
            output.push_str("\n[Display truncated at 8 KiB]");
        }
        Self { success, output }
    }

    pub(crate) fn apply(self, mut item: FileToolItem, duration_ms: Option<i64>) -> FileToolItem {
        item.status = if self.success {
            FileToolStatus::Completed
        } else {
            FileToolStatus::Failed
        };
        item.success = Some(self.success);
        item.output = Some(self.output);
        item.duration_ms = duration_ms;
        item
    }
}

pub(crate) fn started_item(call_id: &str, tool: FileTool, arguments: &str) -> FileToolItem {
    // Display metadata only: charge escaped JSON before keeping arguments.
    let arguments = serde_json::from_str::<Value>(arguments)
        .ok()
        .filter(|value| value.to_string().len() <= 4096)
        .unwrap_or_else(|| json!({"display": "Arguments omitted (invalid or over 4 KiB)"}));
    FileToolItem {
        id: call_id.to_owned(),
        tool: tool.name().to_owned(),
        arguments,
        status: FileToolStatus::InProgress,
        output: None,
        success: None,
        duration_ms: None,
    }
}

pub(crate) fn envelope(item: FileToolItem) -> ExtensionTurnItem {
    ExtensionTurnItem {
        item: ExtensionItem::FileTool(item),
        legacy_events: Vec::new(),
    }
}
