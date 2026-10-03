//! Original Claude-format read-only tools, executed through the host filesystem.
//! ToolPolicy, Claude permission gates and native hooks remain host-owned.

use std::sync::Arc;
use std::time::Duration;

use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::FunctionCallError;
use codex_extension_api::JsonToolOutput;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolContributor;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolExecutorFuture;
use codex_extension_api::ToolName;
use codex_extension_api::ToolSpec;
use codex_tools::ToolEnvironment;
use codex_utils_path_uri::PathConvention;
use serde_json::Value;

mod display;
mod read;
mod search;
mod spec;

const MAX_FILE_BYTES: usize = 1024 * 1024;
// Even adversarial one-token-per-byte ASCII remains below the 10K-token item cap.
const MAX_RESPONSE_BYTES: usize = 8 * 1024;

#[derive(Clone, Copy)]
enum FileTool {
    Read,
    Glob,
    Grep,
}

impl FileTool {
    fn name(self) -> &'static str {
        match self {
            Self::Read => "Read",
            Self::Glob => "Glob",
            Self::Grep => "Grep",
        }
    }
}

impl<'call> ToolExecutor<ToolCall<'call>> for FileTool {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(self.name())
    }

    fn spec(&self) -> ToolSpec {
        spec::file_tool_spec(*self)
    }

    fn yields_to_client_tools(&self) -> bool {
        true
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn handle<'a>(&'a self, call: ToolCall<'call>) -> ToolExecutorFuture<'a>
    where
        'call: 'a,
    {
        Box::pin(async move {
            let arguments = call.function_arguments().map_err(|failure| match failure {
                FunctionCallError::Fatal(_) => FunctionCallError::Fatal(
                    "File tool invoked with incompatible payload".to_owned(),
                ),
                FunctionCallError::RespondToModel(message) => error(message),
            })?;
            if arguments.len() > 32 * 1024 {
                return Err(error("File tool arguments exceed 32 KiB"));
            }
            let environment = match call.environments.as_slice() {
                [environment] => environment,
                _ => return Err(error("File tools require exactly one host environment")),
            };
            validate_environment(environment)?;
            let budget = call.response_byte_budget(MAX_RESPONSE_BYTES);
            let display = display::DisplayCall::start(&call, *self, arguments).await;
            let result = tokio::time::timeout(Duration::from_secs(30), async {
                match self {
                    Self::Read => read::read(environment, arguments, budget).await,
                    Self::Glob | Self::Grep => {
                        search::search(environment, arguments, budget, matches!(self, Self::Grep))
                            .await
                    }
                }
            })
            .await
            .map_err(|_| error("File tool exceeded its 30 second limit"))
            .and_then(|result| result)
            .and_then(|result| {
                if result.to_string().len() > budget {
                    Err(error("File tool response budget is too small"))
                } else {
                    Ok(result)
                }
            });
            display.finish(&call, &result).await;
            let result = result?;
            Ok(Box::new(JsonToolOutput::new(result).with_external_context()) as _)
        })
    }
}

struct FileTools;

impl ToolContributor for FileTools {
    fn tools(
        &self,
        _: &ExtensionData,
        _: &ExtensionData,
    ) -> Vec<Arc<dyn for<'call> ToolExecutor<ToolCall<'call>>>> {
        [FileTool::Read, FileTool::Glob, FileTool::Grep]
            .into_iter()
            .map(|tool| Arc::new(tool) as Arc<dyn for<'call> ToolExecutor<ToolCall<'call>>>)
            .collect()
    }
}

pub fn install<C: Sync>(registry: &mut ExtensionRegistryBuilder<C>) {
    registry.tool_contributor(Arc::new(FileTools));
}

fn error(message: impl std::fmt::Display) -> FunctionCallError {
    let mut message = message.to_string();
    if message.len() > MAX_RESPONSE_BYTES {
        let mut end = MAX_RESPONSE_BYTES - 64;
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        message.truncate(end);
        message.push_str("\n[Error truncated at 8 KiB]");
    }
    FunctionCallError::RespondToModel(message)
}

fn parse<T: serde::de::DeserializeOwned>(arguments: &str) -> Result<T, FunctionCallError> {
    serde_json::from_str(arguments).map_err(|_| error("Invalid or unsupported file tool arguments"))
}

fn validate_environment(environment: &ToolEnvironment<'_>) -> Result<(), FunctionCallError> {
    let sandbox = &environment.file_system_sandbox_context;
    if sandbox.should_read_from_sandbox()
        && environment.cwd.infer_path_convention() == Some(PathConvention::Windows)
        && !sandbox.windows_sandbox_is_requested()
    {
        return Err(error(
            "Required filesystem read sandbox is unavailable on this Windows environment",
        ));
    }
    Ok(())
}

fn append_bounded(rows: &mut Vec<Value>, row: Value, budget: usize) -> bool {
    // JSON escaping, including control characters, is charged before insertion.
    let size = rows
        .iter()
        .map(|value| value.to_string().len() + 1)
        .sum::<usize>();
    if size
        .saturating_add(row.to_string().len())
        .saturating_add(512)
        > budget
    {
        return false;
    }
    rows.push(row);
    true
}

#[cfg(test)]
#[path = "file_tools_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "file_system_tests.rs"]
mod test_file_system;

#[cfg(test)]
#[path = "display_tests.rs"]
mod display_tests;

#[cfg(test)]
#[path = "error_bounds_tests.rs"]
mod error_bounds_tests;
