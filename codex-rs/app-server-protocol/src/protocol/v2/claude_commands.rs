use crate::JsonSchema;
use crate::TS;
use serde::Deserialize;
use serde::Serialize;

/// Claude Markdown invocation metadata. An absent value keeps older catalogs compatible.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ClaudeCommandMetadata {
    pub user_invocable: bool,
    #[serde(default)]
    pub argument_hint: Option<String>,
}

#[cfg(test)]
#[path = "claude_commands_tests.rs"]
mod tests;

/// Resolve one explicitly invoked command from this thread's live skill catalog.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ClaudeCommandExpandParams {
    pub thread_id: String,
    pub cwd: std::path::PathBuf,
    pub name: String,
    pub path: codex_utils_absolute_path::AbsolutePathBuf,
    pub arguments: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ClaudeCommandExpandResponse {
    pub text: String,
}
