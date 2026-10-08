//! Observable native run state. Labels never authorize operations on a thread.
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowAgentStatus {
    pub label: String,
    pub status: String,
    pub thread_id: String,
    pub model: String,
    pub provider: String,
    pub effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRunStatus {
    pub run_id: String,
    pub status: String,
    pub phase: String,
    pub started_at: i64,
    pub updated_at: i64,
    pub parent_thread_id: String,
    pub token: String,
    pub control_revision: Option<String>,
    pub agents: Vec<WorkflowAgentStatus>,
    pub logs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowControlRequest {
    pub token: String,
    pub revision: String,
    pub action: WorkflowControlAction,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowControlAction {
    Pause,
    Resume,
    Stop,
}

pub fn valid_run_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}
