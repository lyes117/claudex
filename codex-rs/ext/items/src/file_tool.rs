use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use ts_rs::TS;

/// Display-only lifecycle for a host-executed file tool, never a client request.
#[derive(Debug, Clone, Deserialize, Serialize, TS, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct FileToolItem {
    pub id: String,
    pub tool: String,
    pub arguments: Value,
    pub status: FileToolStatus,
    pub output: Option<String>,
    pub success: Option<bool>,
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, TS, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum FileToolStatus {
    InProgress,
    Completed,
    Failed,
}
