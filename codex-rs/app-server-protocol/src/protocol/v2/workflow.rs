use crate::JsonSchema;
use crate::TS;
use serde::Deserialize;
use serde::Serialize;

/// Queue a native workflow on an existing thread, preserving that thread's authority.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "v2/")]
pub struct WorkflowStartParams {
    pub thread_id: String,
    pub script_path: std::path::PathBuf,
    pub args: serde_json::Value,
    pub run_id: String,
}

/// Acceptance is not completion. Observe the native workflow and child events.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct WorkflowStartResponse {
    pub run_id: String,
    pub submission_id: String,
}
