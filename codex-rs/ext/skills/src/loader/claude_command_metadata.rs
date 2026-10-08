//! Invocation metadata derived only from already parsed Claude Markdown.

use codex_skills::ClaudeCommandMetadata;
use serde_json::Value;

pub(super) fn parse(metadata: &Value) -> Result<ClaudeCommandMetadata, String> {
    let user_invocable = match metadata.get("user-invocable") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(value)) => *value,
        Some(_) => return Err("Invalid Claude user-invocable metadata".to_string()),
    };
    let argument_hint = match metadata.get("argument-hint") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) if value.len() <= 1024 => Some(value.clone()),
        Some(_) => return Err("Invalid Claude argument-hint metadata".to_string()),
    };
    Ok(ClaudeCommandMetadata {
        user_invocable,
        argument_hint,
    })
}

/// Bound the actual byte stream before UTF-8 decoding or Markdown parsing.
pub(crate) async fn read_command_text(
    file_system: &dyn codex_exec_server::ExecutorFileSystem,
    path: &codex_utils_path_uri::PathUri,
) -> std::io::Result<String> {
    use futures::StreamExt;
    let mut stream = file_system.read_file_stream(path, /*sandbox*/ None).await?;
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if bytes.len().saturating_add(chunk.len()) > 32 * 1024 {
            return Err(std::io::Error::other(
                "Claude command exceeds the 32 KiB source limit",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes).map_err(|_| std::io::Error::other("Invalid Claude command text"))
}

#[cfg(test)]
#[path = "claude_command_metadata_tests.rs"]
mod tests;
